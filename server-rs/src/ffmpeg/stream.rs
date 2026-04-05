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
    VIDEO_CRF, VIDEO_MAX_BITRATE, VIDEO_BUFSIZE, VIDEO_GOP_SIZE,
    AUDIO_BITRATE, AUDIO_CHANNELS_OUT,
    THREAD_JOIN_TIMEOUT_SECS, HEALTH_CHECK_INTERVAL_SECS,
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

impl Default for RtmpManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Public API (headline) ─────────────────────────────────

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
        self.save_init_segment(data);
        let mut buf = self.video_chunks.lock().unwrap();
        buf.push_back((Instant::now(), data.to_vec()));
        let buf_len = buf.len();
        self.drop_overflow_chunks(&mut buf, buf_len);
        if buf_len.is_multiple_of(50) {
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
        let to_restart = self.collect_crashed_streams();
        self.cleanup_crashed_streams(to_restart)
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

    /// Stop all FFmpeg processes and clean up.
    /// Drain threads are joined with a 3-second timeout to prevent cleanup from hanging.
    pub async fn stop_all(&mut self) {
        for (id, mut stream) in self.streams.drain() {
            stream.stop_flag.store(true, Ordering::Release);
            Self::kill_ffmpeg_process(&id, &mut stream.child);
            Self::join_drain_threads(&id, &mut stream).await;
            let _ = std::fs::remove_file(&stream.audio_fifo);
        }
    }

    /// Restart all RTMP streams (stop + re-spawn with same config).
    /// Used when YouTube drops the RTMP connection and needs a fresh start.
    pub async fn restart_all(&mut self) {
        let configs = self.collect_stream_configs();
        if configs.is_empty() {
            tracing::info!("[RTMP] restart_all: no streams to restart");
            return;
        }
        tracing::info!("[RTMP] restarting {} stream(s)", configs.len());
        self.stop_all().await;
        self.respawn_all(configs);
        tracing::info!("[RTMP] restart complete");
    }
}

// ── Private helpers (details) ─────────────────────────────

impl RtmpManager {
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
        let (child, stdin, rtmp_error, audio_fifo) = self.setup_ffmpeg(stream_id, rtmp_url)?;
        let (audio_queue, stop_flag, is_restart) = prepare_stream_state(existing_queue);
        let (video_handle, audio_handle) = self.spawn_drain_threads(
            stream_id, is_restart, stdin, &audio_queue, &audio_fifo, &stop_flag,
        )?;
        self.register_stream(
            stream_id, lang, rtmp_url, child, video_handle, audio_handle,
            audio_fifo, audio_queue, stop_flag, rtmp_error,
        );
        Ok(())
    }

    /// Create the audio FIFO, build args, and spawn the FFmpeg process.
    fn setup_ffmpeg(
        &self,
        stream_id: &str,
        rtmp_url: &str,
    ) -> Result<(std::process::Child, std::process::ChildStdin, Arc<AtomicBool>, String), String> {
        let audio_fifo = create_audio_fifo(stream_id)?;
        let args = self.build_ffmpeg_args(&audio_fifo, rtmp_url);
        let (child, stdin, rtmp_error) = spawn_ffmpeg_process(stream_id, &args)?;
        Ok((child, stdin, rtmp_error, audio_fifo))
    }

    /// Spawn video and audio drain threads.
    fn spawn_drain_threads(
        &self,
        stream_id: &str,
        is_restart: bool,
        stdin: std::process::ChildStdin,
        audio_queue: &Arc<StdMutex<VecDeque<QueuedAudio>>>,
        audio_fifo: &str,
        stop_flag: &Arc<AtomicBool>,
    ) -> Result<(thread::JoinHandle<()>, thread::JoinHandle<()>), String> {
        let video_handle = self.spawn_video_drain(stream_id, is_restart, stdin, stop_flag)?;
        let audio_handle = spawn_audio_drain(stream_id, audio_queue, audio_fifo, self.broadcast_delay, stop_flag)?;
        Ok((video_handle, audio_handle))
    }

    /// Insert the fully-built RtmpStream into the manager's stream map.
    #[allow(clippy::too_many_arguments)]
    fn register_stream(
        &mut self,
        stream_id: &str,
        lang: &str,
        rtmp_url: &str,
        child: std::process::Child,
        video_handle: thread::JoinHandle<()>,
        audio_handle: thread::JoinHandle<()>,
        audio_fifo: String,
        audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
        stop_flag: Arc<AtomicBool>,
        rtmp_error: Arc<AtomicBool>,
    ) {
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
    }

    /// Build the full FFmpeg CLI argument list for video+audio -> RTMP.
    fn build_ffmpeg_args(&self, audio_fifo: &str, rtmp_url: &str) -> Vec<String> {
        let mut args = self.build_input_args(audio_fifo);
        args.extend(self.build_video_encoding_args());
        args.extend(Self::build_output_args(rtmp_url));
        args
    }

    /// Build input arguments: encoded video from stdin, raw PCM from FIFO.
    fn build_input_args(&self, audio_fifo: &str) -> Vec<String> {
        vec![
            "-y".to_string(),
            "-loglevel".to_string(), "warning".to_string(),
            // Video input: encoded stream from MediaRecorder
            "-i".to_string(), "pipe:0".to_string(),
            // Audio input: raw PCM from FIFO
            "-f".to_string(), "s16le".to_string(),
            "-ar".to_string(), "44100".to_string(),
            "-ac".to_string(), "1".to_string(),
            "-i".to_string(), audio_fifo.to_string(),
        ]
    }

    /// Build H.264 video encoding arguments (ultrafast preset).
    fn build_video_encoding_args(&self) -> Vec<String> {
        tracing::info!("[FFMPEG] Encoding {} → H.264 (ultrafast)", self.video_codec);
        vec![
            "-c:v".to_string(), "libx264".to_string(),
            "-preset".to_string(), "ultrafast".to_string(),
            "-tune".to_string(), "zerolatency".to_string(),
            "-crf".to_string(), VIDEO_CRF.to_string(),
            "-maxrate".to_string(), VIDEO_MAX_BITRATE.to_string(),
            "-bufsize".to_string(), VIDEO_BUFSIZE.to_string(),
            "-pix_fmt".to_string(), "yuv420p".to_string(),
            "-g".to_string(), VIDEO_GOP_SIZE.to_string(),
        ]
    }

    /// Build output arguments: AAC audio, FLV container, RTMP destination.
    fn build_output_args(rtmp_url: &str) -> Vec<String> {
        vec![
            "-c:a".to_string(), "aac".to_string(),
            "-ac:a".to_string(), AUDIO_CHANNELS_OUT.to_string(),
            "-b:a".to_string(), AUDIO_BITRATE.to_string(),
            "-map".to_string(), "0:v".to_string(),
            "-map".to_string(), "1:a".to_string(),
            "-f".to_string(), "flv".to_string(),
            // RTMP reconnect: retry on network drops instead of dying
            "-flvflags".to_string(), "no_duration_filesize".to_string(),
            "-rtmp_live".to_string(), "live".to_string(),
            rtmp_url.to_string(),
        ]
    }

    /// Spawn the video drain thread that forwards delayed chunks to FFmpeg stdin.
    fn spawn_video_drain(
        &self,
        stream_id: &str,
        is_restart: bool,
        stdin: std::process::ChildStdin,
        stop_flag: &Arc<AtomicBool>,
    ) -> Result<thread::JoinHandle<()>, String> {
        let video_chunk_buf = self.video_chunks.clone();
        let video_init = self.video_init_segment.clone();
        let video_stop = stop_flag.clone();
        let video_sid = stream_id.to_string();
        let delay = self.broadcast_delay;
        thread::Builder::new()
            .name(format!("video-drain-{}", stream_id))
            .spawn(move || {
                video_drain::video_chunk_drain_loop(video_sid, video_chunk_buf, video_init, is_restart, stdin, delay, video_stop);
            })
            .map_err(|e| format!("Video thread spawn failed: {}", e))
    }

    /// Save the first video chunk as the init segment (ftyp+moov) for restart recovery.
    fn save_init_segment(&self, data: &[u8]) {
        let mut init = self.video_init_segment.lock().unwrap();
        if init.is_none() {
            tracing::debug!("[VIDEO] saved init segment ({}B)", data.len());
            *init = Some(data.to_vec());
        }
    }

    /// Drop oldest chunks when buffer exceeds MAX_VIDEO_CHUNKS.
    fn drop_overflow_chunks(&self, buf: &mut VecDeque<(Instant, Vec<u8>)>, buf_len: usize) {
        let mut dropped = 0;
        while buf.len() > MAX_VIDEO_CHUNKS {
            buf.pop_front();
            dropped += 1;
        }
        if dropped > 0 {
            tracing::warn!("[VIDEO] buffer overflow: dropped {} old chunks (buf={})", dropped, buf_len);
        }
    }

    /// Scan all streams and collect those that have crashed or hit RTMP errors.
    fn collect_crashed_streams(&mut self) -> Vec<(String, String, String)> {
        let mut to_restart = Vec::new();
        for (id, stream) in &mut self.streams {
            if let Some(restart_info) = check_stream_health(id, stream) {
                to_restart.push(restart_info);
            }
        }
        to_restart
    }

    /// Remove crashed streams, clean up resources, and return restart info.
    fn cleanup_crashed_streams(
        &mut self,
        to_restart: Vec<(String, String, String)>,
    ) -> Vec<(String, String, String, u32, Arc<StdMutex<VecDeque<QueuedAudio>>>)> {
        to_restart
            .into_iter()
            .filter_map(|(id, lang, rtmp_url)| {
                let mut old = self.streams.remove(&id)?;
                let (prev_count, audio_queue) = cleanup_single_stream(&mut old);
                Some((id, lang, rtmp_url, prev_count, audio_queue))
            })
            .collect()
    }

    /// Snapshot each stream's config for restart_all.
    fn collect_stream_configs(&self) -> Vec<(String, String, String, Arc<StdMutex<VecDeque<QueuedAudio>>>)> {
        self.streams.iter().map(|(id, s)| {
            (id.clone(), s.lang.clone(), s.rtmp_url.clone(), s.audio_queue.clone())
        }).collect()
    }

    /// Re-spawn all streams from saved configs (after stop_all).
    fn respawn_all(&mut self, configs: Vec<(String, String, String, Arc<StdMutex<VecDeque<QueuedAudio>>>)>) {
        for (id, lang, url, aq) in configs {
            self.restart_stream(&id, &lang, &url, 0, aq);
        }
    }

    /// Kill an FFmpeg child process and log the result.
    fn kill_ffmpeg_process(id: &str, child: &mut std::process::Child) {
        match child.kill() {
            Ok(_) => {
                let _ = child.wait();
                tracing::info!("[FFMPEG:{}] killed", id);
            }
            Err(e) => tracing::error!("[FFMPEG:{}] kill error: {}", id, e),
        }
    }

    /// Join video and audio drain threads with a timeout.
    async fn join_drain_threads(id: &str, stream: &mut RtmpStream) {
        let join_timeout = Duration::from_secs(THREAD_JOIN_TIMEOUT_SECS);
        for (label, handle) in [
            ("video", stream.video_handle.take()),
            ("audio", stream.audio_handle.take()),
        ] {
            if let Some(h) = handle {
                let id_clone = id.to_string();
                let result = tokio::time::timeout(join_timeout, async {
                    tokio::task::spawn_blocking(move || h.join()).await
                })
                .await;
                log_thread_join_result(&id_clone, label, result);
            }
        }
    }
}

// ── Free functions (implementation details) ───────────────

/// Resolve the audio queue and stop flag for a new or restarted stream.
fn prepare_stream_state(
    existing_queue: Option<Arc<StdMutex<VecDeque<QueuedAudio>>>>,
) -> (Arc<StdMutex<VecDeque<QueuedAudio>>>, Arc<AtomicBool>, bool) {
    let is_restart = existing_queue.is_some();
    let audio_queue = existing_queue
        .unwrap_or_else(|| Arc::new(StdMutex::new(VecDeque::new())));
    let stop_flag = Arc::new(AtomicBool::new(false));
    (audio_queue, stop_flag, is_restart)
}

/// Create a named FIFO for audio data at /tmp/brivva_audio_{stream_id}.
fn create_audio_fifo(stream_id: &str) -> Result<String, String> {
    let path = format!("/tmp/brivva_audio_{}", stream_id);
    let _ = std::fs::remove_file(&path);
    tracing::info!("[FFMPEG:{}] creating FIFO: {}", stream_id, path);
    std::process::Command::new("mkfifo")
        .arg(&path)
        .output()
        .map_err(|e| format!("mkfifo failed: {}", e))?;
    Ok(path)
}

/// Spawn the FFmpeg process. Returns the child, its stdin, and an RTMP error flag.
fn spawn_ffmpeg_process(
    stream_id: &str,
    args: &[String],
) -> Result<(std::process::Child, std::process::ChildStdin, Arc<AtomicBool>), String> {
    tracing::info!(
        "[FFMPEG:{}] spawning: {} {}",
        stream_id, &*FFMPEG_BIN, args.join(" ")
    );
    let mut child = std::process::Command::new(&*FFMPEG_BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;
    tracing::info!("[FFMPEG:{}] spawned PID={}", stream_id, child.id());

    let stdin = child.stdin.take().ok_or("No FFmpeg stdin")?;
    let rtmp_error = Arc::new(AtomicBool::new(false));
    spawn_stderr_reader(stream_id, &mut child, &rtmp_error);
    Ok((child, stdin, rtmp_error))
}

/// Drain FFmpeg stderr in background to prevent pipe buffer from filling up
/// (which would block FFmpeg and stop audio/video processing).
fn spawn_stderr_reader(
    stream_id: &str,
    child: &mut std::process::Child,
    rtmp_error: &Arc<AtomicBool>,
) {
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
                        Ok(l) if !l.is_empty() => process_stderr_line(&sid, &l, &err_flag),
                        Err(_) => break,
                        _ => {}
                    }
                }
            })
            .ok();
    }
}

/// Handle a single non-empty stderr line: log it and flag RTMP errors.
fn process_stderr_line(stream_id: &str, line: &str, err_flag: &AtomicBool) {
    tracing::warn!("[FFMPEG:{}] {}", stream_id, line);
    if is_rtmp_connection_error(line) {
        tracing::error!("[FFMPEG:{}] RTMP error detected, flagging for restart", stream_id);
        err_flag.store(true, Ordering::Release);
    }
}

/// Check whether an FFmpeg stderr line indicates an RTMP connection failure.
fn is_rtmp_connection_error(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("broken pipe")
        || lower.contains("connection timed out")
        || lower.contains("i/o error")
        || lower.contains("error writing trailer")
}

/// Spawn the audio drain thread that writes PCM to the FIFO.
fn spawn_audio_drain(
    stream_id: &str,
    audio_queue: &Arc<StdMutex<VecDeque<QueuedAudio>>>,
    audio_fifo: &str,
    delay: Duration,
    stop_flag: &Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>, String> {
    let aq = audio_queue.clone();
    let stop = stop_flag.clone();
    let sid = stream_id.to_string();
    let fifo = audio_fifo.to_string();
    thread::Builder::new()
        .name(format!("audio-drain-{}", stream_id))
        .spawn(move || {
            audio_drain::audio_drain_loop(sid, aq, fifo, delay, stop);
        })
        .map_err(|e| format!("Audio thread spawn failed: {}", e))
}

/// Check a single stream's health. Returns restart info if it needs restarting.
fn check_stream_health(id: &str, stream: &mut RtmpStream) -> Option<(String, String, String)> {
    match stream.child.try_wait() {
        Ok(Some(status)) => check_exited_process(id, stream, status),
        Ok(None) => check_rtmp_error(id, stream),
        Err(e) => {
            tracing::error!("[FFMPEG] Error checking process status for {}: {}", id, e);
            None
        }
    }
}

/// Handle a process that has already exited (crashed).
fn check_exited_process(
    id: &str,
    stream: &mut RtmpStream,
    status: std::process::ExitStatus,
) -> Option<(String, String, String)> {
    if stream.stop_flag.load(Ordering::Acquire) {
        return None;
    }
    let code = status.code().unwrap_or(-1);
    tracing::error!(
        "[FFMPEG] Process crashed for lang={}, exit={}, restarting...",
        stream.lang, code
    );
    if !can_restart(stream) {
        return None;
    }
    Some((id.to_string(), stream.lang.clone(), stream.rtmp_url.clone()))
}

/// Handle a running process that has an RTMP error flag set.
fn check_rtmp_error(id: &str, stream: &mut RtmpStream) -> Option<(String, String, String)> {
    if !stream.rtmp_error.load(Ordering::Acquire) {
        return None;
    }
    if stream.stop_flag.load(Ordering::Acquire) {
        return None;
    }
    tracing::error!(
        "[FFMPEG] RTMP connection error for lang={}, killing for restart",
        stream.lang
    );
    let _ = stream.child.kill();
    let _ = stream.child.wait();
    if !can_restart(stream) {
        return None;
    }
    Some((id.to_string(), stream.lang.clone(), stream.rtmp_url.clone()))
}

/// Check if a stream can be restarted (under the max restart limit).
/// If not, sets the stop flag and logs the failure.
fn can_restart(stream: &mut RtmpStream) -> bool {
    if stream.restart_count >= MAX_FFMPEG_RESTARTS {
        tracing::error!(
            "[FFMPEG] Failed to restart after {} attempts for lang={}",
            MAX_FFMPEG_RESTARTS, stream.lang
        );
        stream.stop_flag.store(true, Ordering::Release);
        return false;
    }
    true
}

/// Log the result of joining a drain thread.
fn log_thread_join_result(
    id: &str,
    label: &str,
    result: Result<Result<Result<(), Box<dyn std::any::Any + Send>>, tokio::task::JoinError>, tokio::time::error::Elapsed>,
) {
    match result {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(_))) => tracing::error!("[FFMPEG:{}] {} thread panicked", id, label),
        Ok(Err(_)) => tracing::warn!("[FFMPEG:{}] {} thread join cancelled", id, label),
        Err(_) => tracing::warn!("[FFMPEG:{}] {} thread join timed out ({}s), abandoning", id, label, THREAD_JOIN_TIMEOUT_SECS),
    }
}

/// Stop the stream, kill the process, remove the FIFO, and return restart state.
fn cleanup_single_stream(old: &mut RtmpStream) -> (u32, Arc<StdMutex<VecDeque<QueuedAudio>>>) {
    old.stop_flag.store(true, Ordering::Release);
    let _ = old.child.kill();
    let _ = old.child.wait();
    let _ = std::fs::remove_file(&old.audio_fifo);
    (old.restart_count, old.audio_queue.clone())
}

/// Thread-safe wrapper
pub type SharedRtmpManager = Arc<tokio::sync::Mutex<RtmpManager>>;

// ── Health Monitor ────────────────────────────────────────

/// Background health checker that detects crashed FFmpeg processes and restarts them.
struct HealthMonitor {
    manager: SharedRtmpManager,
    stop_flag: Arc<AtomicBool>,
}

impl HealthMonitor {
    /// Main loop: tick every N seconds, detect crashes, restart streams.
    async fn run(self) {
        tracing::info!("[HEALTH] FFmpeg health monitor started (check every {}s)", HEALTH_CHECK_INTERVAL_SECS);
        let mut interval = tokio::time::interval(Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS));
        let mut check_count: u64 = 0;
        loop {
            interval.tick().await;
            if self.stop_flag.load(Ordering::Acquire) {
                tracing::info!("[HEALTH] stop signal received, exiting");
                break;
            }
            check_count += 1;
            self.detect_and_restart_crashes(&mut check_count).await;
        }
    }

    /// Single health-check pass: find crashed streams and restart each one.
    async fn detect_and_restart_crashes(&self, check_count: &mut u64) {
        let crashed = {
            let mut mgr = self.manager.lock().await;
            mgr.detect_crashed()
        };
        if !crashed.is_empty() {
            tracing::warn!("[HEALTH] check #{}: {} crashed stream(s) detected", check_count, crashed.len());
        }
        for (id, lang, rtmp_url, prev_count, audio_queue) in crashed {
            self.restart_one(&id, &lang, &rtmp_url, prev_count, audio_queue).await;
        }
    }

    /// Wait the restart delay, then restart a single crashed stream.
    async fn restart_one(
        &self,
        id: &str,
        lang: &str,
        rtmp_url: &str,
        prev_count: u32,
        audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    ) {
        tokio::time::sleep(FFMPEG_RESTART_DELAY).await;
        if self.stop_flag.load(Ordering::Acquire) {
            return;
        }
        let mut mgr = self.manager.lock().await;
        mgr.restart_stream(id, lang, rtmp_url, prev_count, audio_queue);
    }
}

/// Spawn a background task that periodically checks for crashed FFmpeg processes
/// and restarts them. Runs every N seconds. Stops when stop_flag is set.
pub fn spawn_health_monitor(
    manager: SharedRtmpManager,
    stop_flag: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    let monitor = HealthMonitor { manager, stop_flag };
    tokio::spawn(monitor.run())
}
