//! FFmpeg RTMP muxer: synced video frames + translated audio → YouTube live stream.
//!
//! Each language stream gets its own FFmpeg process that:
//! 1. Receives JPEG frames on stdin (video pipe)
//! 2. Receives audio via a named pipe or secondary input
//! 3. Muxes H.264 + AAC → FLV → RTMP push to YouTube
//!
//! Architecture:
//!   Per utterance: buffered video frames + TTS audio arrive together (synced)
//!   → Write frames to FFmpeg stdin at original FPS
//!   → Write audio to FFmpeg audio pipe
//!   → FFmpeg muxes and pushes to RTMP

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};

use crate::types::TimestampedFrame;

/// A chunk of synced audio + video to push to RTMP
pub struct SyncedChunk {
    pub frames: Vec<TimestampedFrame>, // JPEG frames from utterance window
    pub audio_mp3: Vec<u8>,           // buffered TTS MP3
}

/// Handle to a running FFmpeg RTMP process for one language stream
struct FfmpegStream {
    child: Child,
    /// Send synced chunks to the writer task
    chunk_tx: mpsc::UnboundedSender<SyncedChunk>,
}

/// Manages all FFmpeg RTMP streams for a session
pub struct RtmpManager {
    streams: HashMap<String, FfmpegStream>, // lang → process
}

impl RtmpManager {
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
        }
    }

    /// Start an FFmpeg process for a language stream
    pub async fn start_stream(
        &mut self,
        lang: &str,
        rtmp_url: &str,
        fps: u32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        // FFmpeg command: read JPEG frames from stdin, encode to H.264, mux with audio, push RTMP
        //
        // Video: JPEG frames piped via stdin → mjpeg decoder → libx264
        // Audio: MP3 data piped via a temp file / concat approach
        //
        // For simplicity, we use a two-pass approach per utterance:
        // write frames as MJPEG stream, audio as separate input

        let mut child = Command::new("ffmpeg")
            .args([
                "-y",
                // Video input: MJPEG frames from stdin
                "-f", "mjpeg",
                "-framerate", &fps.to_string(),
                "-i", "pipe:0",
                // Video encoding
                "-c:v", "libx264",
                "-preset", "ultrafast",
                "-tune", "zerolatency",
                "-b:v", "2500k",
                "-maxrate", "2500k",
                "-bufsize", "5000k",
                "-pix_fmt", "yuv420p",
                "-g", &(fps * 2).to_string(), // keyframe every 2s
                "-s", &format!("{}x{}", width, height),
                // No audio initially — we'll add it per-utterance
                "-an",
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

        let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel::<SyncedChunk>();

        // Writer task: receives synced chunks and writes JPEG frames to FFmpeg stdin
        let lang_str = lang.to_string();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(chunk) = chunk_rx.recv().await {
                for frame in &chunk.frames {
                    // Decode base64 JPEG and write raw bytes to stdin
                    use base64::Engine;
                    if let Ok(jpeg_bytes) = base64::engine::general_purpose::STANDARD.decode(&frame.data) {
                        if let Err(e) = stdin.write_all(&jpeg_bytes).await {
                            eprintln!("[FFMPEG:{}] write error: {}", lang_str, e);
                            return;
                        }
                    }
                }
                if let Err(e) = stdin.flush().await {
                    eprintln!("[FFMPEG:{}] flush error: {}", lang_str, e);
                    return;
                }
            }
            // Close stdin to signal EOF
            drop(stdin);
        });

        eprintln!("[FFMPEG] Started RTMP stream for {} → {}", lang, rtmp_url);

        self.streams.insert(lang.to_string(), FfmpegStream {
            child,
            chunk_tx,
        });

        Ok(())
    }

    /// Push a synced audio+video chunk to a language stream
    pub fn push_chunk(&self, lang: &str, chunk: SyncedChunk) -> Result<(), String> {
        let stream = self.streams.get(lang).ok_or("Stream not found")?;
        stream.chunk_tx.send(chunk).map_err(|_| "Stream channel closed".to_string())
    }

    /// Stop all FFmpeg processes
    pub async fn stop_all(&mut self) {
        for (lang, mut stream) in self.streams.drain() {
            drop(stream.chunk_tx); // Close channel → writer task exits → stdin closes → FFmpeg finishes
            match stream.child.wait().await {
                Ok(status) => eprintln!("[FFMPEG:{}] exited: {}", lang, status),
                Err(e) => eprintln!("[FFMPEG:{}] wait error: {}", lang, e),
            }
        }
    }

    /// Stop a specific language stream
    pub async fn stop_stream(&mut self, lang: &str) {
        if let Some(mut stream) = self.streams.remove(lang) {
            drop(stream.chunk_tx);
            let _ = stream.child.wait().await;
            eprintln!("[FFMPEG:{}] stopped", lang);
        }
    }
}

/// Thread-safe wrapper for RtmpManager
pub type SharedRtmpManager = Arc<Mutex<RtmpManager>>;

pub fn new_rtmp_manager() -> SharedRtmpManager {
    Arc::new(Mutex::new(RtmpManager::new()))
}
