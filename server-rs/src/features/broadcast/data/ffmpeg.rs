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

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

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
/// Minimum time a burn-in caption stays on screen before we overwrite it.
/// Prevents rapid-fire translations from flashing through unreadably.
const MIN_CAPTION_DWELL_MS: u64 = 1_500;

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
struct CaptionState {
    path: String,
    sender: tokio::sync::mpsc::UnboundedSender<String>,
    writer: Option<tokio::task::JoinHandle<()>>,
}

impl CaptionState {
    fn spawn(stream_id: &str) -> Self {
        let path = format!("/tmp/brivva_caption_{}.txt", stream_id);
        // Empty initial file so drawtext reads cleanly from the first frame.
        let _ = std::fs::write(&path, "");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let path_clone = path.clone();
        let writer = tokio::spawn(async move {
            let min_dwell = Duration::from_millis(MIN_CAPTION_DWELL_MS);
            let mut last_written = Instant::now()
                .checked_sub(min_dwell)
                .unwrap_or_else(Instant::now);
            while let Some(mut text) = rx.recv().await {
                // Wait out any remaining minimum-dwell budget for the previous caption.
                let elapsed = last_written.elapsed();
                if elapsed < min_dwell {
                    tokio::time::sleep(min_dwell - elapsed).await;
                }
                // Coalesce any updates that piled up during the sleep — only the
                // latest translation is worth showing.
                while let Ok(newer) = rx.try_recv() {
                    text = newer;
                }
                let sanitized = sanitize_caption(&text);
                write_caption_atomic(&path_clone, &sanitized);
                last_written = Instant::now();
            }
        });
        Self {
            path,
            sender: tx,
            writer: Some(writer),
        }
    }

    fn push(&self, text: &str) {
        let _ = self.sender.send(text.to_string());
    }
}

/// Remove control chars and cap caption length so drawtext stays legible.
fn sanitize_caption(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(200)
        .collect()
}

/// Write the caption atomically so FFmpeg's `reload=1` never sees a torn file.
fn write_caption_atomic(path: &str, text: &str) {
    let tmp = format!("{}.tmp", path);
    if let Err(e) = std::fs::write(&tmp, text) {
        eprintln!("[CAPTION] write tmp failed ({}): {}", path, e);
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        eprintln!("[CAPTION] rename failed ({}): {}", path, e);
    }
}

pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
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
        self.spawn_stream_inner(
            stream_id, lang, rtmp_url, delay_ms, is_source, host_gain, None,
        )?;
        eprintln!(
            "[FFMPEG] started stream={} lang={} → {} [delay={}ms, source={}, host_gain={:.2}]",
            stream_id, lang, rtmp_url, delay_ms, is_source, host_gain
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
            if stream.lang == lang && !stream.is_source {
                if let Some(cap) = &stream.caption {
                    cap.push(&text);
                }
            }
        }
    }

    /// Scan for crashed FFmpeg processes and surface the restart context.
    pub(crate) fn detect_crashed(
        &mut self,
    ) -> Vec<(String, String, String, u64, bool, f32, u32, StreamBuffers)> {
        let mut to_restart = Vec::new();

        for (id, stream) in &mut self.streams {
            match stream.child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(-1);
                    if stream.stop_flag.load(Ordering::Acquire) {
                        continue;
                    }
                    eprintln!(
                        "[FFMPEG] crashed lang={} exit={}, will restart",
                        stream.lang, code
                    );
                    if stream.restart_count >= MAX_FFMPEG_RESTARTS {
                        eprintln!(
                            "[FFMPEG] giving up on {} after {} attempts",
                            stream.lang, MAX_FFMPEG_RESTARTS
                        );
                        stream.stop_flag.store(true, Ordering::Release);
                        continue;
                    }
                    to_restart.push(id.clone());
                }
                Ok(None) => {}
                Err(e) => eprintln!("[FFMPEG] status check failed for {}: {}", id, e),
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
                    drop(cap.sender);
                    if let Some(handle) = cap.writer.take() {
                        handle.abort();
                    }
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
    pub(crate) fn restart_stream(
        &mut self,
        id: &str,
        lang: &str,
        rtmp_url: &str,
        delay_ms: u64,
        is_source: bool,
        host_gain: f32,
        prev_count: u32,
        buffers: StreamBuffers,
    ) {
        match self.spawn_stream_inner(
            id,
            lang,
            rtmp_url,
            delay_ms,
            is_source,
            host_gain,
            Some(buffers),
        ) {
            Ok(()) => {
                if let Some(stream) = self.streams.get_mut(id) {
                    stream.restart_count = prev_count + 1;
                }
                eprintln!(
                    "[FFMPEG] restarted {} ({}) attempt {}/{}",
                    id,
                    lang,
                    prev_count + 1,
                    MAX_FFMPEG_RESTARTS
                );
            }
            Err(e) => eprintln!("[FFMPEG] restart {} ({}) failed: {}", id, lang, e),
        }
    }

    fn spawn_stream_inner(
        &mut self,
        stream_id: &str,
        lang: &str,
        rtmp_url: &str,
        delay_ms: u64,
        is_source: bool,
        host_gain: f32,
        existing_buffers: Option<StreamBuffers>,
    ) -> Result<(), String> {
        let audio_fifo = format!("/tmp/brivva_audio_{}", stream_id);

        let _ = std::fs::remove_file(&audio_fifo);
        std::process::Command::new("mkfifo")
            .arg(&audio_fifo)
            .output()
            .map_err(|e| format!("mkfifo failed: {}", e))?;

        // Target streams get a burn-in caption textfile + drawtext filter.
        // Source streams skip both (no translation to display).
        let caption = if is_source {
            None
        } else {
            Some(CaptionState::spawn(stream_id))
        };

        // Compose FFmpeg args. Only target streams apply the drawtext filter.
        let mut args: Vec<String> = vec![
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
            args.extend_from_slice(&["-vf".into(), drawtext]);
        }
        args.extend_from_slice(&[
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
            rtmp_url.into(),
        ]);

        let mut child = std::process::Command::new("ffmpeg")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;

        let stdin = child.stdin.take().ok_or("No FFmpeg stdin")?;
        let buffers = existing_buffers.unwrap_or_else(StreamBuffers::new);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let delay = Duration::from_millis(delay_ms);

        // Video drain
        let v_buf = buffers.video.clone();
        let v_stop = stop_flag.clone();
        let v_sid = stream_id.to_string();
        let video_handle = thread::Builder::new()
            .name(format!("video-drain-{}", stream_id))
            .spawn(move || video_drain_loop(v_sid, v_buf, stdin, delay, v_stop))
            .map_err(|e| format!("Video thread spawn failed: {}", e))?;

        // Audio drain
        let a_buf = buffers.audio.clone();
        let a_tts = buffers.tts.clone();
        let a_stop = stop_flag.clone();
        let a_sid = stream_id.to_string();
        let a_fifo = audio_fifo.clone();
        let audio_handle = thread::Builder::new()
            .name(format!("audio-drain-{}", stream_id))
            .spawn(move || {
                audio_drain_loop(
                    a_sid, a_buf, a_tts, a_fifo, delay, is_source, host_gain, a_stop,
                )
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
                delay,
                is_source,
                host_gain,
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
                    eprintln!("[FFMPEG:{}] killed", id);
                }
                Err(e) => eprintln!("[FFMPEG:{}] kill error: {}", id, e),
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
                // Dropping the sender closes the channel so the writer task exits.
                drop(cap.sender);
                if let Some(handle) = cap.writer.take() {
                    handle.abort();
                }
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
                mgr.restart_stream(
                    &id, &lang, &rtmp_url, delay_ms, is_source, host_gain, prev_count, buffers,
                );
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

        if let Some(f) = to_write {
            if stdin.write_all(&f).is_err() {
                if !stop.load(Ordering::Acquire) {
                    eprintln!("[VIDEO:{}] write error, exiting", stream_id);
                }
                break;
            }
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

fn audio_drain_loop(
    stream_id: String,
    host_buf: Arc<StdMutex<VecDeque<TimedChunk>>>,
    tts_queue: Arc<StdMutex<VecDeque<u8>>>,
    fifo_path: String,
    delay: Duration,
    is_source: bool,
    host_gain: f32,
    stop: Arc<AtomicBool>,
) {
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

/// Apply a uniform gain to a PCM s16le buffer and clip to the i16 range.
fn apply_gain(pcm: &[u8], gain: f32) -> Vec<u8> {
    let n_aligned = pcm.len() - (pcm.len() % 2);
    let mut out = Vec::with_capacity(n_aligned);
    let mut i = 0;
    while i + 1 < n_aligned {
        let s = i16::from_le_bytes([pcm[i], pcm[i + 1]]) as f32;
        let scaled = (s * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        out.extend_from_slice(&scaled.to_le_bytes());
        i += 2;
    }
    out
}

/// Mix two PCM streams (s16le little-endian, same length) with per-source gain
/// and clip to the i16 range. Output length = min(a.len(), b.len()).
fn mix_pcm_s16le(a: &[u8], a_gain: f32, b: &[u8], b_gain: f32) -> Vec<u8> {
    let n = a.len().min(b.len());
    let n_aligned = n - (n % 2);
    let mut out = Vec::with_capacity(n_aligned);
    let mut i = 0;
    while i + 1 < n_aligned {
        let sa = i16::from_le_bytes([a[i], a[i + 1]]) as f32;
        let sb = i16::from_le_bytes([b[i], b[i + 1]]) as f32;
        let mixed = (sa * a_gain + sb * b_gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        out.extend_from_slice(&mixed.to_le_bytes());
        i += 2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    fn decode_samples(bytes: &[u8]) -> Vec<i16> {
        bytes
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect()
    }

    #[test]
    fn sanitize_caption_strips_control_chars_and_truncates() {
        let text = format!("hi\x00there\n{}", "a".repeat(250));
        let out = sanitize_caption(&text);

        assert!(!out.contains('\x00'));
        assert!(out.contains('\n'));
        assert_eq!(out.chars().count(), 200);
    }

    #[test]
    fn apply_gain_scales_samples_and_ignores_trailing_odd_byte() {
        let scaled = apply_gain(&[0x10, 0x27, 0xF0, 0xD8, 0xAA], 0.5);
        assert_eq!(decode_samples(&scaled), vec![5000, -5000]);
    }

    #[test]
    fn mix_pcm_s16le_clips_on_overflow() {
        let mixed = mix_pcm_s16le(&pcm(&[30_000, -30_000]), 1.0, &pcm(&[10_000, -10_000]), 1.0);
        assert_eq!(decode_samples(&mixed), vec![32_767, -32_768]);
    }
}

// ── Startup cleanup ────────────────────────────────────────

pub fn kill_orphan_ffmpeg() {
    let output = match std::process::Command::new("pgrep")
        .args(["-f", "brivva_audio"])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!(
                "[STARTUP] pgrep not available, skipping orphan cleanup: {}",
                e
            );
            return;
        }
    };

    let pids = String::from_utf8_lossy(&output.stdout);
    let mut killed = 0;
    for line in pids.lines() {
        if let Ok(pid) = line.trim().parse::<i32>() {
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

    if let Ok(entries) = std::fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("brivva_audio_") || name.starts_with("brivva_caption_") {
                    let _ = std::fs::remove_file(entry.path());
                    eprintln!("[STARTUP] removed stale file: {}", name);
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

/// Decode MP3 bytes to raw PCM s16le 44.1 kHz mono via FFmpeg subprocess.
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
