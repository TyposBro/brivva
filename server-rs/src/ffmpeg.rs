//! FFmpeg RTMP muxer: host video + translated audio → RTMP streams.
//!
//! Implements the "Fixed-Delay Jitter Buffer" pattern for video-audio sync:
//!
//! 1. Video frames are buffered with capture timestamps in a shared ring buffer
//! 2. A dedicated OS thread drains video at exactly 30fps (33.33ms ticks)
//! 3. A separate dedicated OS thread drains audio at 20ms ticks
//! 4. Both threads read from a shared delayed clock: `Instant::now() - D`
//! 5. TTS audio is queued and released when the delayed clock reaches utterance_start
//! 6. Between utterances, exact silence padding maintains cumulative sample count
//! 7. TTS calls have a hard timeout at D-500ms; missed utterances become silence
//!
//! Using OS threads (not Tokio tasks) ensures timing precision isn't affected
//! by async runtime contention from STT/translation/TTS futures.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command as TokioCommand;

// ── FFmpeg Binary Resolution ──────────────────────────────
//
// Looks for bundled FFmpeg (Tauri sidecar) next to the executable first,
// then falls back to system PATH.

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
const SIDECAR_NAME: &str = "ffmpeg-aarch64-apple-darwin";
#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
const SIDECAR_NAME: &str = "ffmpeg-x86_64-apple-darwin";
#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
const SIDECAR_NAME: &str = "ffmpeg-x86_64-unknown-linux-gnu";
#[cfg(not(any(
    all(target_arch = "aarch64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"),
)))]
const SIDECAR_NAME: &str = "ffmpeg";

static FFMPEG_BIN: LazyLock<String> = LazyLock::new(|| {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Tauri sidecar convention: ffmpeg-{target_triple}
            let sidecar = dir.join(SIDECAR_NAME);
            if sidecar.exists() {
                eprintln!("[FFMPEG] Using bundled: {}", sidecar.display());
                return sidecar.to_string_lossy().to_string();
            }
            // Plain name (manual placement)
            let plain = dir.join("ffmpeg");
            if plain.exists() {
                eprintln!("[FFMPEG] Using bundled: {}", plain.display());
                return plain.to_string_lossy().to_string();
            }
        }
    }
    eprintln!("[FFMPEG] Using system ffmpeg from PATH");
    "ffmpeg".to_string()
});

/// Shared growing PCM buffer for streaming TTS audio.
/// TTS writes chunks as they arrive; the audio drain reads from the same buffer.
#[derive(Clone)]
pub struct StreamingPcm {
    pub pcm: Arc<StdMutex<Vec<u8>>>,
    pub complete: Arc<AtomicBool>,
}

impl StreamingPcm {
    pub fn new() -> Self {
        Self {
            pcm: Arc::new(StdMutex::new(Vec::new())),
            complete: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Append a PCM chunk (called from TTS as each WebSocket chunk arrives)
    pub fn append(&self, data: &[u8]) {
        let mut buf = self.pcm.lock().unwrap();
        buf.extend_from_slice(data);
    }

    /// Mark the stream as complete (all TTS chunks received or error)
    pub fn finish(&self) {
        self.complete.store(true, Ordering::Release);
    }

    /// Append data and apply truncation + fadeout if over max_bytes
    pub fn append_with_limit(&self, data: &[u8], max_bytes: usize) {
        let mut buf = self.pcm.lock().unwrap();
        let remaining_capacity = max_bytes.saturating_sub(buf.len());
        if remaining_capacity == 0 {
            return;
        }
        let to_add = data.len().min(remaining_capacity);
        buf.extend_from_slice(&data[..to_add]);
        if buf.len() >= max_bytes {
            truncate_with_fadeout(&mut buf, max_bytes);
            drop(buf);
            self.complete.store(true, Ordering::Release);
        }
    }
}

/// Audio waiting to be played at the right point in the delayed timeline
pub(crate) struct QueuedAudio {
    /// Source timestamp when this utterance started (host speaking)
    play_at: Instant,
    /// Shared PCM buffer (may still be growing if TTS is streaming)
    pcm: Arc<StdMutex<Vec<u8>>>,
    /// True when all audio data has been written
    complete: Arc<AtomicBool>,
}

/// State for draining queued audio chunk-by-chunk
struct ActiveAudio {
    pcm: Arc<StdMutex<Vec<u8>>>,
    complete: Arc<AtomicBool>,
    offset: usize,
}

/// Handle for one FFmpeg RTMP stream (per language/platform)
struct RtmpStream {
    child: std::process::Child,
    video_handle: Option<thread::JoinHandle<()>>,
    audio_handle: Option<thread::JoinHandle<()>>,
    audio_fifo: String,
    lang: String,
    rtmp_url: String,
    /// Per-stream audio queue
    audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    /// Signal to stop drain threads
    stop_flag: Arc<AtomicBool>,
    /// How many times we've restarted this stream after a crash
    restart_count: u32,
    /// Set to true by stderr reader when RTMP errors are detected
    rtmp_error: Arc<AtomicBool>,
}

/// Manages all FFmpeg RTMP streams for a session.
///
/// Receives pre-encoded video chunks from MediaRecorder (H.264/VP8) and
/// queued PCM audio from the translation pipeline. Video chunks are delayed
/// by D seconds to allow TTS to complete before the corresponding video plays.
pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
    /// Delayed queue of encoded video chunks (timestamp, data)
    video_chunks: Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    /// Fixed broadcast delay applied to all streams
    broadcast_delay: Duration,
    /// Video codec from MediaRecorder ("h264" = passthrough, "vp8"/"vp9" = re-encode)
    video_codec: String,
}
/// Audio: 20ms per tick
const AUDIO_TICK: Duration = Duration::from_millis(20);
/// Audio bytes per 20ms tick: 44100Hz × 2 bytes/sample × 1 channel × 0.02s = 1764 bytes
const AUDIO_BYTES_PER_TICK: usize = 1764;
/// Max video chunks to buffer (~60s at 10 chunks/sec)
const MAX_VIDEO_CHUNKS: usize = 600;
/// Default broadcast delay (5s gives chunked utterances enough pipeline budget)
const DEFAULT_DELAY_MS: u64 = 3000;
/// Max FFmpeg restart attempts per stream
const MAX_FFMPEG_RESTARTS: u32 = 50;
/// Delay between FFmpeg restart attempts
const FFMPEG_RESTART_DELAY: Duration = Duration::from_secs(2);
/// Jitter warning threshold (100ms avoids log spam)
const JITTER_WARN_THRESHOLD: Duration = Duration::from_millis(100);
/// Jitter recovery threshold — if we fall this far behind, reset the tick anchor
/// rather than trying to catch up (which causes a cascade of late writes).
const JITTER_RECOVERY_THRESHOLD: Duration = Duration::from_millis(500);
/// Fade-out duration in bytes: 50ms at 44100Hz mono 16-bit = 4410 bytes
const FADE_OUT_BYTES: usize = 4410;
/// Audio bytes per second: 44100Hz × 2 bytes/sample = 88200
const BYTES_PER_SEC: f64 = 88200.0;

impl RtmpManager {
    pub fn new() -> Self {
        Self::with_delay(DEFAULT_DELAY_MS)
    }

    pub fn with_delay(delay_ms: u64) -> Self {
        eprintln!("[SYNC] Broadcast delay: {}ms", delay_ms);
        Self {
            streams: HashMap::new(),
            video_chunks: Arc::new(StdMutex::new(VecDeque::new())),
            broadcast_delay: Duration::from_millis(delay_ms),
            video_codec: "vp8".to_string(),
        }
    }

    /// Returns the broadcast delay for TTS timeout calculations
    pub fn broadcast_delay(&self) -> Duration {
        self.broadcast_delay
    }

    /// Set video codec for FFmpeg passthrough decision.
    /// "h264" = use `-c:v copy` (zero CPU), anything else = re-encode.
    pub fn set_video_codec(&mut self, codec: &str) {
        self.video_codec = codec.to_string();
        eprintln!("[FFMPEG] Video codec set to: {} ({})",
            codec, if codec == "h264" { "passthrough" } else { "re-encode" });
    }

    /// Start an FFmpeg RTMP process with dedicated video and audio drain threads
    pub fn start_stream(
        &mut self,
        stream_id: &str,
        lang: &str,
        rtmp_url: &str,
    ) -> Result<(), String> {
        self.spawn_stream_inner(stream_id, lang, rtmp_url, None)?;
        eprintln!(
            "[FFMPEG] Started RTMP stream {} ({}) → {} [delay={}ms, video+audio on dedicated OS threads]",
            stream_id, lang, rtmp_url, self.broadcast_delay.as_millis()
        );
        Ok(())
    }

    /// Buffer an encoded video chunk from MediaRecorder.
    /// Chunks are timestamped and released after the broadcast delay.
    pub fn push_video_chunk(&self, data: &[u8]) {
        let mut buf = self.video_chunks.lock().unwrap();
        buf.push_back((Instant::now(), data.to_vec()));
        let buf_len = buf.len();
        let mut dropped = 0;
        // Cap buffer at ~60s of chunks (assuming ~10 chunks/sec at 100ms intervals)
        while buf.len() > 600 {
            buf.pop_front();
            dropped += 1;
        }
        if dropped > 0 {
            eprintln!("[VIDEO] buffer overflow: dropped {} old chunks (buf={})", dropped, buf_len);
        }
        if buf_len % 50 == 0 {
            eprintln!("[VIDEO] buffered chunk: {}B (buf_depth={})", data.len(), buf_len);
        }
    }

    /// Queue complete audio for synced playback (used for passthrough).
    pub fn queue_audio(&self, lang: &str, pcm: Vec<u8>, utterance_start: Instant) {
        let pcm_len = pcm.len();
        let pcm_arc = Arc::new(StdMutex::new(pcm));
        let complete = Arc::new(AtomicBool::new(true));
        for stream in self.streams.values() {
            if stream.lang == lang {
                let mut q = stream.audio_queue.lock().unwrap();
                q.push_back(QueuedAudio {
                    play_at: utterance_start,
                    pcm: pcm_arc,
                    complete,
                });
                eprintln!(
                    "[AUDIO:{}] queued passthrough audio: {}KB ({:.1}s) queue_depth={}",
                    lang, pcm_len / 1024, pcm_len as f64 / 88200.0, q.len()
                );
                return;
            }
        }
        eprintln!("[AUDIO] no stream found for lang={}, audio dropped", lang);
    }

    /// Queue a streaming audio slot. Returns StreamingPcm that the TTS task
    /// writes chunks into. Audio drain starts playing as soon as data arrives
    /// and the delayed clock reaches play_at.
    pub fn queue_streaming_audio(&self, lang: &str, utterance_start: Instant) -> StreamingPcm {
        let streaming = StreamingPcm::new();
        for stream in self.streams.values() {
            if stream.lang == lang {
                let mut q = stream.audio_queue.lock().unwrap();
                q.push_back(QueuedAudio {
                    play_at: utterance_start,
                    pcm: streaming.pcm.clone(),
                    complete: streaming.complete.clone(),
                });
                eprintln!(
                    "[AUDIO:{}] queued streaming TTS slot, queue_depth={}",
                    lang, q.len()
                );
                return streaming;
            }
        }
        eprintln!("[AUDIO] no stream found for lang={}, streaming slot orphaned", lang);
        streaming
    }

    /// Check all FFmpeg processes for crashes. Returns a list of streams that need
    /// restarting (id, lang, rtmp_url, prev_restart_count, audio_queue).
    /// The caller is responsible for waiting between retries (to avoid blocking the runtime).
    pub(crate) fn detect_crashed(&mut self) -> Vec<(String, String, String, u32, Arc<StdMutex<VecDeque<QueuedAudio>>>)> {
        let mut to_restart = Vec::new();

        for (id, stream) in &mut self.streams {
            match stream.child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(-1);
                    if stream.stop_flag.load(Ordering::Acquire) {
                        continue;
                    }
                    eprintln!(
                        "[FFMPEG] Process crashed for lang={}, exit={}, restarting...",
                        stream.lang, code
                    );
                    if stream.restart_count >= MAX_FFMPEG_RESTARTS {
                        eprintln!(
                            "[FFMPEG] Failed to restart after {} attempts for lang={}",
                            MAX_FFMPEG_RESTARTS, stream.lang
                        );
                        stream.stop_flag.store(true, Ordering::Release);
                        continue;
                    }
                    to_restart.push((
                        id.clone(),
                        stream.lang.clone(),
                        stream.rtmp_url.clone(),
                    ));
                }
                Ok(None) => {
                    // Process alive — check for RTMP errors flagged by stderr reader
                    if stream.rtmp_error.load(Ordering::Acquire) {
                        if stream.stop_flag.load(Ordering::Acquire) {
                            continue;
                        }
                        eprintln!(
                            "[FFMPEG] RTMP connection error for lang={}, killing for restart",
                            stream.lang
                        );
                        let _ = stream.child.kill();
                        let _ = stream.child.wait();
                        if stream.restart_count >= MAX_FFMPEG_RESTARTS {
                            eprintln!(
                                "[FFMPEG] Failed to restart after {} attempts for lang={}",
                                MAX_FFMPEG_RESTARTS, stream.lang
                            );
                            stream.stop_flag.store(true, Ordering::Release);
                            continue;
                        }
                        to_restart.push((
                            id.clone(),
                            stream.lang.clone(),
                            stream.rtmp_url.clone(),
                        ));
                    }
                }
                Err(e) => {
                    eprintln!("[FFMPEG] Error checking process status for {}: {}", id, e);
                }
            }
        }

        // Collect cleanup info and remove old entries
        let mut result = Vec::new();
        for (id, lang, rtmp_url) in to_restart {
            if let Some(mut old) = self.streams.remove(&id) {
                old.stop_flag.store(true, Ordering::Release);
                let _ = old.child.kill();
                let _ = old.child.wait();
                let _ = std::fs::remove_file(&old.audio_fifo);
                let prev_count = old.restart_count;
                let audio_queue = old.audio_queue.clone();
                result.push((id, lang, rtmp_url, prev_count, audio_queue));
            }
        }

        result
    }

    /// Restart a single stream after a crash. Called after an async delay.
    pub(crate) fn restart_stream(
        &mut self,
        id: &str,
        lang: &str,
        rtmp_url: &str,
        prev_count: u32,
        audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    ) {
        match self.spawn_stream_inner(id, lang, rtmp_url, Some(audio_queue)) {
            Ok(()) => {
                if let Some(stream) = self.streams.get_mut(id) {
                    stream.restart_count = prev_count + 1;
                }
                eprintln!(
                    "[FFMPEG] Restarted stream {} ({}) attempt {}/{}",
                    id, lang, prev_count + 1, MAX_FFMPEG_RESTARTS
                );
            }
            Err(e) => {
                eprintln!(
                    "[FFMPEG] Restart failed for {} ({}): {}",
                    id, lang, e
                );
            }
        }
    }

    /// Internal helper to spawn an FFmpeg process and its drain threads.
    /// If `existing_queue` is provided (restart case), reuses the audio queue
    /// so pending audio isn't lost.
    fn spawn_stream_inner(
        &mut self,
        stream_id: &str,
        lang: &str,
        rtmp_url: &str,
        existing_queue: Option<Arc<StdMutex<VecDeque<QueuedAudio>>>>,
    ) -> Result<(), String> {
        let audio_fifo = format!("/tmp/brivva_audio_{}", stream_id);

        // Create named FIFO
        let _ = std::fs::remove_file(&audio_fifo);
        eprintln!("[FFMPEG:{}] creating FIFO: {}", stream_id, audio_fifo);
        std::process::Command::new("mkfifo")
            .arg(&audio_fifo)
            .output()
            .map_err(|e| format!("mkfifo failed: {}", e))?;

        // Spawn FFmpeg — accepts encoded video (webm/mp4) from stdin,
        // PCM audio from FIFO. H.264 input = passthrough (zero CPU), VP8/VP9 = re-encode.
        let mut args = vec![
            "-y".to_string(),
            "-loglevel".to_string(), "warning".to_string(),
            // Video input: encoded stream from MediaRecorder
            "-i".to_string(), "pipe:0".to_string(),
            // Audio input: raw PCM from FIFO
            "-f".to_string(), "s16le".to_string(),
            "-ar".to_string(), "44100".to_string(),
            "-ac".to_string(), "1".to_string(),
            "-i".to_string(), audio_fifo.clone(),
        ];

        // Always re-encode to H.264 for FLV container.
        // -c:v copy doesn't work with chunked WebM from MediaRecorder stdin.
        // ultrafast preset keeps CPU usage low since MediaRecorder already compressed.
        {
            eprintln!("[FFMPEG] Encoding {} → H.264 (ultrafast)", self.video_codec);
            args.extend([
                "-c:v".to_string(), "libx264".to_string(),
                "-preset".to_string(), "ultrafast".to_string(),
                "-tune".to_string(), "zerolatency".to_string(),
                "-crf".to_string(), "23".to_string(),
                "-maxrate".to_string(), "8000k".to_string(),
                "-bufsize".to_string(), "16000k".to_string(),
                "-pix_fmt".to_string(), "yuv420p".to_string(),
                "-g".to_string(), "60".to_string(),
            ]);
        }

        args.extend([
            "-c:a".to_string(), "aac".to_string(),
            "-ac:a".to_string(), "2".to_string(),
            "-b:a".to_string(), "128k".to_string(),
            "-map".to_string(), "0:v".to_string(),
            "-map".to_string(), "1:a".to_string(),
            "-f".to_string(), "flv".to_string(),
            // RTMP reconnect: retry on network drops instead of dying
            "-flvflags".to_string(), "no_duration_filesize".to_string(),
            "-rtmp_live".to_string(), "live".to_string(),
            rtmp_url.to_string(),
        ]);

        eprintln!(
            "[FFMPEG:{}] spawning: {} {}",
            stream_id, &*FFMPEG_BIN, args.join(" ")
        );
        let mut child = std::process::Command::new(&*FFMPEG_BIN)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;
        eprintln!("[FFMPEG:{}] spawned PID={}", stream_id, child.id());

        let stdin = child.stdin.take().ok_or("No FFmpeg stdin")?;

        // Track RTMP errors detected by stderr reader
        let rtmp_error = Arc::new(AtomicBool::new(false));

        // Drain FFmpeg stderr in background to prevent pipe buffer from filling up
        // (which would block FFmpeg and stop audio/video processing)
        if let Some(stderr) = child.stderr.take() {
            let sid = stream_id.to_string();
            let err_flag = rtmp_error.clone();
            thread::Builder::new()
                .name(format!("ffmpeg-stderr-{}", stream_id))
                .spawn(move || {
                    use std::io::{BufRead, BufReader};
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        match line {
                            Ok(l) if !l.is_empty() => {
                                eprintln!("[FFMPEG:{}] {}", sid, l);
                                // Detect RTMP connection failures
                                let lower = l.to_lowercase();
                                if lower.contains("connection refused")
                                    || lower.contains("connection reset")
                                    || lower.contains("broken pipe")
                                    || lower.contains("connection timed out")
                                    || lower.contains("i/o error")
                                    || lower.contains("error writing trailer")
                                {
                                    eprintln!("[FFMPEG:{}] RTMP error detected, flagging for restart", sid);
                                    err_flag.store(true, Ordering::Release);
                                }
                            }
                            Err(_) => break,
                            _ => {}
                        }
                    }
                })
                .ok();
        }

        let audio_queue = existing_queue
            .unwrap_or_else(|| Arc::new(StdMutex::new(VecDeque::new())));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let delay = self.broadcast_delay;

        // Spawn video drain thread — forwards delayed encoded chunks to FFmpeg stdin
        let video_chunk_buf = self.video_chunks.clone();
        let video_stop = stop_flag.clone();
        let video_sid = stream_id.to_string();
        let video_handle = thread::Builder::new()
            .name(format!("video-drain-{}", stream_id))
            .spawn(move || {
                video_chunk_drain_loop(video_sid, video_chunk_buf, stdin, delay, video_stop);
            })
            .map_err(|e| format!("Video thread spawn failed: {}", e))?;

        // Spawn audio drain thread
        let audio_aq = audio_queue.clone();
        let audio_stop = stop_flag.clone();
        let audio_sid = stream_id.to_string();
        let audio_fifo_path = audio_fifo.clone();
        let audio_handle = thread::Builder::new()
            .name(format!("audio-drain-{}", stream_id))
            .spawn(move || {
                audio_drain_loop(audio_sid, audio_aq, audio_fifo_path, delay, audio_stop);
            })
            .map_err(|e| format!("Audio thread spawn failed: {}", e))?;

        self.streams.insert(
            stream_id.to_string(),
            RtmpStream {
                child,
                video_handle: Some(video_handle),
                audio_handle: Some(audio_handle),
                audio_fifo,
                lang: lang.to_string(),
                rtmp_url: rtmp_url.to_string(),
                audio_queue,
                stop_flag,
                restart_count: 0,
                rtmp_error,
            },
        );

        Ok(())
    }

    /// Stop all FFmpeg processes and clean up.
    /// Drain threads are joined with a 3-second timeout to prevent cleanup from hanging.
    pub async fn stop_all(&mut self) {
        for (id, mut stream) in self.streams.drain() {
            // Signal threads to stop
            stream.stop_flag.store(true, Ordering::Release);
            // Kill FFmpeg process (causes stdin/FIFO writes to fail, unblocking threads)
            match stream.child.kill() {
                Ok(_) => {
                    let _ = stream.child.wait();
                    eprintln!("[FFMPEG:{}] killed", id);
                }
                Err(e) => eprintln!("[FFMPEG:{}] kill error: {}", id, e),
            }
            // Join drain threads with timeout — if a thread is stuck (e.g., blocked
            // on FIFO write after FFmpeg died in a weird state), don't hang cleanup.
            let join_timeout = Duration::from_secs(3);
            for (label, handle) in [
                ("video", stream.video_handle.take()),
                ("audio", stream.audio_handle.take()),
            ] {
                if let Some(h) = handle {
                    let id_clone = id.clone();
                    let result = tokio::time::timeout(join_timeout, async {
                        tokio::task::spawn_blocking(move || h.join()).await
                    })
                    .await;
                    match result {
                        Ok(Ok(Ok(()))) => {}
                        Ok(Ok(Err(_))) => eprintln!("[FFMPEG:{}] {} thread panicked", id_clone, label),
                        Ok(Err(_)) => eprintln!("[FFMPEG:{}] {} thread join cancelled", id_clone, label),
                        Err(_) => eprintln!("[FFMPEG:{}] {} thread join timed out (3s), abandoning", id_clone, label),
                    }
                }
            }
            let _ = std::fs::remove_file(&stream.audio_fifo);
        }
    }

    /// Restart all RTMP streams (stop + re-spawn with same config).
    /// Used when YouTube drops the RTMP connection and needs a fresh start.
    pub async fn restart_all(&mut self) {
        let configs: Vec<(String, String, String, Arc<StdMutex<VecDeque<QueuedAudio>>>)> =
            self.streams.iter().map(|(id, s)| {
                (id.clone(), s.lang.clone(), s.rtmp_url.clone(), s.audio_queue.clone())
            }).collect();

        if configs.is_empty() {
            eprintln!("[RTMP] restart_all: no streams to restart");
            return;
        }

        eprintln!("[RTMP] restarting {} stream(s)", configs.len());
        self.stop_all().await;

        for (id, lang, url, aq) in configs {
            self.restart_stream(&id, &lang, &url, 0, aq);
        }
        eprintln!("[RTMP] restart complete");
    }
}

/// Thread-safe wrapper
pub type SharedRtmpManager = Arc<tokio::sync::Mutex<RtmpManager>>;

/// Spawn a background task that periodically checks for crashed FFmpeg processes
/// and restarts them. Runs every 2 seconds. Stops when stop_flag is set.
pub fn spawn_health_monitor(
    manager: SharedRtmpManager,
    stop_flag: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        eprintln!("[HEALTH] FFmpeg health monitor started (check every 2s)");
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        let mut check_count: u64 = 0;
        loop {
            interval.tick().await;
            if stop_flag.load(Ordering::Acquire) {
                eprintln!("[HEALTH] stop signal received, exiting");
                break;
            }
            check_count += 1;
            // Detect crashes (quick, non-blocking check)
            let crashed = {
                let mut mgr = manager.lock().await;
                mgr.detect_crashed()
            };
            if !crashed.is_empty() {
                eprintln!("[HEALTH] check #{}: {} crashed stream(s) detected", check_count, crashed.len());
            }
            // Restart each crashed stream with async delay between attempts
            for (id, lang, rtmp_url, prev_count, audio_queue) in crashed {
                tokio::time::sleep(FFMPEG_RESTART_DELAY).await;
                if stop_flag.load(Ordering::Acquire) {
                    break;
                }
                let mut mgr = manager.lock().await;
                mgr.restart_stream(&id, &lang, &rtmp_url, prev_count, audio_queue);
            }
        }
    })
}

// ── Video Chunk Drain Thread ──────────────────────────────

/// Dedicated OS thread: forwards encoded video chunks to FFmpeg stdin
/// after the broadcast delay has elapsed.
///
/// Polls every 20ms. Chunks older than D seconds are written to FFmpeg.
/// Much lighter than the old per-frame JPEG approach since the browser's
/// hardware encoder already compressed the video.
fn video_chunk_drain_loop(
    stream_id: String,
    chunk_buffer: Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    mut stdin: std::process::ChildStdin,
    delay: Duration,
    stop: Arc<AtomicBool>,
) {
    let poll_interval = Duration::from_millis(20);
    let mut chunks_written: u64 = 0;
    let mut total_bytes_written: u64 = 0;
    let drain_start = Instant::now();

    eprintln!(
        "[VIDEO:{}] chunk drain thread started ({}ms delay)",
        stream_id, delay.as_millis()
    );

    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }

        thread::sleep(poll_interval);

        let deadline = Instant::now() - delay;

        // Drain all chunks that are old enough
        loop {
            let chunk = {
                let mut buf = chunk_buffer.lock().unwrap();
                match buf.front() {
                    Some((ts, _)) if *ts <= deadline => buf.pop_front(),
                    _ => None,
                }
            };

            match chunk {
                Some((_, data)) => {
                    let data_len = data.len();
                    if stdin.write_all(&data).is_err() {
                        if !stop.load(Ordering::Acquire) {
                            eprintln!("[VIDEO:{}] write error, exiting", stream_id);
                        }
                        drop(stdin);
                        return;
                    }
                    chunks_written += 1;
                    total_bytes_written += data_len as u64;

                    // Periodic stats every 100 chunks (~10s at 10 chunks/sec)
                    if chunks_written % 100 == 0 {
                        eprintln!(
                            "[VIDEO:{}] stats: {} chunks, {}KB written, {:.0}s elapsed",
                            stream_id, chunks_written, total_bytes_written / 1024,
                            drain_start.elapsed().as_secs_f64()
                        );
                    }
                }
                None => break,
            }
        }
    }

    drop(stdin);
    eprintln!(
        "[VIDEO:{}] chunk drain thread exited after {} chunks ({}KB, {:.0}s)",
        stream_id, chunks_written, total_bytes_written / 1024, drain_start.elapsed().as_secs_f64()
    );
}

// ── Audio Drain Thread ─────────────────────────────────────

/// Dedicated OS thread: drains audio at 20ms ticks.
///
/// Independent from the video thread — shares only the delayed clock reference.
/// Tracks cumulative samples written to detect drift over long sessions.
fn audio_drain_loop(
    stream_id: String,
    audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    fifo_path: String,
    delay: Duration,
    stop: Arc<AtomicBool>,
) {
    // Open FIFO for writing (blocks until FFmpeg opens it for reading)
    eprintln!("[AUDIO:{}] opening FIFO (blocks until FFmpeg reads)...", stream_id);
    let mut fifo = match std::fs::OpenOptions::new()
        .write(true)
        .open(&fifo_path)
    {
        Ok(f) => f,
        Err(e) => {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[AUDIO:{}] failed to open FIFO: {}", stream_id, e);
            }
            return;
        }
    };
    eprintln!("[AUDIO:{}] FIFO opened", stream_id);

    let silence = vec![0u8; AUDIO_BYTES_PER_TICK];
    let mut active_audio: Option<ActiveAudio> = None;
    let mut tick_count: u64 = 0;
    // Reset tick anchor AFTER FIFO opens (FIFO open blocks on FFmpeg startup)
    let mut next_tick = Instant::now() + AUDIO_TICK;

    // Cumulative sample tracking for drift detection
    let start_time = Instant::now();
    let mut total_bytes_written: u64 = 0;
    let mut jitter_warn_count: u64 = 0;

    eprintln!(
        "[AUDIO:{}] drain thread started (20ms ticks, {}ms delay)",
        stream_id,
        delay.as_millis()
    );

    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }

        // Sleep until next tick
        let now = Instant::now();
        if next_tick > now {
            thread::sleep(next_tick - now);
        }

        // Check jitter
        let actual = Instant::now();
        let jitter = actual.saturating_duration_since(next_tick);

        if jitter > JITTER_RECOVERY_THRESHOLD && tick_count > 0 {
            // Severe jitter: reset the tick anchor instead of trying to catch up.
            // Catching up dumps a burst of audio into the FIFO that desynchronizes everything.
            let skipped_ticks = jitter.as_millis() / AUDIO_TICK.as_millis();
            eprintln!(
                "[AUDIO:{}] JITTER RECOVERY: {}ms behind at tick {}, skipping ~{} ticks, resetting anchor",
                stream_id, jitter.as_millis(), tick_count, skipped_ticks
            );
            next_tick = actual + AUDIO_TICK;
            tick_count += skipped_ticks as u64;
            total_bytes_written += skipped_ticks as u64 * AUDIO_BYTES_PER_TICK as u64;
            // Skip this tick (don't write anything — the silence was already "written" by the anchor reset)
            continue;
        } else if jitter > JITTER_WARN_THRESHOLD && tick_count > 0 {
            jitter_warn_count += 1;
            // Rate-limit: log every 25th warning, or the first one
            if jitter_warn_count == 1 || jitter_warn_count % 25 == 0 {
                eprintln!(
                    "[AUDIO:{}] jitter: tick {} was {}ms late (warning #{}, threshold={}ms)",
                    stream_id, tick_count, jitter.as_millis(), jitter_warn_count,
                    JITTER_WARN_THRESHOLD.as_millis()
                );
            }
        }

        // Anchor next tick to prevent drift accumulation
        next_tick += AUDIO_TICK;
        tick_count += 1;

        let target_ts = actual - delay;

        // Check if a new queued audio should start playing
        if active_audio.is_none() {
            let mut q = audio_queue.lock().unwrap();
            if let Some(front) = q.front() {
                if target_ts >= front.play_at {
                    let audio = q.pop_front().unwrap();
                    let pcm_len = audio.pcm.lock().unwrap().len();
                    let is_complete = audio.complete.load(Ordering::Acquire);
                    let remaining = q.len();
                    eprintln!(
                        "[AUDIO:{}] starting utterance: {}B available, complete={}, queue_depth={}",
                        stream_id, pcm_len, is_complete, remaining
                    );
                    active_audio = Some(ActiveAudio {
                        pcm: audio.pcm,
                        complete: audio.complete,
                        offset: 0,
                    });
                }
            }
        }

        // Write one tick's worth of audio (1764 bytes = 20ms at 44100Hz mono 16-bit)
        // Supports streaming: reads from a growing buffer, writes silence if TTS
        // hasn't produced enough data yet.
        let (data_to_write, should_clear) = if let Some(ref mut active) = active_audio {
            let guard = active.pcm.lock().unwrap();
            let available = guard.len() - active.offset;
            let is_complete = active.complete.load(Ordering::Acquire);

            if available >= AUDIO_BYTES_PER_TICK {
                let start = active.offset;
                let end = start + AUDIO_BYTES_PER_TICK;
                let data = guard[start..end].to_vec();
                active.offset = end;
                let done = active.offset >= guard.len() && is_complete;
                (data, done)
            } else if is_complete {
                if available > 0 {
                    // Last partial chunk — pad with silence
                    let mut chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
                    chunk[..available].copy_from_slice(&guard[active.offset..]);
                    (chunk, true)
                } else {
                    (silence.clone(), true)
                }
            } else {
                // TTS still streaming, not enough data yet — write silence this tick
                (silence.clone(), false)
            }
        } else {
            (silence.clone(), false)
        };

        if should_clear {
            if let Some(ref a) = active_audio {
                let total = a.pcm.lock().unwrap().len();
                let played_ms = (a.offset as f64 / BYTES_PER_SEC * 1000.0) as u64;
                eprintln!(
                    "[AUDIO:{}] utterance done: played {}B/{}B ({}ms audio)",
                    stream_id, a.offset, total, played_ms
                );
            }
            active_audio = None;
        }

        let write_result = fifo.write_all(&data_to_write);

        total_bytes_written += AUDIO_BYTES_PER_TICK as u64;

        if write_result.is_err() {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[AUDIO:{}] write error, exiting", stream_id);
            }
            break;
        }

        // Periodic drift check (every ~5 seconds = 250 ticks at 20ms)
        if tick_count % 250 == 0 {
            let elapsed = start_time.elapsed().as_secs_f64();
            let expected_bytes = (elapsed * BYTES_PER_SEC) as u64;
            let drift_bytes =
                (total_bytes_written as i64 - expected_bytes as i64).unsigned_abs();
            let drift_ms = (drift_bytes as f64 / BYTES_PER_SEC * 1000.0) as u64;
            if drift_ms > 50 {
                eprintln!(
                    "[AUDIO:{}] drift warning: {}ms (written={}, expected={})",
                    stream_id, drift_ms, total_bytes_written, expected_bytes
                );
            }
        }
    }

    drop(fifo);
    eprintln!(
        "[AUDIO:{}] drain thread exited after {} ticks",
        stream_id, tick_count
    );
}

// ── Shared Helpers ─────────────────────────────────────────

/// Truncate PCM audio to max_bytes and apply a 50ms fade-out at the cut point.
/// Operates on s16le (16-bit signed little-endian, mono) samples.
pub fn truncate_with_fadeout(pcm: &mut Vec<u8>, max_bytes: usize) {
    if pcm.len() <= max_bytes {
        return;
    }
    pcm.truncate(max_bytes);

    // Apply linear fade-out to the last FADE_OUT_BYTES
    let fade_bytes = FADE_OUT_BYTES.min(pcm.len());
    let fade_start = pcm.len() - fade_bytes;
    let fade_samples = fade_bytes / 2; // 16-bit = 2 bytes per sample

    for i in 0..fade_samples {
        let byte_offset = fade_start + i * 2;
        if byte_offset + 1 >= pcm.len() {
            break;
        }
        let sample = i16::from_le_bytes([pcm[byte_offset], pcm[byte_offset + 1]]);
        // Linear fade: 1.0 at start of fade region → 0.0 at end
        let gain = 1.0 - (i as f32 / fade_samples as f32);
        let faded = (sample as f32 * gain) as i16;
        let bytes = faded.to_le_bytes();
        pcm[byte_offset] = bytes[0];
        pcm[byte_offset + 1] = bytes[1];
    }
}

/// Kill any orphaned FFmpeg processes from a previous server crash.
/// Called once at startup before accepting connections.
/// Matches FFmpeg processes whose cmdline contains "brivva_audio" (our FIFO naming convention).
pub fn kill_orphan_ffmpeg() {
    let output = match std::process::Command::new("pgrep")
        .args(["-f", "brivva_audio"])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[STARTUP] pgrep not available, skipping orphan cleanup: {}", e);
            return;
        }
    };

    let pids = String::from_utf8_lossy(&output.stdout);
    let mut killed = 0;
    for line in pids.lines() {
        if let Ok(pid) = line.trim().parse::<i32>() {
            // Don't kill ourselves
            let my_pid = std::process::id() as i32;
            if pid == my_pid {
                continue;
            }
            eprintln!("[STARTUP] killing orphan FFmpeg process (PID {})", pid);
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
            killed += 1;
        }
    }

    // Clean up stale FIFOs
    if let Ok(entries) = std::fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("brivva_audio_") {
                    let _ = std::fs::remove_file(entry.path());
                    eprintln!("[STARTUP] removed stale FIFO: {}", name);
                }
            }
        }
    }

    if killed > 0 {
        eprintln!("[STARTUP] killed {} orphan FFmpeg process(es)", killed);
    } else {
        eprintln!("[STARTUP] no orphan FFmpeg processes found");
    }
}

/// Decode MP3 bytes to raw PCM s16le 44100Hz mono using FFmpeg subprocess
pub async fn decode_mp3_to_pcm(mp3: &[u8]) -> Result<Vec<u8>, String> {
    eprintln!("[FFMPEG] decode_mp3_to_pcm: {}B MP3 input", mp3.len());
    let decode_start = Instant::now();
    let mut child = TokioCommand::new(&*FFMPEG_BIN)
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
    eprintln!(
        "[FFMPEG] decode_mp3_to_pcm: {}B MP3 -> {}B PCM ({:.1}s audio) in {}ms",
        mp3.len(), output.stdout.len(),
        output.stdout.len() as f64 / 88200.0,
        decode_start.elapsed().as_millis()
    );
    Ok(output.stdout)
}

// ── Incremental MP3 Decoder ─────────────────────────────
//
// Long-lived FFmpeg subprocess for streaming MP3→PCM decode.
// Each `feed()` writes MP3 bytes to stdin and reads available PCM from stdout.
// `finish()` closes stdin and drains remaining PCM.

pub struct IncrementalMp3Decoder {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    total_mp3_in: usize,
    total_pcm_out: usize,
}

impl IncrementalMp3Decoder {
    pub async fn new() -> Result<Self, String> {
        let mut child = TokioCommand::new(&*FFMPEG_BIN)
            .args([
                "-f", "mp3", "-i", "pipe:0",
                "-f", "s16le", "-ar", "44100", "-ac", "1",
                "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("IncrementalMp3Decoder spawn failed: {}", e))?;

        let stdin = child.stdin.take().ok_or("No stdin on decoder")?;
        let stdout = child.stdout.take().ok_or("No stdout on decoder")?;

        Ok(Self { child, stdin, stdout, total_mp3_in: 0, total_pcm_out: 0 })
    }

    /// Feed an MP3 chunk and read any available PCM output.
    pub async fn feed(&mut self, mp3_chunk: &[u8]) -> Result<Vec<u8>, String> {
        self.stdin
            .write_all(mp3_chunk)
            .await
            .map_err(|e| format!("Decoder stdin write failed: {}", e))?;
        self.total_mp3_in += mp3_chunk.len();

        // Read whatever PCM is available (non-blocking with short timeout)
        let pcm = self.read_available().await;
        self.total_pcm_out += pcm.len();
        Ok(pcm)
    }

    /// Close stdin and drain all remaining PCM.
    pub async fn finish(mut self) -> Result<Vec<u8>, String> {
        drop(self.stdin);

        let mut remaining = Vec::new();
        let mut buf = [0u8; 16384];
        loop {
            match tokio::time::timeout(
                Duration::from_millis(200),
                self.stdout.read(&mut buf),
            ).await {
                Ok(Ok(0)) => break,        // EOF
                Ok(Ok(n)) => remaining.extend_from_slice(&buf[..n]),
                Ok(Err(_)) => break,        // read error
                Err(_) => break,            // timeout — no more data
            }
        }

        let _ = self.child.kill().await;
        self.total_pcm_out += remaining.len();

        eprintln!(
            "[FFMPEG] IncrementalMp3Decoder: {}B MP3 -> {}B PCM ({:.1}s audio)",
            self.total_mp3_in, self.total_pcm_out,
            self.total_pcm_out as f64 / 88200.0
        );
        Ok(remaining)
    }

    /// Try to read available PCM without blocking for too long.
    async fn read_available(&mut self) -> Vec<u8> {
        let mut result = Vec::new();
        let mut buf = [0u8; 8192];
        // Read in a tight loop with short timeouts to drain buffered output
        loop {
            match tokio::time::timeout(
                Duration::from_millis(5),
                self.stdout.read(&mut buf),
            ).await {
                Ok(Ok(0)) => break,        // EOF
                Ok(Ok(n)) => result.extend_from_slice(&buf[..n]),
                Ok(Err(_)) => break,
                Err(_) => break,            // timeout — nothing more available right now
            }
        }
        result
    }
}
