//! FFmpeg RTMP muxer: host video + translated audio → RTMP streams.
//!
//! Each RTMP endpoint gets its own FFmpeg process:
//! - Video: JPEG frames piped to stdin (image2pipe)
//! - Audio: PCM s16le written to a named FIFO (silence when idle, TTS audio when available)
//!
//! Video frames are buffered and released in sync with TTS audio per-utterance.
//! When audio for an utterance arrives, all frames up to utterance_end are flushed
//! to FFmpeg, followed by the audio — keeping video and translated speech aligned.

use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};

/// Handle for one FFmpeg RTMP process
struct RtmpStream {
    child: Child,
    video_tx: mpsc::UnboundedSender<Vec<u8>>,  // raw JPEG bytes
    audio_tx: mpsc::UnboundedSender<Vec<u8>>,  // raw PCM s16le bytes
    audio_fifo: String,                         // FIFO path for cleanup
    lang: String,                               // language code
}

/// Manages all FFmpeg RTMP streams for a session.
/// Buffers video frames and releases them per-utterance when audio arrives.
pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
    /// Buffered video frames awaiting release. Each frame is (timestamp, JPEG bytes).
    frame_buffer: VecDeque<(Instant, Vec<u8>)>,
}

// 20ms of silence at 44100Hz, 16-bit mono = 1764 bytes
const SILENCE_CHUNK_SIZE: usize = 1764;
const SILENCE_INTERVAL_MS: u64 = 20;
// Max buffered frames before we start draining old ones (10 seconds at 30fps)
const MAX_BUFFERED_FRAMES: usize = 300;

impl RtmpManager {
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            frame_buffer: VecDeque::new(),
        }
    }

    /// Start an FFmpeg RTMP process for a stream
    pub async fn start_stream(
        &mut self,
        stream_id: &str,
        lang: &str,
        rtmp_url: &str,
    ) -> Result<(), String> {
        let audio_fifo = format!("/tmp/brivva_audio_{}", stream_id);

        // Create named FIFO
        let _ = std::fs::remove_file(&audio_fifo); // clean up stale
        std::process::Command::new("mkfifo")
            .arg(&audio_fifo)
            .output()
            .map_err(|e| format!("mkfifo failed: {}", e))?;

        // Start FFmpeg: video from stdin, audio from FIFO
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
                // Audio encoding (stereo AAC for WebRTC compatibility)
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

        // Video writer: receives JPEG bytes and writes to FFmpeg stdin
        let (video_tx, mut video_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let stream_id_v = stream_id.to_string();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(jpeg_bytes) = video_rx.recv().await {
                if stdin.write_all(&jpeg_bytes).await.is_err() {
                    eprintln!("[FFMPEG:{}] video write error", stream_id_v);
                    break;
                }
            }
            drop(stdin);
        });

        // Audio writer: receives PCM bytes and writes to named FIFO
        let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let fifo_path = audio_fifo.clone();
        let stream_id_a = stream_id.to_string();
        tokio::spawn(async move {
            let fifo = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&fifo_path)
                .await;
            let mut fifo = match fifo {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("[FFMPEG:{}] failed to open audio FIFO: {}", stream_id_a, e);
                    return;
                }
            };

            let silence = vec![0u8; SILENCE_CHUNK_SIZE];

            loop {
                match tokio::time::timeout(
                    Duration::from_millis(SILENCE_INTERVAL_MS),
                    audio_rx.recv(),
                )
                .await
                {
                    Ok(Some(pcm_data)) => {
                        if fifo.write_all(&pcm_data).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        if fifo.write_all(&silence).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        eprintln!(
            "[FFMPEG] Started RTMP stream {} ({}) → {}",
            stream_id, lang, rtmp_url
        );

        self.streams.insert(
            stream_id.to_string(),
            RtmpStream {
                child,
                video_tx,
                audio_tx,
                audio_fifo,
                lang: lang.to_string(),
            },
        );

        Ok(())
    }

    /// Buffer a video frame. Frames are held until released by `flush_and_push_audio()`.
    /// If the buffer exceeds MAX_BUFFERED_FRAMES, the oldest frames are drained
    /// to keep FFmpeg fed during long silences.
    pub fn push_video_frame(&mut self, jpeg_bytes: &[u8]) {
        self.frame_buffer.push_back((Instant::now(), jpeg_bytes.to_vec()));

        // Safety valve: if buffer is too large (long silence, no utterances),
        // drain the oldest half to keep the stream alive
        if self.frame_buffer.len() > MAX_BUFFERED_FRAMES {
            let drain_count = self.frame_buffer.len() - MAX_BUFFERED_FRAMES / 2;
            eprintln!(
                "[SYNC] buffer overflow ({} frames), draining {} idle frames",
                self.frame_buffer.len(), drain_count
            );
            for _ in 0..drain_count {
                if let Some((_, data)) = self.frame_buffer.pop_front() {
                    for stream in self.streams.values() {
                        let _ = stream.video_tx.send(data.clone());
                    }
                }
            }
        }
    }

    /// Flush buffered frames up to `utterance_end` and push audio for a language.
    ///
    /// This is the core sync mechanism:
    /// 1. All frames with timestamp <= utterance_end are sent to FFmpeg video
    /// 2. Audio PCM is sent to the matching language stream's FFmpeg audio
    /// 3. Remaining frames (after utterance_end) stay buffered for the next utterance
    pub fn flush_and_push_audio(
        &mut self,
        lang: &str,
        pcm: &[u8],
        utterance_end: Instant,
    ) {
        // Drain frames up to utterance_end
        let mut flushed = 0;
        while let Some((ts, _)) = self.frame_buffer.front() {
            if *ts <= utterance_end {
                let (_, data) = self.frame_buffer.pop_front().unwrap();
                for stream in self.streams.values() {
                    let _ = stream.video_tx.send(data.clone());
                }
                flushed += 1;
            } else {
                break;
            }
        }

        // Push audio to matching language streams
        for stream in self.streams.values() {
            if stream.lang == lang {
                let _ = stream.audio_tx.send(pcm.to_vec());
            }
        }

        eprintln!(
            "[SYNC] flushed {} frames + {}KB audio for {} ({} frames still buffered)",
            flushed,
            pcm.len() / 1024,
            lang,
            self.frame_buffer.len()
        );
    }

    /// Stop all FFmpeg processes and clean up FIFOs
    pub async fn stop_all(&mut self) {
        // Flush any remaining buffered frames
        while let Some((_, data)) = self.frame_buffer.pop_front() {
            for stream in self.streams.values() {
                let _ = stream.video_tx.send(data.clone());
            }
        }

        for (id, mut stream) in self.streams.drain() {
            drop(stream.video_tx);
            drop(stream.audio_tx);
            match stream.child.kill().await {
                Ok(_) => eprintln!("[FFMPEG:{}] killed", id),
                Err(e) => eprintln!("[FFMPEG:{}] kill error: {}", id, e),
            }
            let _ = std::fs::remove_file(&stream.audio_fifo);
        }
    }
}

/// Thread-safe wrapper
pub type SharedRtmpManager = Arc<Mutex<RtmpManager>>;

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
