use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::constants::BYTES_PER_SEC;
use super::process::FFMPEG_BIN;
use super::types::{
    StreamingPcm, QueuedAudio,
    MAX_VIDEO_CHUNKS, DEFAULT_DELAY_MS, MAX_FFMPEG_RESTARTS, FFMPEG_RESTART_DELAY,
};
use super::{video_drain, audio_drain};

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

// ── RtmpManager ───────────────────────────────────────────

/// Manages all FFmpeg RTMP streams for a session.
///
/// Receives pre-encoded video chunks from MediaRecorder (H.264/VP8) and
/// queued PCM audio from the translation pipeline. Video chunks are delayed
/// by D seconds to allow TTS to complete before the corresponding video plays.
pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
    /// Delayed queue of encoded video chunks (timestamp, data)
    video_chunks: Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    /// First video chunk from MediaRecorder (ftyp+moov init segment).
    /// Replayed on restart so FFmpeg can parse the fMP4 container.
    video_init_segment: Arc<StdMutex<Option<Vec<u8>>>>,
    /// Fixed broadcast delay applied to all streams
    broadcast_delay: Duration,
    /// Video codec from MediaRecorder ("h264" = passthrough, "vp8"/"vp9" = re-encode)
    video_codec: String,
}

impl RtmpManager {
    pub fn new() -> Self {
        Self::with_delay(DEFAULT_DELAY_MS)
    }

    pub fn with_delay(delay_ms: u64) -> Self {
        tracing::info!("[SYNC] Broadcast delay: {}ms", delay_ms);
        Self {
            streams: HashMap::new(),
            video_chunks: Arc::new(StdMutex::new(VecDeque::new())),
            video_init_segment: Arc::new(StdMutex::new(None)),
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
        tracing::info!("[FFMPEG] Video codec set to: {} ({})",
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
        tracing::info!(
            "[FFMPEG] Started RTMP stream {} ({}) → {} [delay={}ms, video+audio on dedicated OS threads]",
            stream_id, lang, rtmp_url, self.broadcast_delay.as_millis()
        );
        Ok(())
    }

    /// Buffer an encoded video chunk from MediaRecorder.
    /// Chunks are timestamped and released after the broadcast delay.
    pub fn push_video_chunk(&self, data: &[u8]) {
        // Save first chunk as init segment (ftyp+moov) for restart recovery
        {
            let mut init = self.video_init_segment.lock().unwrap();
            if init.is_none() {
                tracing::debug!("[VIDEO] saved init segment ({}B)", data.len());
                *init = Some(data.to_vec());
            }
        }
        let mut buf = self.video_chunks.lock().unwrap();
        buf.push_back((Instant::now(), data.to_vec()));
        let buf_len = buf.len();
        let mut dropped = 0;
        // Cap buffer at ~60s of chunks (assuming ~10 chunks/sec at 100ms intervals)
        while buf.len() > MAX_VIDEO_CHUNKS {
            buf.pop_front();
            dropped += 1;
        }
        if dropped > 0 {
            tracing::warn!("[VIDEO] buffer overflow: dropped {} old chunks (buf={})", dropped, buf_len);
        }
        if buf_len % 50 == 0 {
            tracing::debug!("[VIDEO] buffered chunk: {}B (buf_depth={})", data.len(), buf_len);
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
                tracing::debug!(
                    "[AUDIO:{}] queued passthrough audio: {}KB ({:.1}s) queue_depth={}",
                    lang, pcm_len / 1024, pcm_len as f64 / BYTES_PER_SEC, q.len()
                );
                return;
            }
        }
        tracing::warn!("[AUDIO] no stream found for lang={}, audio dropped", lang);
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
                tracing::debug!(
                    "[AUDIO:{}] queued streaming TTS slot, queue_depth={}",
                    lang, q.len()
                );
                return streaming;
            }
        }
        tracing::warn!("[AUDIO] no stream found for lang={}, streaming slot orphaned", lang);
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
                    tracing::error!(
                        "[FFMPEG] Process crashed for lang={}, exit={}, restarting...",
                        stream.lang, code
                    );
                    if stream.restart_count >= MAX_FFMPEG_RESTARTS {
                        tracing::error!(
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
                        tracing::error!(
                            "[FFMPEG] RTMP connection error for lang={}, killing for restart",
                            stream.lang
                        );
                        let _ = stream.child.kill();
                        let _ = stream.child.wait();
                        if stream.restart_count >= MAX_FFMPEG_RESTARTS {
                            tracing::error!(
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
                    tracing::error!("[FFMPEG] Error checking process status for {}: {}", id, e);
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
                tracing::info!(
                    "[FFMPEG] Restarted stream {} ({}) attempt {}/{}",
                    id, lang, prev_count + 1, MAX_FFMPEG_RESTARTS
                );
            }
            Err(e) => {
                tracing::error!(
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
        tracing::info!("[FFMPEG:{}] creating FIFO: {}", stream_id, audio_fifo);
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
            tracing::info!("[FFMPEG] Encoding {} → H.264 (ultrafast)", self.video_codec);
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

        tracing::info!(
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
        tracing::info!("[FFMPEG:{}] spawned PID={}", stream_id, child.id());

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
                                tracing::warn!("[FFMPEG:{}] {}", sid, l);
                                // Detect RTMP connection failures
                                let lower = l.to_lowercase();
                                if lower.contains("connection refused")
                                    || lower.contains("connection reset")
                                    || lower.contains("broken pipe")
                                    || lower.contains("connection timed out")
                                    || lower.contains("i/o error")
                                    || lower.contains("error writing trailer")
                                {
                                    tracing::error!("[FFMPEG:{}] RTMP error detected, flagging for restart", sid);
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

        let is_restart = existing_queue.is_some();
        let audio_queue = existing_queue
            .unwrap_or_else(|| Arc::new(StdMutex::new(VecDeque::new())));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let delay = self.broadcast_delay;

        // Spawn video drain thread — forwards delayed encoded chunks to FFmpeg stdin
        let video_chunk_buf = self.video_chunks.clone();
        let video_init = self.video_init_segment.clone();
        let video_stop = stop_flag.clone();
        let video_sid = stream_id.to_string();
        let video_handle = thread::Builder::new()
            .name(format!("video-drain-{}", stream_id))
            .spawn(move || {
                video_drain::video_chunk_drain_loop(video_sid, video_chunk_buf, video_init, is_restart, stdin, delay, video_stop);
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
                audio_drain::audio_drain_loop(audio_sid, audio_aq, audio_fifo_path, delay, audio_stop);
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
                    tracing::info!("[FFMPEG:{}] killed", id);
                }
                Err(e) => tracing::error!("[FFMPEG:{}] kill error: {}", id, e),
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
                        Ok(Ok(Err(_))) => tracing::error!("[FFMPEG:{}] {} thread panicked", id_clone, label),
                        Ok(Err(_)) => tracing::warn!("[FFMPEG:{}] {} thread join cancelled", id_clone, label),
                        Err(_) => tracing::warn!("[FFMPEG:{}] {} thread join timed out (3s), abandoning", id_clone, label),
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
            tracing::info!("[RTMP] restart_all: no streams to restart");
            return;
        }

        tracing::info!("[RTMP] restarting {} stream(s)", configs.len());
        self.stop_all().await;

        for (id, lang, url, aq) in configs {
            self.restart_stream(&id, &lang, &url, 0, aq);
        }
        tracing::info!("[RTMP] restart complete");
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
        tracing::info!("[HEALTH] FFmpeg health monitor started (check every 2s)");
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        let mut check_count: u64 = 0;
        loop {
            interval.tick().await;
            if stop_flag.load(Ordering::Acquire) {
                tracing::info!("[HEALTH] stop signal received, exiting");
                break;
            }
            check_count += 1;
            // Detect crashes (quick, non-blocking check)
            let crashed = {
                let mut mgr = manager.lock().await;
                mgr.detect_crashed()
            };
            if !crashed.is_empty() {
                tracing::warn!("[HEALTH] check #{}: {} crashed stream(s) detected", check_count, crashed.len());
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
