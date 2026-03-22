//! FFmpeg RTMP muxer: host video + translated audio → RTMP streams.
//!
//! Each RTMP endpoint gets its own FFmpeg process:
//! - Video: JPEG frames piped to stdin (image2pipe) with adaptive delay
//! - Audio: PCM s16le written to a named FIFO (silence when idle, TTS audio when available)
//!
//! Video frames are buffered and delayed to sync with the TTS audio pipeline.
//! The delay adapts based on a rolling average of measured pipeline latency.

use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};

/// A video frame waiting to be released after a delay
struct DelayedFrame {
    data: Vec<u8>,
    release_at: Instant,
}

/// Handle for one FFmpeg RTMP process
struct RtmpStream {
    child: Child,
    video_tx: mpsc::UnboundedSender<Vec<u8>>,  // raw JPEG bytes (after delay)
    audio_tx: mpsc::UnboundedSender<Vec<u8>>,  // raw PCM s16le bytes
    delay_tx: mpsc::UnboundedSender<Vec<u8>>,  // raw JPEG bytes (before delay)
    audio_fifo: String,                         // FIFO path for cleanup
    lang: String,                               // language code
}

/// Manages all FFmpeg RTMP streams for a session
pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,  // stream_id → handle
    /// Rolling average pipeline latency in ms (STT + translate + TTS)
    pipeline_delay_ms: Arc<AtomicU64>,
}

// 20ms of silence at 44100Hz, 16-bit mono = 1764 bytes
const SILENCE_CHUNK_SIZE: usize = 1764;
const SILENCE_INTERVAL_MS: u64 = 20;
// Initial delay before any pipeline measurements (conservative default)
const INITIAL_DELAY_MS: u64 = 500;
// Minimum delay to prevent zero-delay (pipeline always has some latency)
const MIN_DELAY_MS: u64 = 200;
// Maximum delay to cap buffering
const MAX_DELAY_MS: u64 = 3000;
// How fast the rolling average adapts (0.0–1.0, higher = faster)
const EMA_ALPHA: f64 = 0.3;

impl RtmpManager {
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            pipeline_delay_ms: Arc::new(AtomicU64::new(INITIAL_DELAY_MS)),
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

        // Video writer channel (receives frames AFTER delay)
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

        // Video delay buffer: receives frames immediately, releases after delay
        let (delay_tx, mut delay_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let video_tx_delayed = video_tx.clone();
        let delay_ref = self.pipeline_delay_ms.clone();
        let _stream_id_d = stream_id.to_string();
        tokio::spawn(async move {
            let mut buffer: VecDeque<DelayedFrame> = VecDeque::new();
            let tick = Duration::from_millis(5); // check buffer every 5ms

            loop {
                // Drain all available incoming frames without blocking
                loop {
                    match delay_rx.try_recv() {
                        Ok(data) => {
                            let delay_ms = delay_ref.load(Ordering::Relaxed);
                            let release_at = Instant::now() + Duration::from_millis(delay_ms);
                            buffer.push_back(DelayedFrame { data, release_at });
                        }
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            // Flush remaining frames
                            while let Some(frame) = buffer.pop_front() {
                                let _ = video_tx_delayed.send(frame.data);
                            }
                            return;
                        }
                    }
                }

                // Release frames whose delay has elapsed
                let now = Instant::now();
                while let Some(front) = buffer.front() {
                    if now >= front.release_at {
                        let frame = buffer.pop_front().unwrap();
                        if video_tx_delayed.send(frame.data).is_err() {
                            return;
                        }
                    } else {
                        break;
                    }
                }

                tokio::time::sleep(tick).await;
            }
        });

        // Audio writer channel
        let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let fifo_path = audio_fifo.clone();
        let stream_id_a = stream_id.to_string();
        tokio::spawn(async move {
            // Open FIFO for writing (blocks until FFmpeg opens it for reading)
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
                // Try to receive audio data with a timeout
                match tokio::time::timeout(
                    Duration::from_millis(SILENCE_INTERVAL_MS),
                    audio_rx.recv(),
                )
                .await
                {
                    Ok(Some(pcm_data)) => {
                        // Write real audio
                        if fifo.write_all(&pcm_data).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => break, // Channel closed
                    Err(_) => {
                        // Timeout — write silence
                        if fifo.write_all(&silence).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        eprintln!(
            "[FFMPEG] Started RTMP stream {} ({}) → {} [delay={}ms]",
            stream_id, lang, rtmp_url, self.pipeline_delay_ms.load(Ordering::Relaxed)
        );

        self.streams.insert(
            stream_id.to_string(),
            RtmpStream {
                child,
                video_tx,
                audio_tx,
                delay_tx,
                audio_fifo,
                lang: lang.to_string(),
            },
        );

        Ok(())
    }

    /// Push a video frame to ALL RTMP streams (enters delay buffer first)
    pub fn push_video_frame(&self, jpeg_bytes: &[u8]) {
        for stream in self.streams.values() {
            let _ = stream.delay_tx.send(jpeg_bytes.to_vec());
        }
    }

    /// Push decoded PCM audio to streams matching a specific language
    pub fn push_audio_pcm(&self, lang: &str, pcm: &[u8]) {
        for stream in self.streams.values() {
            if stream.lang == lang {
                let _ = stream.audio_tx.send(pcm.to_vec());
            }
        }
    }

    /// Update the adaptive pipeline delay based on measured latency.
    /// Called after each TTS round-trip completes.
    pub fn update_pipeline_delay(&self, measured_ms: u64) {
        let current = self.pipeline_delay_ms.load(Ordering::Relaxed) as f64;
        let new_avg = current * (1.0 - EMA_ALPHA) + measured_ms as f64 * EMA_ALPHA;
        let clamped = (new_avg as u64).clamp(MIN_DELAY_MS, MAX_DELAY_MS);
        let prev = self.pipeline_delay_ms.swap(clamped, Ordering::Relaxed);
        if prev != clamped {
            eprintln!("[SYNC] pipeline delay updated: {}ms → {}ms (measured: {}ms)", prev, clamped, measured_ms);
        }
    }

    /// Stop all FFmpeg processes and clean up FIFOs
    pub async fn stop_all(&mut self) {
        for (id, mut stream) in self.streams.drain() {
            drop(stream.delay_tx);
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
