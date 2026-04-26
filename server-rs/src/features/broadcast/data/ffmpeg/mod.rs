//! FFmpeg RTMP muxer: per-stream delayed host media mixed with translated TTS.
//!
//! Model: every RTMP output stream has an independent `delay_ms`. Host audio
//! and video are fanned out to every stream's delay buffer the instant they
//! arrive. Each stream's drain threads emit media only after it has aged by
//! the stream's configured delay, mixing in any queued TTS PCM at emit time.
//!
//! Why this is simpler than the previous utterance-timestamp scheduler:
//! - No global clock, no `BROADCAST_DELAY_MS` env.
//! - No per-utterance truncation / fade-out / alignment.
//! - Source-language streams are just `is_source = true` → skip the TTS mix.
//! - Different target languages can have different delays (tuned to their
//!   expected STT+translate+TTS latency) — frontend decides, D1 persists,
//!   Fargate reads via the session bundle.

mod args;
mod drain;
mod mixer;
mod orphan;

pub use args::{VideoInputMode, drain_stderr_lines};
pub use orphan::{decode_mp3_to_pcm, kill_orphan_ffmpeg};

use args::build_ffmpeg_args;
use drain::{
    AudioDrainCtx, VideoDrainCtx, audio_drain_loop, video_copy_drain_loop, video_drain_loop,
};

use crate::features::broadcast::domain::SessionMetrics;

use std::collections::{HashMap, VecDeque};
use std::io::BufReader;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ── Timing + format constants ─────────────────────────────

/// Cap the host audio delay buffer per stream at ~20 s of samples.
/// (Prevents unbounded growth if the drain thread falls behind.)
const HOST_AUDIO_CAP_BYTES: usize = 20 * 88_200;
/// Cap the host video buffer at ~20 s of frames @ 30 fps.
const HOST_VIDEO_CAP_FRAMES: usize = 20 * 30;
/// Host camera/original audio must be a live continuous stream. Translated
/// TTS is mixed in when it arrives; it must not delay or stretch source media.
const HOST_MEDIA_DELAY: Duration = Duration::ZERO;
/// Cap the TTS queue at 60 s of PCM. Oldest bytes are dropped on overflow so
/// the translated speech stays fresh rather than falling further behind.
/// Raised from 5 s in April 2026: a single 291-char Korean utterance renders
/// to ~15 s of PCM, which at the old 5 s cap got its head silently chopped
/// off — listeners heard translations starting mid-sentence. 60 s gives a
/// 3x margin over worst-case ElevenLabs slowdown backlog.
const TTS_QUEUE_CAP_BYTES: usize = 60 * 88_200;
/// Max FFmpeg restart attempts per stream.
const MAX_FFMPEG_RESTARTS: u32 = 3;
/// Delay between FFmpeg restart attempts.
const FFMPEG_RESTART_DELAY: Duration = Duration::from_secs(2);
/// Force an FFmpeg restart if no bytes have been written to the child for
/// this long. Grip regional endpoints drop silently after ~30 min; the
/// stream appears fine (FFmpeg hasn't exited) but audio/video stops flowing.
/// Pre-emptively killing the child surfaces it to `detect_crashed`, which
/// then restarts the stream with the existing delay buffers preserved.
const IDLE_RESTART_THRESHOLD: Duration = Duration::from_secs(25);

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ── Per-stream shared state ───────────────────────────────

/// Timestamped chunk of host media. The tuple is (received_at, bytes). A chunk
/// becomes eligible for emission at `received_at + stream.delay`.
type TimedChunk = (Instant, Vec<u8>);

pub(crate) struct StreamBuffers {
    audio: Arc<StdMutex<VecDeque<TimedChunk>>>,
    video: Arc<StdMutex<VecDeque<TimedChunk>>>,
    tts: Arc<StdMutex<VecDeque<u8>>>,
}

impl StreamBuffers {
    fn new() -> Self {
        Self {
            audio: Arc::new(StdMutex::new(VecDeque::new())),
            video: Arc::new(StdMutex::new(VecDeque::new())),
            tts: Arc::new(StdMutex::new(VecDeque::new())),
        }
    }
}

struct RtmpStream {
    child: std::process::Child,
    video_handle: Option<thread::JoinHandle<()>>,
    audio_handle: Option<thread::JoinHandle<()>>,
    audio_fifo: String,
    lang: String,
    rtmp_url: String,
    delay: Duration,
    is_source: bool,
    host_gain: f32,
    /// User explicitly selected "Passthrough (source)" for this destination.
    /// The pipeline skips STT+translate+TTS and re-broadcasts host audio raw.
    /// Implementation-wise this is a superset of `is_source=true` — we keep
    /// it as a dedicated flag so traces + future bifurcations (e.g. per-
    /// stream billing exemption) can tell "user picked passthrough" apart
    /// from "stream's lang coincidentally equals source_lang".
    passthrough: bool,
    buffers: StreamBuffers,
    stop_flag: Arc<AtomicBool>,
    restart_count: u32,
    /// Unix-ms wall clock of the most recent successful FFmpeg-stdin write
    /// from either drain thread. The health monitor uses it to detect silent
    /// output stalls that don't crash FFmpeg (see `IDLE_RESTART_THRESHOLD`).
    last_write_ms: Arc<AtomicI64>,
}

pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
    video_input_mode: VideoInputMode,
    /// Optional billing counters. `None` in unit tests / paths that don't
    /// care about metrics; `Some` when the session wires one via
    /// `set_metrics`. Cloned into each drain thread so increments stay
    /// lock-free.
    metrics: Option<Arc<SessionMetrics>>,
}

type CrashedStreamSnapshot = (
    String,
    String,
    String,
    u64,
    bool,
    f32,
    bool,
    u32,
    StreamBuffers,
);

struct RestartStreamArgs {
    id: String,
    lang: String,
    rtmp_url: String,
    delay_ms: u64,
    is_source: bool,
    host_gain: f32,
    passthrough: bool,
    prev_count: u32,
    buffers: StreamBuffers,
}

struct StreamSpawnArgs {
    stream_id: String,
    lang: String,
    rtmp_url: String,
    delay_ms: u64,
    is_source: bool,
    host_gain: f32,
    passthrough: bool,
    existing_buffers: Option<StreamBuffers>,
}

/// Public signature for `RtmpManager::start_stream`. Grouping the per-stream
/// inputs into a struct keeps the call site readable as the number of flags
/// grows (passthrough joined `is_source` + `host_gain`) and stays inside the
/// §3.3 arg budget.
pub struct StartStreamArgs<'a> {
    pub stream_id: &'a str,
    pub lang: &'a str,
    pub rtmp_url: &'a str,
    pub delay_ms: u64,
    pub is_source: bool,
    pub host_gain: f32,
    /// See `RtmpStream::passthrough`. Fargate skips STT/translate/TTS entirely
    /// for passthrough streams — host audio re-broadcast at gain 1.0.
    pub passthrough: bool,
}

impl Default for RtmpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RtmpManager {
    pub fn new() -> Self {
        Self::new_with_video_input(VideoInputMode::Mjpeg)
    }

    pub fn new_with_video_input(video_input_mode: VideoInputMode) -> Self {
        Self {
            streams: HashMap::new(),
            video_input_mode,
            metrics: None,
        }
    }

    /// Attach a billing-metrics sink. Must be called before `start_stream`
    /// so per-stream drain threads receive the counter refs at spawn time.
    pub fn set_metrics(&mut self, metrics: Arc<SessionMetrics>) {
        self.metrics = Some(metrics);
    }

    /// Start an FFmpeg RTMP process with dedicated video + audio drain threads.
    ///
    /// `host_gain` is the multiplier applied to the delayed host audio before
    /// mixing with translated TTS (1.0 for source streams, typically 0.2 for
    /// ducked target streams). `passthrough` streams always spawn with
    /// `is_source=true` semantics and gain pinned at 1.0.
    pub fn start_stream(&mut self, args: StartStreamArgs<'_>) -> Result<(), String> {
        self.spawn_stream_inner(StreamSpawnArgs {
            stream_id: args.stream_id.to_string(),
            lang: args.lang.to_string(),
            rtmp_url: args.rtmp_url.to_string(),
            delay_ms: args.delay_ms,
            is_source: args.is_source,
            host_gain: args.host_gain,
            passthrough: args.passthrough,
            existing_buffers: None,
        })?;
        tracing::info!(
            stream_id = %args.stream_id,
            lang = %args.lang,
            rtmp_url = %args.rtmp_url,
            delay_ms = args.delay_ms,
            is_source = args.is_source,
            host_gain = args.host_gain,
            passthrough = args.passthrough,
            "ffmpeg rtmp stream started"
        );
        Ok(())
    }

    /// Push a host video frame (JPEG) into every stream's delay buffer.
    pub fn push_video_frame(&self, jpeg_bytes: &[u8]) {
        if self.video_input_mode != VideoInputMode::Mjpeg {
            return;
        }
        let now = Instant::now();
        for stream in self.streams.values() {
            let mut buf = stream.buffers.video.lock().unwrap();
            buf.push_back((now, jpeg_bytes.to_vec()));
            while buf.len() > HOST_VIDEO_CAP_FRAMES {
                buf.pop_front();
            }
        }
    }

    /// Push one H.264 Annex B access unit from WebRTC RTP depacketization into
    /// every stream's live video buffer.
    pub fn push_h264_annex_b(&self, chunk: &[u8]) {
        if self.video_input_mode != VideoInputMode::H264AnnexB || chunk.is_empty() {
            return;
        }
        let now = Instant::now();
        for stream in self.streams.values() {
            let mut buf = stream.buffers.video.lock().unwrap();
            buf.push_back((now, chunk.to_vec()));
            while buf.len() > HOST_VIDEO_CAP_FRAMES {
                buf.pop_front();
            }
        }
    }

    /// Push raw host PCM (s16le 44.1 kHz mono) into every stream's delay buffer.
    pub fn push_host_audio(&self, pcm: &[u8]) {
        if pcm.is_empty() {
            return;
        }
        let now = Instant::now();
        for stream in self.streams.values() {
            let mut buf = stream.buffers.audio.lock().unwrap();
            buf.push_back((now, pcm.to_vec()));
            // Cap total bytes across queued chunks.
            let mut total: usize = buf.iter().map(|(_, b)| b.len()).sum();
            while total > HOST_AUDIO_CAP_BYTES {
                if let Some((_, front)) = buf.pop_front() {
                    total = total.saturating_sub(front.len());
                } else {
                    break;
                }
            }
        }
    }

    /// Append translated PCM for a specific language to that stream's TTS queue.
    /// No timestamps — the mixer plays it in arrival order and the queue is
    /// capped so a slow target can't fall arbitrarily behind.
    ///
    /// Both `is_source` and `passthrough` streams are skipped: neither carries
    /// a translated track, so enqueueing PCM would leak memory up to the cap
    /// and never play back.
    pub fn push_tts(&self, lang: &str, pcm: Vec<u8>) {
        if let Some(m) = &self.metrics {
            m.record_tts_pcm(lang, pcm.len() as u64);
        }
        for stream in self.streams.values() {
            if stream.lang == lang && !stream.is_source && !stream.passthrough {
                let mut q = stream.buffers.tts.lock().unwrap();
                q.extend(pcm.iter().copied());
                let before = q.len();
                while q.len() > TTS_QUEUE_CAP_BYTES {
                    q.pop_front();
                }
                let dropped = before.saturating_sub(q.len());
                // §0.5.4: head-chop was silent before April 2026. A listener
                // would hear a translation starting mid-sentence with no log
                // line anywhere. Now every overflow leaves a greppable trace.
                if dropped > 0 {
                    tracing::warn!(
                        lang = %lang,
                        dropped_bytes = dropped,
                        cap_bytes = TTS_QUEUE_CAP_BYTES,
                        "tts pcm queue overflow — head bytes dropped"
                    );
                }
            }
        }
    }

    /// Kill FFmpeg children that have gone silent (no drain writes in
    /// `IDLE_RESTART_THRESHOLD`). We only send the kill here; `detect_crashed`
    /// on the next monitor tick sees the exit and runs the normal restart
    /// path with buffers reused. Deliberately does not set `stop_flag` so
    /// the stream is treated as a crash, not an intentional shutdown.
    pub(crate) fn kill_idle_streams(&mut self) {
        let now = now_unix_ms();
        let threshold_ms = IDLE_RESTART_THRESHOLD.as_millis() as i64;
        for (id, stream) in &mut self.streams {
            if stream.stop_flag.load(Ordering::Acquire) {
                continue;
            }
            let last = stream.last_write_ms.load(Ordering::Acquire);
            if last == 0 {
                continue;
            }
            let idle_ms = now.saturating_sub(last);
            if idle_ms < threshold_ms {
                continue;
            }
            tracing::warn!(
                stream_id = %id,
                lang = %stream.lang,
                idle_ms,
                threshold_ms,
                "ffmpeg idle beyond threshold, killing to trigger restart"
            );
            match stream.child.kill() {
                Ok(()) => {
                    // Reset the timestamp so we don't try to kill a second
                    // time before `detect_crashed` picks up the exit.
                    stream.last_write_ms.store(now, Ordering::Release);
                }
                Err(e) => tracing::error!(
                    stream_id = %id,
                    error = %e,
                    "ffmpeg idle-kill failed"
                ),
            }
        }
    }

    /// Scan for crashed FFmpeg processes and surface the restart context.
    pub(crate) fn detect_crashed(&mut self) -> Vec<CrashedStreamSnapshot> {
        let mut to_restart = Vec::new();

        for (id, stream) in &mut self.streams {
            match stream.child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(-1);
                    if stream.stop_flag.load(Ordering::Acquire) {
                        continue;
                    }
                    tracing::warn!(
                        stream_id = %id,
                        lang = %stream.lang,
                        exit_code = code,
                        restart_count = stream.restart_count,
                        "ffmpeg rtmp process crashed, scheduling restart"
                    );
                    if stream.restart_count >= MAX_FFMPEG_RESTARTS {
                        tracing::error!(
                            stream_id = %id,
                            lang = %stream.lang,
                            attempts = MAX_FFMPEG_RESTARTS,
                            "ffmpeg rtmp giving up after max restart attempts"
                        );
                        stream.stop_flag.store(true, Ordering::Release);
                        continue;
                    }
                    to_restart.push(id.clone());
                }
                Ok(None) => {}
                Err(e) => tracing::error!(
                    stream_id = %id,
                    error = %e,
                    "ffmpeg status check failed"
                ),
            }
        }

        let mut result = Vec::new();
        for id in to_restart {
            if let Some(mut old) = self.streams.remove(&id) {
                old.stop_flag.store(true, Ordering::Release);
                let _ = old.child.kill();
                let _ = old.child.wait();
                let _ = std::fs::remove_file(&old.audio_fifo);
                result.push((
                    id,
                    old.lang,
                    old.rtmp_url,
                    old.delay.as_millis() as u64,
                    old.is_source,
                    old.host_gain,
                    old.passthrough,
                    old.restart_count,
                    old.buffers,
                ));
            }
        }
        result
    }

    /// Restart a stream after a crash, reusing its buffers so queued host
    /// media + TTS survive the FFmpeg restart.
    fn restart_stream(&mut self, args: RestartStreamArgs) {
        match self.spawn_stream_inner(StreamSpawnArgs {
            stream_id: args.id.clone(),
            lang: args.lang.clone(),
            rtmp_url: args.rtmp_url.clone(),
            delay_ms: args.delay_ms,
            is_source: args.is_source,
            host_gain: args.host_gain,
            passthrough: args.passthrough,
            existing_buffers: Some(args.buffers),
        }) {
            Ok(()) => {
                if let Some(stream) = self.streams.get_mut(&args.id) {
                    stream.restart_count = args.prev_count + 1;
                }
                tracing::info!(
                    stream_id = %args.id,
                    lang = %args.lang,
                    attempt = args.prev_count + 1,
                    max_attempts = MAX_FFMPEG_RESTARTS,
                    "ffmpeg rtmp restarted"
                );
            }
            Err(e) => tracing::error!(
                stream_id = %args.id,
                lang = %args.lang,
                error = %e,
                "ffmpeg rtmp restart failed"
            ),
        }
    }

    fn spawn_stream_inner(&mut self, args: StreamSpawnArgs) -> Result<(), String> {
        let audio_fifo = format!("/tmp/brivva_audio_{}", args.stream_id);

        let _ = std::fs::remove_file(&audio_fifo);
        std::process::Command::new("mkfifo")
            .arg(&audio_fifo)
            .output()
            .map_err(|e| format!("mkfifo failed: {}", e))?;

        let ffmpeg_args = build_ffmpeg_args(&audio_fifo, &args.rtmp_url, self.video_input_mode);
        tracing::info!(
            stream_id = %args.stream_id,
            lang = %args.lang,
            is_source = args.is_source,
            passthrough = args.passthrough,
            video_input_mode = ?self.video_input_mode,
            "ffmpeg spawn: caption-free RTMP muxer started"
        );

        let mut child = std::process::Command::new("ffmpeg")
            .args(&ffmpeg_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;

        // Drain ffmpeg's stderr line-by-line so its diagnostics land in our
        // tracing pipeline. Without this reader the piped stderr fills up
        // (~64 KB kernel buffer) and eventually blocks ffmpeg's log writes,
        // but more importantly we were flying blind on every RTMP failure:
        // Grip drops, ffmpeg exits, the restart loop spins, and no log line
        // ever told us why. The thread exits naturally when ffmpeg closes
        // stderr on process exit (BufRead::lines yields None at EOF).
        if let Some(stderr) = child.stderr.take() {
            let sid_for_log = args.stream_id.clone();
            let lang_for_log = args.lang.clone();
            let thread_name = format!("stderr-drain-{}", args.stream_id);
            if let Err(e) = thread::Builder::new().name(thread_name).spawn(move || {
                drain_stderr_lines(BufReader::new(stderr), |line| {
                    tracing::warn!(
                        stream_id = %sid_for_log,
                        lang = %lang_for_log,
                        "ffmpeg stderr: {line}"
                    );
                });
            }) {
                tracing::error!(
                    stream_id = %args.stream_id,
                    error = %e,
                    "ffmpeg stderr drain thread spawn failed"
                );
            }
        }

        let stdin = child.stdin.take().ok_or("No FFmpeg stdin")?;
        let buffers = args.existing_buffers.unwrap_or_else(StreamBuffers::new);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let configured_delay = Duration::from_millis(args.delay_ms);
        let delay = HOST_MEDIA_DELAY;
        // Seed last_write to now so a freshly-spawned stream isn't instantly
        // classified as idle before the drain threads have had a chance to
        // write their first tick.
        let last_write_ms = Arc::new(AtomicI64::new(now_unix_ms()));

        // Video drain
        let v_buf = buffers.video.clone();
        let v_stop = stop_flag.clone();
        let v_sid = args.stream_id.clone();
        let v_last = last_write_ms.clone();
        let v_metrics = self.metrics.clone();
        let video_input_mode = self.video_input_mode;
        let video_handle = thread::Builder::new()
            .name(format!("video-drain-{}", args.stream_id))
            .spawn(move || {
                let ctx = VideoDrainCtx {
                    stream_id: v_sid,
                    video_buf: v_buf,
                    stdin,
                    delay,
                    stop: v_stop,
                    last_write_ms: v_last,
                    metrics: v_metrics,
                };
                match video_input_mode {
                    VideoInputMode::Mjpeg => video_drain_loop(ctx),
                    VideoInputMode::H264AnnexB => video_copy_drain_loop(ctx),
                }
            })
            .map_err(|e| format!("Video thread spawn failed: {}", e))?;

        // Audio drain
        let audio_ctx = AudioDrainCtx {
            stream_id: args.stream_id.clone(),
            host_buf: buffers.audio.clone(),
            tts_queue: buffers.tts.clone(),
            fifo_path: audio_fifo.clone(),
            delay,
            is_source: args.is_source,
            host_gain: args.host_gain,
            stop: stop_flag.clone(),
            last_write_ms: last_write_ms.clone(),
            metrics: self.metrics.clone(),
        };
        let audio_handle = thread::Builder::new()
            .name(format!("audio-drain-{}", args.stream_id))
            .spawn(move || audio_drain_loop(audio_ctx))
            .map_err(|e| format!("Audio thread spawn failed: {}", e))?;

        self.streams.insert(
            args.stream_id.clone(),
            RtmpStream {
                child,
                video_handle: Some(video_handle),
                audio_handle: Some(audio_handle),
                audio_fifo,
                lang: args.lang,
                rtmp_url: args.rtmp_url,
                delay: configured_delay,
                is_source: args.is_source,
                host_gain: args.host_gain,
                passthrough: args.passthrough,
                buffers,
                stop_flag,
                restart_count: 0,
                last_write_ms,
            },
        );

        Ok(())
    }

    pub async fn stop_all(&mut self) {
        for (id, mut stream) in self.streams.drain() {
            stream.stop_flag.store(true, Ordering::Release);
            match stream.child.kill() {
                Ok(_) => {
                    let _ = stream.child.wait();
                    tracing::info!(stream_id = %id, "ffmpeg rtmp process killed (stop_all)");
                }
                Err(e) => tracing::error!(stream_id = %id, error = %e, "ffmpeg kill failed"),
            }
            // Drain threads exit on the next tick (≤20 ms) once they observe
            // stop_flag or hit EPIPE; 500 ms covers a worst-case scheduler
            // gap. Longer than that we let go without blocking session teardown.
            let join_timeout = Duration::from_millis(500);
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
                        Ok(Ok(Err(_))) => {
                            eprintln!("[FFMPEG:{}] {} thread panicked", id_clone, label)
                        }
                        Ok(Err(_)) => {
                            eprintln!("[FFMPEG:{}] {} thread join cancelled", id_clone, label)
                        }
                        Err(_) => tracing::debug!(
                            stream_id = %id_clone,
                            thread = label,
                            "drain thread join timed out, will exit on next tick"
                        ),
                    }
                }
            }
            let _ = std::fs::remove_file(&stream.audio_fifo);
        }
    }
}

pub type SharedRtmpManager = Arc<tokio::sync::Mutex<RtmpManager>>;

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
            let crashed = {
                let mut mgr = manager.lock().await;
                mgr.kill_idle_streams();
                mgr.detect_crashed()
            };
            for (
                id,
                lang,
                rtmp_url,
                delay_ms,
                is_source,
                host_gain,
                passthrough,
                prev_count,
                buffers,
            ) in crashed
            {
                tokio::time::sleep(FFMPEG_RESTART_DELAY).await;
                if stop_flag.load(Ordering::Acquire) {
                    break;
                }
                let mut mgr = manager.lock().await;
                mgr.restart_stream(RestartStreamArgs {
                    id,
                    lang,
                    rtmp_url,
                    delay_ms,
                    is_source,
                    host_gain,
                    passthrough,
                    prev_count,
                    buffers,
                });
            }
        }
    })
}

#[cfg(test)]
mod tests;
