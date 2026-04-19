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
mod mixer;
mod orphan;

pub use orphan::{decode_mp3_to_pcm, kill_orphan_ffmpeg};

use caption::CaptionState;
use mixer::{apply_gain, mix_pcm_s16le};

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};

// ── Timing + format constants ─────────────────────────────

/// Video: 33.33 ms per frame at 30 fps.
const FRAME_INTERVAL: Duration = Duration::from_nanos(33_333_333);
/// Audio: 20 ms per tick.
const AUDIO_TICK: Duration = Duration::from_millis(20);
/// Audio bytes per 20 ms at 44.1 kHz mono s16le: 1764.
const AUDIO_BYTES_PER_TICK: usize = 1764;
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
}

/// Per-stream burn-in caption state. Source streams don't have one (nothing
/// to burn in — they're the host's own audio). Target streams get a textfile
/// that drawtext reads with `reload=1`; writes are rate-limited via a tokio
/// task so a fresh translation always gets at least MIN_CAPTION_DWELL_MS on
/// screen before being replaced.
pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
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

struct AudioDrainCtx {
    stream_id: String,
    host_buf: Arc<StdMutex<VecDeque<TimedChunk>>>,
    tts_queue: Arc<StdMutex<VecDeque<u8>>>,
    fifo_path: String,
    delay: Duration,
    is_source: bool,
    host_gain: f32,
    stop: Arc<AtomicBool>,
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
        }
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

    /// Scan for crashed FFmpeg processes and surface the restart context.
    pub(crate) fn detect_crashed(
        &mut self,
    ) -> Vec<CrashedStreamSnapshot> {
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

        // Compose FFmpeg args. Only target streams apply the drawtext filter.
        let mut ffmpeg_args: Vec<String> = vec![
            "-y".into(),
            "-loglevel".into(),
            "warning".into(),
            "-f".into(),
            "image2pipe".into(),
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
            audio_fifo.clone(),
        ];
        if let Some(cap) = &caption {
            // Escape the textfile path for drawtext — it uses `\` as an escape
            // and `:` as a filter-option separator.
            let escaped = cap.path.replace('\\', "\\\\").replace(':', "\\:");
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
            args.rtmp_url.clone(),
        ]);

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

        // Video drain
        let v_buf = buffers.video.clone();
        let v_stop = stop_flag.clone();
        let v_sid = args.stream_id.clone();
        let video_handle = thread::Builder::new()
            .name(format!("video-drain-{}", args.stream_id))
            .spawn(move || video_drain_loop(v_sid, v_buf, stdin, delay, v_stop))
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

// ── Video drain ────────────────────────────────────────────
//
// Emit at exactly 30 fps. Each tick: drain every video chunk that has aged
// past `delay`, keep the latest one, then write it (or repeat the previous
// frame if nothing is ready yet).

fn video_drain_loop(
    stream_id: String,
    video_buf: Arc<StdMutex<VecDeque<TimedChunk>>>,
    mut stdin: std::process::ChildStdin,
    delay: Duration,
    stop: Arc<AtomicBool>,
) {
    let mut last_frame: Option<Vec<u8>> = None;
    let mut tick_count: u64 = 0;
    let mut next_tick = Instant::now() + FRAME_INTERVAL;

    eprintln!(
        "[VIDEO:{}] drain started (30 fps, delay={}ms)",
        stream_id,
        delay.as_millis()
    );

    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }

        let now = Instant::now();
        if next_tick > now {
            thread::sleep(next_tick - now);
        }
        let actual = Instant::now();
        next_tick += FRAME_INTERVAL;
        tick_count += 1;

        // Drain any chunk whose delay has elapsed; keep the newest one.
        let ready = {
            let mut buf = video_buf.lock().unwrap();
            let mut latest: Option<Vec<u8>> = None;
            while let Some((ts, _)) = buf.front() {
                if *ts + delay <= actual {
                    let (_, f) = buf.pop_front().unwrap();
                    latest = Some(f);
                } else {
                    break;
                }
            }
            latest
        };

        let to_write = match ready {
            Some(f) => {
                last_frame = Some(f.clone());
                Some(f)
            }
            None => last_frame.clone(),
        };

        if let Some(f) = to_write && stdin.write_all(&f).is_err() {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[VIDEO:{}] write error, exiting", stream_id);
            }
            break;
        }
        // else: startup, no frames yet — skip writing this tick.
    }

    drop(stdin);
    eprintln!(
        "[VIDEO:{}] drain exited after {} ticks",
        stream_id, tick_count
    );
}

// ── Audio drain + mixer ────────────────────────────────────
//
// Emit PCM s16le @ 44.1 kHz mono at 20 ms ticks. Each tick:
// 1. Ingest any delayed host-audio chunks into `ready_host`.
// 2. Take up to 1764 bytes host + 1764 bytes TTS (pad with silence).
// 3. `output = clip(host × host_gain + tts × 1.0)`. Source streams use
//    host_gain=1.0 and never receive TTS, so the mix collapses to the
//    host passthrough.
// 4. Write to the FFmpeg FIFO.

fn audio_drain_loop(ctx: AudioDrainCtx) {
    let AudioDrainCtx {
        stream_id,
        host_buf,
        tts_queue,
        fifo_path,
        delay,
        is_source,
        host_gain,
        stop,
    } = ctx;

    let mut fifo = match std::fs::OpenOptions::new().write(true).open(&fifo_path) {
        Ok(f) => f,
        Err(e) => {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[AUDIO:{}] failed to open FIFO: {}", stream_id, e);
            }
            return;
        }
    };

    let silence_chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
    let mut ready_host: Vec<u8> = Vec::with_capacity(AUDIO_BYTES_PER_TICK * 4);
    let mut tick_count: u64 = 0;
    let mut next_tick = Instant::now() + AUDIO_TICK;

    eprintln!(
        "[AUDIO:{}] drain started (20 ms, delay={}ms, source={}, host_gain={:.2})",
        stream_id,
        delay.as_millis(),
        is_source,
        host_gain
    );

    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }

        let now = Instant::now();
        if next_tick > now {
            thread::sleep(next_tick - now);
        }
        let actual = Instant::now();
        next_tick += AUDIO_TICK;
        tick_count += 1;

        // Ingest any host chunks whose delay has elapsed.
        {
            let mut buf = host_buf.lock().unwrap();
            while let Some((ts, _)) = buf.front() {
                if *ts + delay <= actual {
                    let (_, pcm) = buf.pop_front().unwrap();
                    ready_host.extend_from_slice(&pcm);
                } else {
                    break;
                }
            }
        }

        // Take this tick's host PCM, pad with silence if short.
        let host_take = AUDIO_BYTES_PER_TICK.min(ready_host.len());
        let mut host_chunk: Vec<u8> = ready_host.drain(..host_take).collect();
        if host_chunk.len() < AUDIO_BYTES_PER_TICK {
            host_chunk.resize(AUDIO_BYTES_PER_TICK, 0);
        }

        let output = if is_source {
            // Source streams never queue TTS — skip the mix entirely.
            // host_gain usually 1.0 here; if a user picked something lower
            // the stream will simply sound quieter, which is fine.
            if (host_gain - 1.0).abs() < f32::EPSILON {
                host_chunk
            } else {
                apply_gain(&host_chunk, host_gain)
            }
        } else {
            let tts_chunk: Vec<u8> = {
                let mut q = tts_queue.lock().unwrap();
                let n = AUDIO_BYTES_PER_TICK.min(q.len());
                q.drain(..n).collect()
            };
            let mut tts_padded = tts_chunk;
            if tts_padded.len() < AUDIO_BYTES_PER_TICK {
                tts_padded.resize(AUDIO_BYTES_PER_TICK, 0);
            }
            mix_pcm_s16le(&host_chunk, host_gain, &tts_padded, 1.0)
        };

        let write_result = if output.is_empty() {
            fifo.write_all(&silence_chunk)
        } else {
            fifo.write_all(&output)
        };
        if write_result.is_err() {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[AUDIO:{}] write error, exiting", stream_id);
            }
            break;
        }
    }

    drop(fifo);
    eprintln!(
        "[AUDIO:{}] drain exited after {} ticks",
        stream_id, tick_count
    );
}

