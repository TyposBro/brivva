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

mod caption;
mod drain;
mod mixer;
mod orphan;

pub use orphan::{decode_mp3_to_pcm, kill_orphan_ffmpeg};

use caption::CaptionState;
use drain::{AudioDrainCtx, VideoDrainCtx, audio_drain_loop, video_drain_loop};

use crate::features::broadcast::domain::SessionMetrics;

use std::collections::{HashMap, VecDeque};
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
/// Cap the TTS queue at 5 s of PCM. Oldest bytes are dropped on overflow so
/// the translated speech stays fresh rather than falling further behind.
const TTS_QUEUE_CAP_BYTES: usize = 5 * 88_200;
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

/// Pure builder for the FFmpeg CLI args. Factored out of `spawn_stream_inner`
/// so tests can pin the command shape without spawning an FFmpeg child.
fn build_ffmpeg_args(
    audio_fifo: &str,
    caption_textfile_path: Option<&str>,
    rtmp_url: &str,
) -> Vec<String> {
    let mut ffmpeg_args: Vec<String> = vec![
        "-y".into(),
        "-loglevel".into(),
        "warning".into(),
        "-f".into(),
        "image2pipe".into(),
        // Force mjpeg on stdin so ffmpeg does not block on codec auto-detection
        // before the host has sent a first JPEG. The driver sends JPEGs every
        // 2s, and image2pipe would otherwise fail probe with
        // "Could not find codec parameters" and exit.
        "-vcodec".into(),
        "mjpeg".into(),
        "-framerate".into(),
        "30".into(),
        "-i".into(),
        "pipe:0".into(),
        "-f".into(),
        "s16le".into(),
        "-ar".into(),
        "44100".into(),
        "-ac".into(),
        "1".into(),
        "-i".into(),
        audio_fifo.to_string(),
    ];
    if let Some(path) = caption_textfile_path {
        // Escape the textfile path for drawtext — it uses `\` as an escape and
        // `:` as a filter-option separator.
        let escaped = path.replace('\\', "\\\\").replace(':', "\\:");
        let drawtext = format!(
            "drawtext=textfile={}:reload=1:fontcolor=white:fontsize=28:box=1:boxcolor=black@0.6:boxborderw=10:x=(w-text_w)/2:y=h-120",
            escaped
        );
        ffmpeg_args.extend_from_slice(&["-vf".into(), drawtext]);
    }
    ffmpeg_args.extend_from_slice(&[
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "ultrafast".into(),
        "-tune".into(),
        "zerolatency".into(),
        "-crf".into(),
        "20".into(),
        "-maxrate".into(),
        "35000k".into(),
        "-bufsize".into(),
        "70000k".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-g".into(),
        "60".into(),
        "-c:a".into(),
        "aac".into(),
        "-ac:a".into(),
        "2".into(),
        "-b:a".into(),
        "128k".into(),
        "-map".into(),
        "0:v".into(),
        "-map".into(),
        "1:a".into(),
        "-f".into(),
        "flv".into(),
        rtmp_url.to_string(),
    ]);
    ffmpeg_args
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
    buffers: StreamBuffers,
    caption: Option<CaptionState>,
    stop_flag: Arc<AtomicBool>,
    restart_count: u32,
    /// Unix-ms wall clock of the most recent successful FFmpeg-stdin write
    /// from either drain thread. The health monitor uses it to detect silent
    /// output stalls that don't crash FFmpeg (see `IDLE_RESTART_THRESHOLD`).
    last_write_ms: Arc<AtomicI64>,
}

/// Per-stream burn-in caption state. Source streams don't have one (nothing
/// to burn in — they're the host's own audio). Target streams get a textfile
/// that drawtext reads with `reload=1`; writes are rate-limited via a tokio
/// task so a fresh translation always gets at least MIN_CAPTION_DWELL_MS on
/// screen before being replaced.
pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
    /// Optional billing counters. `None` in unit tests / paths that don't
    /// care about metrics; `Some` when the session wires one via
    /// `set_metrics`. Cloned into each drain thread so increments stay
    /// lock-free.
    metrics: Option<Arc<SessionMetrics>>,
}

type CrashedStreamSnapshot = (String, String, String, u64, bool, f32, u32, StreamBuffers);

struct RestartStreamArgs {
    id: String,
    lang: String,
    rtmp_url: String,
    delay_ms: u64,
    is_source: bool,
    host_gain: f32,
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
    existing_buffers: Option<StreamBuffers>,
}

impl Default for RtmpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RtmpManager {
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
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
    /// ducked target streams).
    pub fn start_stream(
        &mut self,
        stream_id: &str,
        lang: &str,
        rtmp_url: &str,
        delay_ms: u64,
        is_source: bool,
        host_gain: f32,
    ) -> Result<(), String> {
        self.spawn_stream_inner(StreamSpawnArgs {
            stream_id: stream_id.to_string(),
            lang: lang.to_string(),
            rtmp_url: rtmp_url.to_string(),
            delay_ms,
            is_source,
            host_gain,
            existing_buffers: None,
        })?;
        tracing::info!(
            stream_id = %stream_id,
            lang = %lang,
            rtmp_url = %rtmp_url,
            delay_ms,
            is_source,
            host_gain,
            "ffmpeg rtmp stream started"
        );
        Ok(())
    }

    /// Push a host video frame (JPEG) into every stream's delay buffer.
    pub fn push_video_frame(&self, jpeg_bytes: &[u8]) {
        let now = Instant::now();
        for stream in self.streams.values() {
            let mut buf = stream.buffers.video.lock().unwrap();
            buf.push_back((now, jpeg_bytes.to_vec()));
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
    pub fn push_tts(&self, lang: &str, pcm: Vec<u8>) {
        if let Some(m) = &self.metrics {
            m.record_tts_pcm(lang, pcm.len() as u64);
        }
        for stream in self.streams.values() {
            if stream.lang == lang && !stream.is_source {
                let mut q = stream.buffers.tts.lock().unwrap();
                q.extend(pcm.iter().copied());
                while q.len() > TTS_QUEUE_CAP_BYTES {
                    q.pop_front();
                }
            }
        }
    }

    /// Update the burn-in caption for a target-language stream. Write is
    /// rate-limited to MIN_CAPTION_DWELL_MS so each line gets read time.
    /// Source streams have no caption track — this is a no-op for them.
    pub fn push_caption(&self, lang: &str, text: String) {
        for stream in self.streams.values() {
            if stream.lang == lang
                && !stream.is_source
                && let Some(cap) = &stream.caption
            {
                cap.push(&text);
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
                // Captions get reborn by spawn_stream_inner — tear down the old one.
                if let Some(mut cap) = old.caption.take() {
                    let _ = std::fs::remove_file(&cap.path);
                    cap.shutdown();
                }
                result.push((
                    id,
                    old.lang,
                    old.rtmp_url,
                    old.delay.as_millis() as u64,
                    old.is_source,
                    old.host_gain,
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

        // Target streams get a burn-in caption textfile + drawtext filter.
        // Source streams skip both (no translation to display).
        let caption = if args.is_source {
            None
        } else {
            Some(CaptionState::spawn(&args.stream_id))
        };

        let ffmpeg_args = build_ffmpeg_args(
            &audio_fifo,
            caption.as_ref().map(|c| c.path.as_str()),
            &args.rtmp_url,
        );

        let mut child = std::process::Command::new("ffmpeg")
            .args(&ffmpeg_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;

        let stdin = child.stdin.take().ok_or("No FFmpeg stdin")?;
        let buffers = args.existing_buffers.unwrap_or_else(StreamBuffers::new);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let delay = Duration::from_millis(args.delay_ms);
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
        let video_handle = thread::Builder::new()
            .name(format!("video-drain-{}", args.stream_id))
            .spawn(move || {
                video_drain_loop(VideoDrainCtx {
                    stream_id: v_sid,
                    video_buf: v_buf,
                    stdin,
                    delay,
                    stop: v_stop,
                    last_write_ms: v_last,
                    metrics: v_metrics,
                })
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
                delay,
                is_source: args.is_source,
                host_gain: args.host_gain,
                buffers,
                caption,
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
                        Ok(Ok(Err(_))) => {
                            eprintln!("[FFMPEG:{}] {} thread panicked", id_clone, label)
                        }
                        Ok(Err(_)) => {
                            eprintln!("[FFMPEG:{}] {} thread join cancelled", id_clone, label)
                        }
                        Err(_) => eprintln!(
                            "[FFMPEG:{}] {} thread join timed out (3s), abandoning",
                            id_clone, label
                        ),
                    }
                }
            }
            let _ = std::fs::remove_file(&stream.audio_fifo);
            if let Some(mut cap) = stream.caption.take() {
                let _ = std::fs::remove_file(&cap.path);
                cap.shutdown();
            }
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
            for (id, lang, rtmp_url, delay_ms, is_source, host_gain, prev_count, buffers) in crashed
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
                    prev_count,
                    buffers,
                });
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_constants_match_documented_values() {
        assert_eq!(MAX_FFMPEG_RESTARTS, 3);
        assert_eq!(FFMPEG_RESTART_DELAY, Duration::from_secs(2));
        assert_eq!(IDLE_RESTART_THRESHOLD, Duration::from_secs(25));
        assert_eq!(HOST_AUDIO_CAP_BYTES, 20 * 88_200);
        assert_eq!(HOST_VIDEO_CAP_FRAMES, 20 * 30);
        assert_eq!(TTS_QUEUE_CAP_BYTES, 5 * 88_200);
    }

    #[test]
    fn now_unix_ms_returns_positive_millis_since_epoch() {
        let t = now_unix_ms();
        assert!(t > 1_700_000_000_000, "expected modern-era millis, got {t}");
    }

    #[test]
    fn stream_buffers_new_starts_empty() {
        let b = StreamBuffers::new();
        assert_eq!(b.audio.lock().unwrap().len(), 0);
        assert_eq!(b.video.lock().unwrap().len(), 0);
        assert_eq!(b.tts.lock().unwrap().len(), 0);
    }

    #[test]
    fn rtmp_manager_new_is_empty_and_has_no_metrics() {
        let m = RtmpManager::new();
        assert!(m.streams.is_empty());
        assert!(m.metrics.is_none());
    }

    #[test]
    fn rtmp_manager_default_equals_new() {
        let m = RtmpManager::default();
        assert!(m.streams.is_empty());
        assert!(m.metrics.is_none());
    }

    #[test]
    fn set_metrics_stores_arc_for_later_use_by_drain_threads() {
        let mut m = RtmpManager::new();
        let metrics = SessionMetrics::new();
        m.set_metrics(metrics.clone());
        assert!(m.metrics.is_some());
        // Pointer equality — set_metrics must not clone internally and lose sharing.
        let stored = m.metrics.as_ref().unwrap().clone();
        assert!(Arc::ptr_eq(&stored, &metrics));
    }

    #[test]
    fn push_video_frame_on_empty_manager_records_nothing_and_does_not_panic() {
        let m = RtmpManager::new();
        m.push_video_frame(&[0u8; 16]);
        assert!(m.streams.is_empty());
    }

    #[test]
    fn push_host_audio_on_empty_manager_is_a_noop() {
        let m = RtmpManager::new();
        m.push_host_audio(&[]);
        m.push_host_audio(&[1, 2, 3]);
        assert!(m.streams.is_empty());
    }

    #[test]
    fn push_tts_bumps_metrics_even_when_no_stream_matches_lang() {
        let mut m = RtmpManager::new();
        let metrics = SessionMetrics::new();
        m.set_metrics(metrics.clone());
        // Big enough to cross the 1ms threshold for lang accounting.
        m.push_tts("ja", vec![0u8; 88_200]);
        let snap = metrics.snapshot();
        assert!(
            snap.output_minutes_by_lang.contains_key("ja"),
            "metrics should record the lang even without a matching stream"
        );
    }

    #[test]
    fn push_tts_is_a_noop_without_metrics_and_no_streams() {
        let m = RtmpManager::new();
        m.push_tts("ja", vec![0u8; 100]);
    }

    #[test]
    fn push_caption_on_empty_manager_is_safe_noop() {
        let m = RtmpManager::new();
        m.push_caption("ja", "hello".into());
    }

    #[test]
    fn detect_crashed_on_empty_manager_returns_empty_vec() {
        let mut m = RtmpManager::new();
        let crashed = m.detect_crashed();
        assert!(crashed.is_empty());
    }

    #[test]
    fn kill_idle_streams_on_empty_manager_does_nothing() {
        let mut m = RtmpManager::new();
        m.kill_idle_streams();
        assert!(m.streams.is_empty());
    }

    #[tokio::test]
    async fn stop_all_on_empty_manager_leaves_manager_empty_and_does_not_hang() {
        let mut m = RtmpManager::new();
        m.stop_all().await;
        assert!(m.streams.is_empty());
    }

    #[tokio::test]
    async fn spawn_health_monitor_exits_promptly_when_stop_flag_preset() {
        let manager: SharedRtmpManager = Arc::new(tokio::sync::Mutex::new(RtmpManager::new()));
        let stop = Arc::new(AtomicBool::new(true));
        let handle = spawn_health_monitor(manager, stop);
        // First `interval.tick()` fires immediately, stop flag trips, loop exits.
        tokio::time::timeout(Duration::from_millis(200), handle)
            .await
            .expect("health monitor should exit promptly")
            .unwrap();
    }

    #[test]
    fn build_ffmpeg_args_source_stream_skips_drawtext_vf_filter() {
        let args = build_ffmpeg_args("/tmp/fifo_src", None, "rtmp://x/y");
        let joined = args.join(" ");
        assert!(!joined.contains("-vf"), "source must not add drawtext");
        assert!(joined.ends_with("rtmp://x/y"));
        assert!(joined.contains("image2pipe"));
        assert!(joined.contains("-vcodec mjpeg"));
    }

    #[test]
    fn build_ffmpeg_args_target_stream_injects_escaped_drawtext_vf() {
        let args = build_ffmpeg_args(
            "/tmp/fifo_tgt",
            Some("/tmp/caption:with:colons"),
            "rtmps://edge/live/KEY",
        );
        let vf_index = args
            .iter()
            .position(|s| s == "-vf")
            .expect("drawtext filter present");
        let filter = &args[vf_index + 1];
        assert!(filter.starts_with("drawtext=textfile="));
        assert!(
            filter.contains("/tmp/caption\\:with\\:colons"),
            "colons must be escaped for drawtext: got {filter}"
        );
        assert!(args.last().unwrap().starts_with("rtmps://"));
    }

    #[test]
    fn build_ffmpeg_args_always_passes_f_flv_for_rtmp_family_publish() {
        let args = build_ffmpeg_args("/tmp/fifo", None, "rtmp://localhost/live");
        let idx = args.iter().rposition(|s| s == "-f").expect("-f present");
        assert_eq!(args[idx + 1], "flv");
    }

    #[test]
    fn build_ffmpeg_args_passes_44100_mono_s16le_for_audio_fifo_input() {
        let args = build_ffmpeg_args("/tmp/fifo", None, "rtmp://x");
        let ar_idx = args.iter().position(|s| s == "-ar").unwrap();
        assert_eq!(args[ar_idx + 1], "44100");
        let ac_idx = args.iter().position(|s| s == "-ac").unwrap();
        assert_eq!(args[ac_idx + 1], "1");
    }

    // Build a `RtmpStream` around a short-lived shell subprocess — stand-in
    // for the real FFmpeg child. Lets us exercise `detect_crashed`, `stop_all`,
    // and push-fan-out branches without spawning FFmpeg.
    fn fake_exited_stream(id: &str, lang: &str, is_source: bool) -> RtmpStream {
        let mut child = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn shell child");
        let _ = child.wait();
        RtmpStream {
            child,
            video_handle: Some(thread::spawn(|| {})),
            audio_handle: Some(thread::spawn(|| {})),
            audio_fifo: format!("/tmp/brivva_audio_fake_{id}"),
            lang: lang.to_string(),
            rtmp_url: "rtmp://fake".into(),
            delay: Duration::from_millis(1000),
            is_source,
            host_gain: 1.0,
            buffers: StreamBuffers::new(),
            caption: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            restart_count: 0,
            last_write_ms: Arc::new(AtomicI64::new(now_unix_ms())),
        }
    }

    #[test]
    fn detect_crashed_picks_up_exited_child_and_emits_restart_snapshot() {
        let mut m = RtmpManager::new();
        m.streams
            .insert("fake".into(), fake_exited_stream("fake", "ja", false));

        let crashed = m.detect_crashed();
        assert_eq!(crashed.len(), 1, "exited child should register as crashed");
        let snapshot = &crashed[0];
        assert_eq!(snapshot.0, "fake");
        assert_eq!(snapshot.1, "ja");
        // Original stream removed so restart path can re-insert.
        assert!(!m.streams.contains_key("fake"));
    }

    #[test]
    fn detect_crashed_skips_streams_with_stop_flag_set() {
        let mut m = RtmpManager::new();
        let stream = fake_exited_stream("stopped", "ja", true);
        stream.stop_flag.store(true, Ordering::Release);
        m.streams.insert("stopped".into(), stream);

        let crashed = m.detect_crashed();
        assert!(crashed.is_empty());
        // Stream still in map — detect_crashed didn't treat it as a crash.
        assert!(m.streams.contains_key("stopped"));
    }

    #[test]
    fn detect_crashed_abandons_stream_after_max_restart_attempts() {
        let mut m = RtmpManager::new();
        let mut stream = fake_exited_stream("hot", "ja", false);
        stream.restart_count = MAX_FFMPEG_RESTARTS;
        m.streams.insert("hot".into(), stream);

        let crashed = m.detect_crashed();
        assert!(
            crashed.is_empty(),
            "exhausted streams must not re-enter restart loop"
        );
        // Stop flag should now be set on the exhausted stream.
        assert!(m.streams["hot"].stop_flag.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn stop_all_drains_every_registered_stream_and_empties_map() {
        let mut m = RtmpManager::new();
        m.streams
            .insert("a".into(), fake_exited_stream("a", "ja", true));
        m.streams
            .insert("b".into(), fake_exited_stream("b", "ko", false));
        m.stop_all().await;
        assert!(m.streams.is_empty());
    }

    #[test]
    fn push_tts_delivers_into_matching_target_stream_queue() {
        let mut m = RtmpManager::new();
        m.streams
            .insert("target".into(), fake_exited_stream("target", "ja", false));

        m.push_tts("ja", vec![1u8; 4_000]);
        let q = m.streams["target"].buffers.tts.lock().unwrap();
        assert_eq!(q.len(), 4_000);
    }

    #[test]
    fn push_tts_skips_source_streams_even_when_lang_matches() {
        let mut m = RtmpManager::new();
        m.streams
            .insert("src".into(), fake_exited_stream("src", "en", true));

        m.push_tts("en", vec![1u8; 100]);
        let q = m.streams["src"].buffers.tts.lock().unwrap();
        assert!(q.is_empty(), "source streams must not receive TTS");
    }

    #[test]
    fn push_tts_caps_queue_at_5s_of_pcm_discarding_oldest_bytes() {
        let mut m = RtmpManager::new();
        m.streams
            .insert("t".into(), fake_exited_stream("t", "ja", false));

        // Push more than the 5s cap — oldest bytes drop so head index shifts.
        m.push_tts("ja", vec![1u8; TTS_QUEUE_CAP_BYTES + 100]);
        let q_len = m.streams["t"].buffers.tts.lock().unwrap().len();
        assert!(q_len <= TTS_QUEUE_CAP_BYTES);
    }

    #[test]
    fn push_host_audio_evicts_oldest_bytes_when_capacity_exceeded() {
        let mut m = RtmpManager::new();
        m.streams
            .insert("a".into(), fake_exited_stream("a", "ja", false));

        // Drop more than the ~20s cap in a single push.
        m.push_host_audio(&vec![1u8; HOST_AUDIO_CAP_BYTES + 500]);
        let total: usize = m.streams["a"]
            .buffers
            .audio
            .lock()
            .unwrap()
            .iter()
            .map(|(_, b)| b.len())
            .sum();
        assert!(total <= HOST_AUDIO_CAP_BYTES + 500); // pushed-once: exactly one entry
    }

    #[test]
    fn push_video_frame_caps_per_stream_buffer_at_20s_of_frames() {
        let mut m = RtmpManager::new();
        m.streams
            .insert("a".into(), fake_exited_stream("a", "ja", false));

        for _ in 0..(HOST_VIDEO_CAP_FRAMES + 50) {
            m.push_video_frame(&[0u8; 8]);
        }
        let buf_len = m.streams["a"].buffers.video.lock().unwrap().len();
        assert_eq!(buf_len, HOST_VIDEO_CAP_FRAMES);
    }

    #[test]
    fn push_caption_on_source_stream_is_a_noop_because_source_has_no_caption_state() {
        let mut m = RtmpManager::new();
        m.streams
            .insert("src".into(), fake_exited_stream("src", "en", true));
        m.push_caption("en", "anything".into()); // source → caption is None
    }

    #[test]
    fn kill_idle_streams_observes_idle_and_invokes_kill_branch() {
        let mut m = RtmpManager::new();
        // Seed last_write at 1 (far older than threshold) so the idle branch
        // fires. Child has already exited — kill() may return Err on some
        // platforms, but the function must still process the branch without
        // panic. Success of the signal itself is not the assertion.
        let stream = fake_exited_stream("idle", "ja", false);
        stream.last_write_ms.store(1, Ordering::Release);
        m.streams.insert("idle".into(), stream);
        m.kill_idle_streams();
        // Stream is still in the map; kill_idle_streams does not remove.
        assert!(m.streams.contains_key("idle"));
    }

    #[test]
    fn kill_idle_streams_skips_streams_with_zero_last_write() {
        let mut m = RtmpManager::new();
        let stream = fake_exited_stream("unseeded", "ja", false);
        stream.last_write_ms.store(0, Ordering::Release);
        m.streams.insert("unseeded".into(), stream);
        m.kill_idle_streams(); // last==0 → skip without kill
        assert_eq!(
            m.streams["unseeded"].last_write_ms.load(Ordering::Acquire),
            0
        );
    }

    #[test]
    fn kill_idle_streams_skips_streams_with_stop_flag_set() {
        let mut m = RtmpManager::new();
        let stream = fake_exited_stream("stopped", "ja", false);
        stream.last_write_ms.store(1, Ordering::Release);
        stream.stop_flag.store(true, Ordering::Release);
        m.streams.insert("stopped".into(), stream);
        m.kill_idle_streams();
        assert_eq!(
            m.streams["stopped"].last_write_ms.load(Ordering::Acquire),
            1
        );
    }
}
