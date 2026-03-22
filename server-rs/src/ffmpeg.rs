//! FFmpeg RTMP muxer: host video + translated audio → RTMP streams.
//!
//! Implements the "Broadcast Delay" pattern for video-audio synchronization:
//!
//! 1. Video frames are buffered with capture timestamps
//! 2. A per-stream drain loop ticks at exactly 30fps (33.33ms)
//! 3. Each tick emits the frame from `now - D` (D = broadcast delay, e.g. 2500ms)
//! 4. TTS audio is queued and released when the delayed timeline reaches utterance_start
//! 5. Between utterances, silence is written to maintain the audio timeline
//!
//! This ensures FFmpeg always receives a steady 30fps video + continuous audio,
//! eliminating the freeze→fast-forward jitter from burst-releasing frames.

use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

/// Audio waiting to be played at the right point in the delayed timeline
struct QueuedAudio {
    /// Source timestamp when this utterance started (host speaking)
    play_at: Instant,
    /// Raw PCM s16le 44100Hz mono
    pcm: Vec<u8>,
}

/// State for draining queued audio chunk-by-chunk at 30fps cadence
struct ActiveAudio {
    pcm: Vec<u8>,
    offset: usize,
}

/// Handle for one FFmpeg RTMP stream (per language/platform)
struct RtmpStream {
    child: Child,
    drain_handle: JoinHandle<()>,
    audio_fifo: String,
    lang: String,
    /// Per-stream audio queue: TTS audio waiting to be released at the right time
    audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
}

/// Manages all FFmpeg RTMP streams for a session.
///
/// Uses a shared frame buffer + per-stream drain loops for synchronized output.
pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
    /// Shared ring buffer of timestamped video frames from the host webcam
    frame_buffer: Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    /// Fixed broadcast delay applied to all streams
    broadcast_delay: Duration,
}

// 33.33ms per frame at 30fps
const FRAME_INTERVAL_NS: u64 = 33_333_333;
// Audio bytes per 33.33ms tick: 44100Hz × 2 bytes × 0.03333s ≈ 2940 bytes
const AUDIO_BYTES_PER_TICK: usize = 2940;
// Max frames to keep in the buffer (broadcast_delay + 2s margin at 30fps)
const MAX_BUFFER_FRAMES: usize = 450;
// Default broadcast delay
const DEFAULT_DELAY_MS: u64 = 2500;

impl RtmpManager {
    pub fn new() -> Self {
        let delay_ms: u64 = std::env::var("BROADCAST_DELAY_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_DELAY_MS);

        eprintln!("[SYNC] Broadcast delay: {}ms", delay_ms);

        Self {
            streams: HashMap::new(),
            frame_buffer: Arc::new(StdMutex::new(VecDeque::new())),
            broadcast_delay: Duration::from_millis(delay_ms),
        }
    }

    /// Start an FFmpeg RTMP process with its own drain loop
    pub async fn start_stream(
        &mut self,
        stream_id: &str,
        lang: &str,
        rtmp_url: &str,
    ) -> Result<(), String> {
        let audio_fifo = format!("/tmp/brivva_audio_{}", stream_id);

        // Create named FIFO
        let _ = std::fs::remove_file(&audio_fifo);
        std::process::Command::new("mkfifo")
            .arg(&audio_fifo)
            .output()
            .map_err(|e| format!("mkfifo failed: {}", e))?;

        // Start FFmpeg
        let mut child = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel", "warning",
                // Video input: JPEG frames from stdin
                "-f", "image2pipe",
                "-framerate", "30",
                "-i", "pipe:0",
                // Audio input: raw PCM from FIFO (mono input)
                "-f", "s16le",
                "-ar", "44100",
                "-ac", "1",
                "-i", &audio_fifo,
                // Video encoding
                "-c:v", "libx264",
                "-preset", "ultrafast",
                "-tune", "zerolatency",
                "-b:v", "2500k",
                "-maxrate", "2500k",
                "-bufsize", "5000k",
                "-pix_fmt", "yuv420p",
                "-g", "60",
                // Audio encoding (stereo AAC)
                "-c:a", "aac",
                "-ac:a", "2",
                "-b:a", "128k",
                // Mapping
                "-map", "0:v",
                "-map", "1:a",
                // Output
                "-f", "flv",
                rtmp_url,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;

        let stdin = child.stdin.take().ok_or("No FFmpeg stdin")?;

        // Per-stream audio queue
        let audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>> =
            Arc::new(StdMutex::new(VecDeque::new()));

        // Spawn the drain loop for this stream
        let frame_buf = self.frame_buffer.clone();
        let aq = audio_queue.clone();
        let delay = self.broadcast_delay;
        let fifo_path = audio_fifo.clone();
        let sid = stream_id.to_string();

        let drain_handle = tokio::spawn(async move {
            drain_loop(sid, frame_buf, aq, stdin, fifo_path, delay).await;
        });

        eprintln!(
            "[FFMPEG] Started RTMP stream {} ({}) → {} [delay={}ms]",
            stream_id,
            lang,
            rtmp_url,
            self.broadcast_delay.as_millis()
        );

        self.streams.insert(
            stream_id.to_string(),
            RtmpStream {
                child,
                drain_handle,
                audio_fifo,
                lang: lang.to_string(),
                audio_queue,
            },
        );

        Ok(())
    }

    /// Buffer a video frame from the host webcam.
    /// Frames are stored with their capture timestamp and picked up by drain loops.
    pub fn push_video_frame(&self, jpeg_bytes: &[u8]) {
        let mut buf = self.frame_buffer.lock().unwrap();
        buf.push_back((Instant::now(), jpeg_bytes.to_vec()));

        // Prune old frames that are well past the delay window
        while buf.len() > MAX_BUFFER_FRAMES {
            buf.pop_front();
        }
    }

    /// Queue translated audio to be played at the right point in the delayed timeline.
    /// The drain loop will pick it up when the delayed clock reaches `utterance_start`.
    pub fn queue_audio(&self, lang: &str, pcm: Vec<u8>, utterance_start: Instant) {
        for stream in self.streams.values() {
            if stream.lang == lang {
                let mut q = stream.audio_queue.lock().unwrap();
                q.push_back(QueuedAudio {
                    play_at: utterance_start,
                    pcm,
                });
                return;
            }
        }
    }

    /// Stop all FFmpeg processes and clean up
    pub async fn stop_all(&mut self) {
        for (id, mut stream) in self.streams.drain() {
            stream.drain_handle.abort();
            match stream.child.kill().await {
                Ok(_) => eprintln!("[FFMPEG:{}] killed", id),
                Err(e) => eprintln!("[FFMPEG:{}] kill error: {}", id, e),
            }
            let _ = std::fs::remove_file(&stream.audio_fifo);
        }
    }
}

/// Thread-safe wrapper
pub type SharedRtmpManager = Arc<tokio::sync::Mutex<RtmpManager>>;

/// The core synchronization loop for one RTMP stream.
///
/// Ticks at exactly 30fps. On each tick:
/// 1. Picks the video frame from `now - delay` in the shared buffer
/// 2. Writes it to FFmpeg stdin (or repeats the last frame)
/// 3. Checks the audio queue for audio that should play at this point
/// 4. Writes audio chunk (from TTS or silence) to the FIFO
///
/// Both video and audio advance at the same rate, keeping them perfectly synced.
async fn drain_loop(
    stream_id: String,
    frame_buffer: Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    mut stdin: tokio::process::ChildStdin,
    fifo_path: String,
    delay: Duration,
) {
    // Open FIFO for writing (blocks until FFmpeg opens it for reading)
    let fifo = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&fifo_path)
        .await;
    let mut fifo = match fifo {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[DRAIN:{}] failed to open audio FIFO: {}", stream_id, e);
            return;
        }
    };

    let mut interval = tokio::time::interval(Duration::from_nanos(FRAME_INTERVAL_NS));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut last_frame: Option<Vec<u8>> = None;
    let mut active_audio: Option<ActiveAudio> = None;
    let silence = vec![0u8; AUDIO_BYTES_PER_TICK];

    eprintln!("[DRAIN:{}] drain loop started", stream_id);

    loop {
        interval.tick().await;
        let target_ts = Instant::now() - delay;

        // ── VIDEO ──────────────────────────────────────────
        let frame = {
            let buf = frame_buffer.lock().unwrap();
            find_frame_at(&buf, target_ts)
        };

        if let Some(f) = frame {
            if stdin.write_all(&f).await.is_err() {
                eprintln!("[DRAIN:{}] video write error, exiting", stream_id);
                break;
            }
            last_frame = Some(f);
        } else if let Some(ref lf) = last_frame {
            // No frame at target_ts yet — repeat last frame to maintain 30fps
            if stdin.write_all(lf).await.is_err() {
                eprintln!("[DRAIN:{}] video write error, exiting", stream_id);
                break;
            }
        }
        // else: no frames yet at all (startup), skip video this tick

        // ── AUDIO ──────────────────────────────────────────
        // Check if a new queued audio should start playing
        if active_audio.is_none() {
            let mut q = audio_queue.lock().unwrap();
            if let Some(front) = q.front() {
                if target_ts >= front.play_at {
                    let audio = q.pop_front().unwrap();
                    active_audio = Some(ActiveAudio {
                        pcm: audio.pcm,
                        offset: 0,
                    });
                }
            }
        }

        // Write one tick's worth of audio (2940 bytes = 33.33ms at 44100Hz mono 16-bit)
        let audio_result = if let Some(ref mut active) = active_audio {
            let remaining = active.pcm.len() - active.offset;
            if remaining >= AUDIO_BYTES_PER_TICK {
                let end = active.offset + AUDIO_BYTES_PER_TICK;
                let r = fifo.write_all(&active.pcm[active.offset..end]).await;
                active.offset = end;
                r
            } else if remaining > 0 {
                // Last partial chunk — pad with silence
                let mut chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
                chunk[..remaining].copy_from_slice(&active.pcm[active.offset..]);
                active_audio = None;
                fifo.write_all(&chunk).await
            } else {
                active_audio = None;
                fifo.write_all(&silence).await
            }
        } else {
            fifo.write_all(&silence).await
        };

        if audio_result.is_err() {
            eprintln!("[DRAIN:{}] audio write error, exiting", stream_id);
            break;
        }
    }

    drop(stdin);
    eprintln!("[DRAIN:{}] drain loop exited", stream_id);
}

/// Find the latest frame with timestamp <= target in the buffer.
/// Returns None if no frame is old enough yet (initial startup delay).
fn find_frame_at(
    buffer: &VecDeque<(Instant, Vec<u8>)>,
    target: Instant,
) -> Option<Vec<u8>> {
    // Iterate from newest to oldest, return first frame at or before target
    for (ts, data) in buffer.iter().rev() {
        if *ts <= target {
            return Some(data.clone());
        }
    }
    None
}

/// Decode MP3 bytes to raw PCM s16le 44100Hz mono using FFmpeg subprocess
pub async fn decode_mp3_to_pcm(mp3: &[u8]) -> Result<Vec<u8>, String> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-f", "mp3", "-i", "pipe:0", "-f", "s16le", "-ar", "44100", "-ac", "1", "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("FFmpeg decode spawn failed: {}", e))?;

    let mut stdin = child.stdin.take().ok_or("No stdin")?;
    stdin
        .write_all(mp3)
        .await
        .map_err(|e| format!("FFmpeg stdin write failed: {}", e))?;
    drop(stdin);

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("FFmpeg wait failed: {}", e))?;
    if output.stdout.is_empty() {
        return Err("Empty PCM output".to_string());
    }
    Ok(output.stdout)
}
