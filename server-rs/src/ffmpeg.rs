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
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

/// Audio waiting to be played at the right point in the delayed timeline
struct QueuedAudio {
    /// Source timestamp when this utterance started (host speaking)
    play_at: Instant,
    /// Raw PCM s16le 44100Hz mono
    pcm: Vec<u8>,
}

/// State for draining queued audio chunk-by-chunk
struct ActiveAudio {
    pcm: Vec<u8>,
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
}

/// Manages all FFmpeg RTMP streams for a session.
///
/// Uses a shared frame buffer + per-stream drain threads for synchronized output.
pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
    /// Shared ring buffer of timestamped video frames from the host webcam
    frame_buffer: Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    /// Fixed broadcast delay applied to all streams
    broadcast_delay: Duration,
}

/// Video: 33.33ms per frame at 30fps
const FRAME_INTERVAL: Duration = Duration::from_nanos(33_333_333);
/// Audio: 20ms per tick
const AUDIO_TICK: Duration = Duration::from_millis(20);
/// Audio bytes per 20ms tick: 44100Hz × 2 bytes/sample × 1 channel × 0.02s = 1764 bytes
const AUDIO_BYTES_PER_TICK: usize = 1764;
/// Max frames to keep in buffer (~15s at 30fps)
const MAX_BUFFER_FRAMES: usize = 450;
/// Default broadcast delay (5s gives chunked utterances enough pipeline budget)
const DEFAULT_DELAY_MS: u64 = 5000;
/// Max FFmpeg restart attempts per stream
const MAX_FFMPEG_RESTARTS: u32 = 3;
/// Delay between FFmpeg restart attempts
const FFMPEG_RESTART_DELAY: Duration = Duration::from_secs(2);
/// Jitter warning threshold
const JITTER_WARN_THRESHOLD: Duration = Duration::from_millis(5);
/// Fade-out duration in bytes: 50ms at 44100Hz mono 16-bit = 4410 bytes
const FADE_OUT_BYTES: usize = 4410;
/// Audio bytes per second: 44100Hz × 2 bytes/sample = 88200
const BYTES_PER_SEC: f64 = 88200.0;

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

    /// Returns the broadcast delay for TTS timeout calculations
    pub fn broadcast_delay(&self) -> Duration {
        self.broadcast_delay
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

    /// Buffer a video frame from the host webcam.
    /// Frames are stored with their capture timestamp and picked up by drain threads.
    pub fn push_video_frame(&self, jpeg_bytes: &[u8]) {
        let mut buf = self.frame_buffer.lock().unwrap();
        buf.push_back((Instant::now(), jpeg_bytes.to_vec()));
        while buf.len() > MAX_BUFFER_FRAMES {
            buf.pop_front();
        }
    }

    /// Queue translated audio for synced playback at the right point in the delayed timeline.
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
                Ok(None) => {}
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
        std::process::Command::new("mkfifo")
            .arg(&audio_fifo)
            .output()
            .map_err(|e| format!("mkfifo failed: {}", e))?;

        // Spawn FFmpeg
        let mut child = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel", "warning",
                "-f", "image2pipe",
                "-framerate", "30",
                "-i", "pipe:0",
                "-f", "s16le",
                "-ar", "44100",
                "-ac", "1",
                "-i", &audio_fifo,
                "-c:v", "libx264",
                "-preset", "ultrafast",
                "-tune", "zerolatency",
                "-crf", "20",
                "-maxrate", "35000k",
                "-bufsize", "70000k",
                "-pix_fmt", "yuv420p",
                "-g", "60",
                "-c:a", "aac",
                "-ac:a", "2",
                "-b:a", "128k",
                "-map", "0:v",
                "-map", "1:a",
                "-f", "flv",
                rtmp_url,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;

        let stdin = child.stdin.take().ok_or("No FFmpeg stdin")?;

        let audio_queue = existing_queue
            .unwrap_or_else(|| Arc::new(StdMutex::new(VecDeque::new())));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let delay = self.broadcast_delay;

        // Spawn video drain thread
        let video_frame_buf = self.frame_buffer.clone();
        let video_stop = stop_flag.clone();
        let video_sid = stream_id.to_string();
        let video_handle = thread::Builder::new()
            .name(format!("video-drain-{}", stream_id))
            .spawn(move || {
                video_drain_loop(video_sid, video_frame_buf, stdin, delay, video_stop);
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
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            if stop_flag.load(Ordering::Acquire) {
                break;
            }
            // Detect crashes (quick, non-blocking check)
            let crashed = {
                let mut mgr = manager.lock().await;
                mgr.detect_crashed()
            };
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

// ── Video Drain Thread ─────────────────────────────────────

/// Dedicated OS thread: drains video frames at exactly 30fps.
///
/// Reads from the shared frame buffer at the delayed timeline position.
/// Duplicates the last frame if no new frame is available (webcam drop).
/// Logs warnings when tick jitter exceeds 5ms.
fn video_drain_loop(
    stream_id: String,
    frame_buffer: Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    mut stdin: std::process::ChildStdin,
    delay: Duration,
    stop: Arc<AtomicBool>,
) {
    let mut last_frame: Option<Vec<u8>> = None;
    let mut tick_count: u64 = 0;
    let mut next_tick = Instant::now() + FRAME_INTERVAL;

    eprintln!(
        "[VIDEO:{}] drain thread started (30fps, {}ms delay)",
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
        if jitter > JITTER_WARN_THRESHOLD && tick_count > 0 {
            eprintln!(
                "[VIDEO:{}] jitter warning: tick {} was {}ms late",
                stream_id,
                tick_count,
                jitter.as_millis()
            );
        }

        // Anchor next tick to prevent drift accumulation
        next_tick += FRAME_INTERVAL;
        tick_count += 1;

        let target_ts = actual - delay;

        // Pick frame at delayed timeline position
        let frame = {
            let buf = frame_buffer.lock().unwrap();
            find_frame_at(&buf, target_ts)
        };

        let write_result = if let Some(f) = frame {
            let r = stdin.write_all(&f);
            last_frame = Some(f);
            r
        } else if let Some(ref lf) = last_frame {
            // No frame at target — duplicate last frame to maintain 30fps
            stdin.write_all(lf)
        } else {
            // No frames yet at all (startup), skip this tick
            continue;
        };

        if write_result.is_err() {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[VIDEO:{}] write error, exiting", stream_id);
            }
            break;
        }
    }

    drop(stdin);
    eprintln!(
        "[VIDEO:{}] drain thread exited after {} ticks",
        stream_id, tick_count
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

    let silence = vec![0u8; AUDIO_BYTES_PER_TICK];
    let mut active_audio: Option<ActiveAudio> = None;
    let mut tick_count: u64 = 0;
    let mut next_tick = Instant::now() + AUDIO_TICK;

    // Cumulative sample tracking for drift detection
    let start_time = Instant::now();
    let mut total_bytes_written: u64 = 0;

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
        if jitter > JITTER_WARN_THRESHOLD && tick_count > 0 {
            eprintln!(
                "[AUDIO:{}] jitter warning: tick {} was {}ms late",
                stream_id,
                tick_count,
                jitter.as_millis()
            );
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
                    active_audio = Some(ActiveAudio {
                        pcm: audio.pcm,
                        offset: 0,
                    });
                }
            }
        }

        // Write one tick's worth of audio (1764 bytes = 20ms at 44100Hz mono 16-bit)
        let write_result = if let Some(ref mut active) = active_audio {
            let remaining = active.pcm.len() - active.offset;
            if remaining >= AUDIO_BYTES_PER_TICK {
                let end = active.offset + AUDIO_BYTES_PER_TICK;
                let r = fifo.write_all(&active.pcm[active.offset..end]);
                active.offset = end;
                r
            } else if remaining > 0 {
                // Last partial chunk — pad with silence to complete the tick
                let mut chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
                chunk[..remaining].copy_from_slice(&active.pcm[active.offset..]);
                active_audio = None;
                fifo.write_all(&chunk)
            } else {
                active_audio = None;
                fifo.write_all(&silence)
            }
        } else {
            fifo.write_all(&silence)
        };

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

/// Find the latest frame with timestamp <= target in the buffer.
/// Returns None if no frame is old enough yet (initial startup delay).
fn find_frame_at(
    buffer: &VecDeque<(Instant, Vec<u8>)>,
    target: Instant,
) -> Option<Vec<u8>> {
    for (ts, data) in buffer.iter().rev() {
        if *ts <= target {
            return Some(data.clone());
        }
    }
    None
}

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
    let mut child = TokioCommand::new("ffmpeg")
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
