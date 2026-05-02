//! Per-stream video + audio drain loops.
//!
//! These run on their own OS threads. They pull aged chunks out of each
//! stream's delay buffers and feed them to FFmpeg: depacketized WebRTC H.264
//! Annex-B bytes over stdin, audio to a FIFO at 20 ms ticks mixed with
//! translated TTS.

use std::collections::VecDeque;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

use super::mixer::{apply_gain, mix_pcm_s16le};
use crate::features::broadcast::domain::SessionMetrics;

/// Audio: 20 ms per tick.
const AUDIO_TICK: Duration = Duration::from_millis(20);
/// Audio: 1764 bytes = 44.1 kHz × 20 ms × s16le mono.
const AUDIO_BYTES_PER_TICK: usize = 1764;
/// Video is allowed to queue briefly and catch up by writing faster than wall
/// clock. Past this window the stream is too stale for live commerce, so old
/// chunks are dropped and the drain resumes at an IDR/keyframe.
const DEFAULT_VIDEO_MAX_LAG: Duration = Duration::from_secs(3);
const MAX_VIDEO_MAX_LAG: Duration = Duration::from_secs(5);
const DEFAULT_AUDIO_MAX_LAG: Duration = Duration::from_secs(3);
const MAX_AUDIO_MAX_LAG: Duration = Duration::from_secs(5);
/// Ready-host audio can normally exceed one 20ms tick because browsers send
/// microphone PCM in larger chunks (ScriptProcessor 4096 frames ≈93ms). Keep a
/// multi-second catch-up window before forced host-audio drops.
#[cfg(test)]
const DEFAULT_AUDIO_MAX_READY_TICKS: usize = 150;
/// Audio drain must never wait longer than one output tick on a full FFmpeg
/// FIFO. If the muxer is blocked, skip the current 20 ms slice and let the
/// next tick continue on wall clock instead of accumulating seconds of stale
/// audio that later replays late.
const AUDIO_FIFO_WRITE_BUDGET: Duration = Duration::from_millis(18);

pub(super) type TimedChunk = (Instant, Arc<[u8]>);

// ── Video ─────────────────────────────────────────────────

pub(super) struct VideoDrainCtx {
    pub stream_id: String,
    pub h264_buf: Arc<StdMutex<VecDeque<TimedChunk>>>,
    pub video_stdin: std::process::ChildStdin,
    pub delay: Duration,
    pub stop: Arc<AtomicBool>,
    /// Unix-ms wall clock updated after each successful write. Health monitor
    /// reads this to detect silent FFmpeg stalls on long live shows.
    pub last_write_ms: Arc<AtomicI64>,
    pub metrics: Option<Arc<SessionMetrics>>,
}

pub(super) fn video_drain_loop(ctx: VideoDrainCtx) {
    let VideoDrainCtx {
        stream_id,
        h264_buf,
        mut video_stdin,
        delay,
        stop,
        last_write_ms,
        metrics,
    } = ctx;
    let mut chunk_count: u64 = 0;
    let mut last_stats = Instant::now();
    let mut need_keyframe = true;
    let max_lag = video_max_lag_from_env();
    let mut stale_chunks_dropped: u64 = 0;
    let mut keyframe_wait_chunks_dropped: u64 = 0;

    eprintln!(
        "[VIDEO:{}] H.264 pipe drain started (delay={}ms)",
        stream_id,
        delay.as_millis()
    );

    while !stop.load(Ordering::Acquire) {
        let now = Instant::now();
        if now.duration_since(last_stats) >= Duration::from_secs(1) {
            let buffered_chunks = h264_buf.lock().unwrap().len();
            eprintln!(
                "[VIDEO:{}] stats chunks_written={} buffered_chunks={} video_stale_chunks_dropped={} video_keyframe_wait_chunks_dropped={}",
                stream_id,
                chunk_count,
                buffered_chunks,
                stale_chunks_dropped,
                keyframe_wait_chunks_dropped
            );
            last_stats = now;
        }

        let Some(drain) = drain_next_h264_live(&h264_buf, now, delay, max_lag, &mut need_keyframe)
        else {
            thread::sleep(Duration::from_millis(2));
            continue;
        };
        if drain.dropped_stale > 0 || drain.dropped_for_keyframe > 0 {
            stale_chunks_dropped += drain.dropped_stale as u64;
            keyframe_wait_chunks_dropped += drain.dropped_for_keyframe as u64;
            eprintln!(
                "[VIDEO:{}] video_stale_chunks_dropped={} video_keyframe_wait_chunks_dropped={} max_lag_ms={}",
                stream_id,
                drain.dropped_stale,
                drain.dropped_for_keyframe,
                max_lag.as_millis()
            );
        }
        let chunk = drain.chunk;
        let n = chunk.len() as u64;
        let write_start = Instant::now();
        if video_stdin.write_all(&chunk).is_err() {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[VIDEO:{}] H.264 pipe write error, exiting", stream_id);
            }
            return;
        }
        let write_elapsed = write_start.elapsed();
        if write_elapsed > Duration::from_millis(20) {
            eprintln!(
                "[VIDEO:{}] pipe write slow elapsed_ms={}",
                stream_id,
                write_elapsed.as_millis()
            );
        }
        chunk_count += 1;
        last_write_ms.store(now_unix_ms(), Ordering::Release);
        if let Some(m) = &metrics {
            m.record_bytes_out(n);
        }
    }

    let _ = video_stdin.flush();
    eprintln!(
        "[VIDEO:{}] H.264 pipe drain exited after {} chunks",
        stream_id, chunk_count
    );
}

struct VideoDrainNext {
    chunk: Arc<[u8]>,
    dropped_stale: usize,
    dropped_for_keyframe: usize,
}

fn drain_next_h264_live(
    h264_buf: &StdMutex<VecDeque<TimedChunk>>,
    now: Instant,
    delay: Duration,
    max_lag: Duration,
    need_keyframe: &mut bool,
) -> Option<VideoDrainNext> {
    let mut buf = h264_buf.lock().unwrap();
    let live_cutoff = now.checked_sub(max_lag).unwrap_or(now);

    // If FFmpeg/RTMP stalled, the buffer may contain seconds of already-due
    // video. Emitting all of it makes viewers see fast-forward video until the
    // pipe catches up. For live streaming, stale video is worse than dropped
    // video: drop old chunks until the next emit is close to wall clock.
    let mut dropped_stale = 0;
    while buf.len() > 1 {
        let Some((ts, _)) = buf.front() else { break };
        let target_ts = *ts + delay;
        if target_ts >= live_cutoff {
            break;
        }
        buf.pop_front();
        dropped_stale += 1;
    }

    // After a stale-drop event, resuming in the middle of an H.264 GOP can
    // feed FFmpeg P/B slices whose SPS/PPS/IDR reference frame was just
    // discarded. When possible, skip forward to the next due IDR/keyframe so
    // the decoder restarts cleanly instead of logging non-existing PPS errors
    // and producing corrupted frames. If no IDR is available yet, keep the last
    // chunk rather than emptying the live buffer; the next keyframe will be
    // preferred as soon as it arrives.
    let mut dropped_for_keyframe = 0;
    if dropped_stale > 0 {
        *need_keyframe = true;
        while buf.len() > 1 {
            let Some((ts, packet)) = buf.front() else {
                break;
            };
            if *ts + delay > now || h264_annexb_contains_idr(packet) {
                break;
            }
            buf.pop_front();
            dropped_for_keyframe += 1;
        }
    }

    while *need_keyframe && buf.len() > 1 {
        let Some((ts, packet)) = buf.front() else {
            break;
        };
        if *ts + delay > now || h264_annexb_contains_idr(packet) {
            break;
        }
        buf.pop_front();
        dropped_for_keyframe += 1;
    }

    let (ts, _) = buf.front()?;
    if *ts + delay <= now {
        let (_, packet) = buf.pop_front().unwrap();
        if *need_keyframe {
            if !h264_annexb_contains_idr(&packet) {
                return None;
            }
            *need_keyframe = false;
        }
        Some(VideoDrainNext {
            chunk: packet,
            dropped_stale,
            dropped_for_keyframe,
        })
    } else {
        None
    }
}

fn video_max_lag_from_env() -> Duration {
    let millis = std::env::var("BRIVVA_VIDEO_MAX_LAG_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_VIDEO_MAX_LAG.as_millis() as u64)
        .clamp(250, MAX_VIDEO_MAX_LAG.as_millis() as u64);
    Duration::from_millis(millis)
}

fn audio_max_lag_from_env() -> Duration {
    let millis = std::env::var("BRIVVA_AUDIO_MAX_LAG_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_AUDIO_MAX_LAG.as_millis() as u64)
        .clamp(250, MAX_AUDIO_MAX_LAG.as_millis() as u64);
    Duration::from_millis(millis)
}

fn audio_max_ready_ticks(max_lag: Duration) -> usize {
    (max_lag.as_millis() / AUDIO_TICK.as_millis()).max(1) as usize
}

fn h264_annexb_contains_idr(packet: &[u8]) -> bool {
    let mut i = 0;
    while i + 3 < packet.len() {
        let start_len = if packet[i..].starts_with(&[0, 0, 1]) {
            3
        } else if packet[i..].starts_with(&[0, 0, 0, 1]) {
            4
        } else {
            i += 1;
            continue;
        };
        let nal_idx = i + start_len;
        if nal_idx < packet.len() && packet[nal_idx] & 0x1f == 5 {
            return true;
        }
        i = nal_idx.saturating_add(1);
    }
    false
}

// ── Audio ─────────────────────────────────────────────────

pub(super) struct AudioDrainCtx {
    pub stream_id: String,
    pub host_buf: Arc<StdMutex<VecDeque<TimedChunk>>>,
    pub tts_queue: Arc<StdMutex<VecDeque<u8>>>,
    pub fifo_path: String,
    pub delay: Duration,
    pub is_source: bool,
    pub host_gain: f32,
    pub stop: Arc<AtomicBool>,
    /// Unix-ms wall clock updated after each successful FIFO write.
    pub last_write_ms: Arc<AtomicI64>,
    pub metrics: Option<Arc<SessionMetrics>>,
}

pub(super) fn audio_drain_loop(ctx: AudioDrainCtx) {
    let AudioDrainCtx {
        stream_id,
        host_buf,
        tts_queue,
        fifo_path,
        delay,
        is_source,
        host_gain,
        stop,
        last_write_ms,
        metrics,
    } = ctx;

    let mut fifo = match open_audio_fifo(&fifo_path, &stream_id, &stop) {
        Some(f) => f,
        None => return,
    };

    let silence_chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
    let mut ready_host: Vec<u8> = Vec::with_capacity(AUDIO_BYTES_PER_TICK * 4);
    let mut tick_count: u64 = 0;
    let mut fifo_would_blocks: u64 = 0;
    let mut fifo_skipped_ticks: u64 = 0;
    let mut host_stale_chunks_dropped: u64 = 0;
    let mut ready_host_bytes_dropped: u64 = 0;
    let mut last_stats = Instant::now();
    let mut next_tick = Instant::now() + AUDIO_TICK;
    let max_lag = audio_max_lag_from_env();
    let max_ready_ticks = audio_max_ready_ticks(max_lag);

    eprintln!(
        "[AUDIO:{}] drain started (20 ms, delay={}ms, source={}, host_gain={:.2})",
        stream_id,
        delay.as_millis(),
        is_source,
        host_gain
    );

    while !stop.load(Ordering::Acquire) {
        let actual = wait_audio_tick(&mut next_tick);
        tick_count += 1;
        if actual.duration_since(last_stats) >= Duration::from_secs(1) {
            let host_buffered_chunks = host_buf.lock().unwrap().len();
            let tts_buffered_bytes = tts_queue.lock().unwrap().len();
            eprintln!(
                "[AUDIO:{}] stats ticks={} host_buffered_chunks={} ready_host_bytes={} tts_buffered_bytes={} fifo_would_blocks={} fifo_skipped_ticks={} host_audio_stale_chunks_dropped={} ready_host_bytes_dropped={}",
                stream_id,
                tick_count,
                host_buffered_chunks,
                ready_host.len(),
                tts_buffered_bytes,
                fifo_would_blocks,
                fifo_skipped_ticks,
                host_stale_chunks_dropped,
                ready_host_bytes_dropped
            );
            last_stats = actual;
        }

        let host_dropped = drain_aged_host_audio(DrainHostArgs {
            host_buf: &host_buf,
            ready_host: &mut ready_host,
            now: actual,
            delay,
            max_lag,
        });
        if host_dropped > 0 {
            host_stale_chunks_dropped += host_dropped as u64;
            eprintln!(
                "[AUDIO:{}] host_audio_stale_chunks_dropped={} max_lag_ms={}",
                stream_id,
                host_dropped,
                max_lag.as_millis()
            );
        }
        let ready_dropped = cap_ready_audio_to_live(&mut ready_host, max_ready_ticks);
        if ready_dropped > 0 {
            ready_host_bytes_dropped += ready_dropped as u64;
            eprintln!(
                "[AUDIO:{}] ready_host_bytes_dropped={} max_ready_ticks={}",
                stream_id, ready_dropped, max_ready_ticks
            );
        }
        let host_real_bytes = ready_host.len().min(AUDIO_BYTES_PER_TICK);
        let host_chunk = take_tick_sample(&mut ready_host);
        let host_requeue = host_chunk[..host_real_bytes].to_vec();
        let output = build_tick_output(TickOutputArgs {
            host_chunk,
            is_source,
            host_gain,
            tts_queue: &tts_queue,
        });

        let bytes = if output.bytes.is_empty() {
            &silence_chunk
        } else {
            &output.bytes
        };
        let n = bytes.len() as u64;
        match fifo_write_nonblocking(&mut fifo, bytes, &stop) {
            FifoWrite::Ok { would_blocks } => {
                fifo_would_blocks += would_blocks;
                if would_blocks > 0 {
                    eprintln!(
                        "[AUDIO:{}] fifo write backpressure would_blocks={}",
                        stream_id, would_blocks
                    );
                }
                last_write_ms.store(now_unix_ms(), Ordering::Release);
                commit_tts_drain(&tts_queue, output.tts_bytes_consumed);
                if let Some(m) = &metrics {
                    m.record_bytes_out(n);
                }
            }
            FifoWrite::Backpressured {
                would_blocks,
                elapsed,
            } => {
                fifo_would_blocks += would_blocks;
                fifo_skipped_ticks += 1;
                let requeued_host_bytes = host_requeue.len();
                let ready_dropped = requeue_host_audio_after_backpressure(
                    &mut ready_host,
                    host_requeue,
                    max_ready_ticks,
                );
                if ready_dropped > 0 {
                    ready_host_bytes_dropped += ready_dropped as u64;
                }
                eprintln!(
                    "[AUDIO:{}] fifo write skipped live tick would_blocks={} elapsed_ms={} requeued_host_bytes={} ready_host_bytes_dropped={}",
                    stream_id,
                    would_blocks,
                    elapsed.as_millis(),
                    requeued_host_bytes,
                    ready_host_bytes_dropped
                );
            }
            FifoWrite::Stopped => break,
            FifoWrite::Err => {
                if !stop.load(Ordering::Acquire) {
                    eprintln!("[AUDIO:{}] write error, exiting", stream_id);
                }
                break;
            }
        }
    }

    drop(fifo);
    eprintln!(
        "[AUDIO:{}] drain exited after {} ticks",
        stream_id, tick_count
    );
}

fn open_audio_fifo(path: &str, stream_id: &str, stop: &AtomicBool) -> Option<std::fs::File> {
    match std::fs::OpenOptions::new().write(true).open(path) {
        Ok(f) => {
            // Switch the writer fd to non-blocking so a dead reader (ffmpeg
            // killed mid-stream) cannot deadlock the drain thread on the next
            // tick. The drain loop polls stop_flag between WouldBlock retries.
            unsafe {
                let fd = f.as_raw_fd();
                let flags = libc::fcntl(fd, libc::F_GETFL);
                if flags >= 0 {
                    libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
            }
            Some(f)
        }
        Err(e) => {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[AUDIO:{}] failed to open FIFO: {}", stream_id, e);
            }
            None
        }
    }
}

enum FifoWrite {
    Ok {
        would_blocks: u64,
    },
    Backpressured {
        would_blocks: u64,
        elapsed: Duration,
    },
    Stopped,
    Err,
}

/// Write one live audio tick to a non-blocking FIFO. On WouldBlock, retry only
/// inside the current 20 ms media-clock budget. A blocked muxer must drop/skips
/// live ticks, not hold this thread and replay stale audio later.
fn fifo_write_nonblocking(fifo: &mut std::fs::File, bytes: &[u8], stop: &AtomicBool) -> FifoWrite {
    let started = Instant::now();
    let mut written = 0usize;
    let mut would_blocks = 0u64;
    while written < bytes.len() {
        if stop.load(Ordering::Acquire) {
            return FifoWrite::Stopped;
        }
        if started.elapsed() >= AUDIO_FIFO_WRITE_BUDGET {
            return FifoWrite::Backpressured {
                would_blocks,
                elapsed: started.elapsed(),
            };
        }
        match fifo.write(&bytes[written..]) {
            Ok(0) => return FifoWrite::Err,
            Ok(n) => written += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                would_blocks += 1;
                thread::sleep(Duration::from_millis(1));
            }
            Err(_) => return FifoWrite::Err,
        }
    }
    FifoWrite::Ok { would_blocks }
}

fn wait_audio_tick(next_tick: &mut Instant) -> Instant {
    let now = Instant::now();
    if *next_tick > now {
        thread::sleep(*next_tick - now);
    }
    let actual = Instant::now();
    *next_tick += AUDIO_TICK;
    actual
}

struct DrainHostArgs<'a> {
    host_buf: &'a StdMutex<VecDeque<TimedChunk>>,
    ready_host: &'a mut Vec<u8>,
    now: Instant,
    delay: Duration,
    max_lag: Duration,
}

fn drain_aged_host_audio(args: DrainHostArgs<'_>) -> usize {
    let mut buf = args.host_buf.lock().unwrap();
    let live_cutoff = args.now.checked_sub(args.max_lag).unwrap_or(args.now);
    let mut dropped = 0usize;

    // Same live policy as video: if old host-audio chunks piled up while
    // FFmpeg/RTMP was blocked, drop them instead of replaying delayed audio.
    while buf.len() > 1 {
        let Some((ts, _)) = buf.front() else { break };
        let target_ts = *ts + args.delay;
        if target_ts >= live_cutoff {
            break;
        }
        buf.pop_front();
        dropped += 1;
    }

    while let Some((ts, _)) = buf.front() {
        if *ts + args.delay <= args.now {
            let (_, pcm) = buf.pop_front().unwrap();
            args.ready_host.extend_from_slice(&pcm);
        } else {
            break;
        }
    }
    dropped
}

fn cap_ready_audio_to_live(ready_host: &mut Vec<u8>, max_ready_ticks: usize) -> usize {
    // Browser mic chunks are often ~93ms, so keeping only one 20ms tick causes
    // regular audio loss. Keep a catch-up window and only shed oldest original
    // host audio when a real write stall exceeds the live lag budget.
    let max_ready_bytes = AUDIO_BYTES_PER_TICK * max_ready_ticks.max(1);
    if ready_host.len() > max_ready_bytes {
        let keep_from = ready_host.len() - max_ready_bytes;
        ready_host.drain(..keep_from);
        keep_from
    } else {
        0
    }
}

fn requeue_host_audio_after_backpressure(
    ready_host: &mut Vec<u8>,
    host_requeue: Vec<u8>,
    max_ready_ticks: usize,
) -> usize {
    if !host_requeue.is_empty() {
        ready_host.splice(0..0, host_requeue);
    }
    cap_ready_audio_to_live(ready_host, max_ready_ticks)
}

fn take_tick_sample(ready_host: &mut Vec<u8>) -> Vec<u8> {
    let take = AUDIO_BYTES_PER_TICK.min(ready_host.len());
    let mut chunk: Vec<u8> = ready_host.drain(..take).collect();
    if chunk.len() < AUDIO_BYTES_PER_TICK {
        chunk.resize(AUDIO_BYTES_PER_TICK, 0);
    }
    chunk
}

struct TickOutputArgs<'a> {
    host_chunk: Vec<u8>,
    is_source: bool,
    host_gain: f32,
    tts_queue: &'a StdMutex<VecDeque<u8>>,
}

struct TickOutput {
    bytes: Vec<u8>,
    tts_bytes_consumed: usize,
}

fn build_tick_output(args: TickOutputArgs<'_>) -> TickOutput {
    let TickOutputArgs {
        host_chunk,
        is_source,
        host_gain,
        tts_queue,
    } = args;
    if is_source {
        // Source streams never queue TTS — skip the mix entirely.
        let bytes = if (host_gain - 1.0).abs() < f32::EPSILON {
            host_chunk
        } else {
            apply_gain(&host_chunk, host_gain)
        };
        TickOutput {
            bytes,
            tts_bytes_consumed: 0,
        }
    } else {
        let (mut tts_padded, tts_bytes_consumed): (Vec<u8>, usize) = {
            let q = tts_queue.lock().unwrap();
            let n = AUDIO_BYTES_PER_TICK.min(q.len());
            (q.iter().take(n).copied().collect(), n)
        };
        if tts_padded.len() < AUDIO_BYTES_PER_TICK {
            tts_padded.resize(AUDIO_BYTES_PER_TICK, 0);
        }
        TickOutput {
            bytes: mix_pcm_s16le(&host_chunk, host_gain, &tts_padded, 1.0),
            tts_bytes_consumed,
        }
    }
}

fn commit_tts_drain(tts_queue: &StdMutex<VecDeque<u8>>, bytes: usize) {
    if bytes == 0 {
        return;
    }
    let mut q = tts_queue.lock().unwrap();
    let drain = bytes.min(q.len());
    q.drain(..drain);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;
    use std::time::{Duration, Instant};

    fn chunk(bytes: Vec<u8>) -> Arc<[u8]> {
        Arc::from(bytes)
    }

    #[test]
    fn take_tick_sample_drains_full_buffer_and_pads_with_zeros() {
        let mut ready = vec![1u8; 100];
        let sample = take_tick_sample(&mut ready);

        assert_eq!(sample.len(), AUDIO_BYTES_PER_TICK);
        assert_eq!(&sample[..100], &vec![1u8; 100][..]);
        assert!(sample[100..].iter().all(|&b| b == 0));
        assert!(ready.is_empty());
    }

    #[test]
    fn take_tick_sample_takes_exactly_one_tick_when_buffer_has_more() {
        let mut ready = vec![2u8; AUDIO_BYTES_PER_TICK + 500];
        let sample = take_tick_sample(&mut ready);

        assert_eq!(sample.len(), AUDIO_BYTES_PER_TICK);
        assert_eq!(sample, vec![2u8; AUDIO_BYTES_PER_TICK]);
        assert_eq!(ready.len(), 500);
    }

    #[test]
    fn take_tick_sample_returns_zero_filled_tick_when_ready_is_empty() {
        let mut ready: Vec<u8> = Vec::new();
        let sample = take_tick_sample(&mut ready);
        assert_eq!(sample, vec![0u8; AUDIO_BYTES_PER_TICK]);
    }

    #[test]
    fn build_tick_output_source_stream_bypasses_mix_at_unit_gain() {
        let q: StdMutex<VecDeque<u8>> = StdMutex::new(VecDeque::new());
        let host = vec![0x10, 0x27];
        let out = build_tick_output(TickOutputArgs {
            host_chunk: host.clone(),
            is_source: true,
            host_gain: 1.0,
            tts_queue: &q,
        });
        assert_eq!(out.bytes, host);
        assert_eq!(out.tts_bytes_consumed, 0);
    }

    #[test]
    fn build_tick_output_source_stream_applies_gain_when_not_unit() {
        let q: StdMutex<VecDeque<u8>> = StdMutex::new(VecDeque::new());
        let out = build_tick_output(TickOutputArgs {
            host_chunk: vec![0x10, 0x27], // 10_000 → *0.5 = 5_000
            is_source: true,
            host_gain: 0.5,
            tts_queue: &q,
        });
        let sample = i16::from_le_bytes([out.bytes[0], out.bytes[1]]);
        assert_eq!(sample, 5_000);
        assert_eq!(out.tts_bytes_consumed, 0);
    }

    #[test]
    fn build_tick_output_target_stream_mixes_host_with_tts_queue() {
        let mut q: VecDeque<u8> = VecDeque::new();
        // 5_000 (le) as a single sample in the TTS queue, then empty → resize
        // to full tick with zeros.
        q.extend(5_000_i16.to_le_bytes());
        let queue = StdMutex::new(q);

        let host_chunk = vec![0x10, 0x27]; // 10_000
        let out = build_tick_output(TickOutputArgs {
            host_chunk,
            is_source: false,
            host_gain: 0.2,
            tts_queue: &queue,
        });
        // host 10_000 * 0.2 = 2_000; plus TTS 5_000 = 7_000.
        assert_eq!(i16::from_le_bytes([out.bytes[0], out.bytes[1]]), 7_000);
        // Remaining bytes padded to tick width with tts zeros + host zeros.
        assert_eq!(out.bytes.len(), AUDIO_BYTES_PER_TICK.min(2));
        assert_eq!(out.tts_bytes_consumed, 2);
        assert_eq!(queue.lock().unwrap().len(), 2);
    }

    #[test]
    fn build_tick_output_target_stream_peeks_one_tick_from_tts_queue() {
        let queue = StdMutex::new(VecDeque::from(vec![0u8; AUDIO_BYTES_PER_TICK * 2]));
        let host_chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
        let out = build_tick_output(TickOutputArgs {
            host_chunk,
            is_source: false,
            host_gain: 0.2,
            tts_queue: &queue,
        });
        assert_eq!(out.tts_bytes_consumed, AUDIO_BYTES_PER_TICK);
        assert_eq!(queue.lock().unwrap().len(), AUDIO_BYTES_PER_TICK * 2);
        commit_tts_drain(&queue, out.tts_bytes_consumed);
        assert_eq!(queue.lock().unwrap().len(), AUDIO_BYTES_PER_TICK);
    }

    #[test]
    fn backpressure_requeues_host_audio_and_keeps_tts_until_success() {
        let tts_queue = StdMutex::new(VecDeque::from(vec![7u8; AUDIO_BYTES_PER_TICK * 2]));
        let mut ready_host = vec![1u8; AUDIO_BYTES_PER_TICK];
        let host_real_bytes = ready_host.len().min(AUDIO_BYTES_PER_TICK);
        let host_chunk = take_tick_sample(&mut ready_host);
        let host_requeue = host_chunk[..host_real_bytes].to_vec();
        let out = build_tick_output(TickOutputArgs {
            host_chunk,
            is_source: false,
            host_gain: 0.2,
            tts_queue: &tts_queue,
        });

        assert!(ready_host.is_empty());
        assert_eq!(out.tts_bytes_consumed, AUDIO_BYTES_PER_TICK);
        assert_eq!(tts_queue.lock().unwrap().len(), AUDIO_BYTES_PER_TICK * 2);

        let dropped = requeue_host_audio_after_backpressure(
            &mut ready_host,
            host_requeue,
            DEFAULT_AUDIO_MAX_READY_TICKS,
        );

        assert_eq!(dropped, 0);
        assert_eq!(ready_host, vec![1u8; AUDIO_BYTES_PER_TICK]);
        assert_eq!(tts_queue.lock().unwrap().len(), AUDIO_BYTES_PER_TICK * 2);

        commit_tts_drain(&tts_queue, out.tts_bytes_consumed);
        assert_eq!(tts_queue.lock().unwrap().len(), AUDIO_BYTES_PER_TICK);
    }

    #[test]
    fn drain_next_h264_live_returns_one_aged_chunk() {
        let buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        {
            let mut b = buf.lock().unwrap();
            b.push_back((now - Duration::from_millis(120), chunk(vec![1])));
            b.push_back((now + Duration::from_millis(500), chunk(vec![2])));
        }
        let mut need_keyframe = false;
        let result = drain_next_h264_live(
            &buf,
            now,
            delay,
            Duration::from_millis(250),
            &mut need_keyframe,
        );
        let result = result.unwrap();
        assert_eq!(result.chunk, chunk(vec![1]));
        assert_eq!(result.dropped_stale, 0);
        assert_eq!(result.dropped_for_keyframe, 0);
        assert_eq!(buf.lock().unwrap().len(), 1);
    }

    #[test]
    fn drain_next_h264_live_catches_up_within_lag_window() {
        let buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        {
            let mut b = buf.lock().unwrap();
            b.push_back((now - Duration::from_millis(1_000), chunk(vec![1])));
            b.push_back((now - Duration::from_millis(150), chunk(vec![0, 0, 1, 5])));
            b.push_back((now + Duration::from_millis(500), chunk(vec![4])));
        }
        let mut need_keyframe = false;
        let result =
            drain_next_h264_live(&buf, now, delay, Duration::from_secs(3), &mut need_keyframe)
                .unwrap();
        assert_eq!(result.chunk, chunk(vec![1]));
        assert_eq!(result.dropped_stale, 0);
        assert_eq!(result.dropped_for_keyframe, 0);
        assert_eq!(buf.lock().unwrap().len(), 2);
    }

    #[test]
    fn drain_next_h264_live_drops_stale_backlog_past_lag_window() {
        let buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        {
            let mut b = buf.lock().unwrap();
            b.push_back((now - Duration::from_millis(2_000), chunk(vec![1])));
            b.push_back((now - Duration::from_millis(1_000), chunk(vec![2])));
            b.push_back((now - Duration::from_millis(150), chunk(vec![0, 0, 1, 5])));
            b.push_back((now + Duration::from_millis(500), chunk(vec![4])));
        }
        let mut need_keyframe = false;
        let result = drain_next_h264_live(
            &buf,
            now,
            delay,
            Duration::from_millis(250),
            &mut need_keyframe,
        );
        let result = result.unwrap();
        assert_eq!(result.chunk, chunk(vec![0, 0, 1, 5]));
        assert_eq!(result.dropped_stale, 2);
        assert_eq!(result.dropped_for_keyframe, 0);
        assert_eq!(buf.lock().unwrap().len(), 1);
    }

    #[test]
    fn drain_next_h264_live_prefers_idr_after_stale_drop() {
        let buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        let p_slice = vec![0, 0, 1, 1, 0xaa];
        let idr = vec![0, 0, 0, 1, 5, 0xbb];
        {
            let mut b = buf.lock().unwrap();
            b.push_back((now - Duration::from_millis(2_000), chunk(vec![9])));
            b.push_back((now - Duration::from_millis(150), chunk(p_slice)));
            b.push_back((now - Duration::from_millis(140), chunk(idr.clone())));
            b.push_back((now + Duration::from_millis(500), chunk(vec![4])));
        }

        let mut need_keyframe = false;
        let result = drain_next_h264_live(
            &buf,
            now,
            delay,
            Duration::from_millis(250),
            &mut need_keyframe,
        );

        let result = result.unwrap();
        assert_eq!(result.chunk, chunk(idr.to_vec()));
        assert_eq!(result.dropped_stale, 1);
        assert_eq!(result.dropped_for_keyframe, 1);
    }

    #[test]
    fn drain_next_h264_live_waits_for_keyframe_on_fresh_ffmpeg_pipe() {
        let buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        {
            let mut b = buf.lock().unwrap();
            b.push_back((
                now - Duration::from_millis(150),
                chunk(vec![0, 0, 1, 1, 0xaa]),
            ));
            b.push_back((
                now - Duration::from_millis(140),
                chunk(vec![0, 0, 1, 5, 0xbb]),
            ));
        }

        let mut need_keyframe = true;
        let result = drain_next_h264_live(
            &buf,
            now,
            delay,
            Duration::from_millis(250),
            &mut need_keyframe,
        );

        let result = result.unwrap();
        assert_eq!(result.chunk, chunk(vec![0, 0, 1, 5, 0xbb]));
        assert_eq!(result.dropped_stale, 0);
        assert_eq!(result.dropped_for_keyframe, 1);
        assert!(!need_keyframe);
    }

    #[test]
    fn h264_annexb_contains_idr_detects_three_and_four_byte_start_codes() {
        assert!(h264_annexb_contains_idr(&[0, 0, 1, 5, 1]));
        assert!(h264_annexb_contains_idr(&[9, 0, 0, 0, 1, 0x65, 1]));
        assert!(!h264_annexb_contains_idr(&[0, 0, 1, 1, 1]));
    }

    #[test]
    fn drain_next_h264_live_returns_none_when_nothing_has_aged() {
        let buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        {
            let mut b = buf.lock().unwrap();
            b.push_back((now + Duration::from_millis(10), chunk(vec![1])));
        }
        let mut need_keyframe = false;
        let result = drain_next_h264_live(
            &buf,
            now,
            Duration::from_millis(100),
            Duration::from_millis(250),
            &mut need_keyframe,
        );
        assert!(result.is_none());
        assert_eq!(buf.lock().unwrap().len(), 1);
    }

    #[test]
    fn drain_aged_host_audio_extends_ready_buffer_only_with_aged_chunks() {
        let host_buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        {
            let mut b = host_buf.lock().unwrap();
            b.push_back((now - Duration::from_millis(150), chunk(vec![0xbb])));
            b.push_back((now + Duration::from_millis(50), chunk(vec![0xcc])));
        }
        let mut ready = Vec::new();
        let dropped = drain_aged_host_audio(DrainHostArgs {
            host_buf: &host_buf,
            ready_host: &mut ready,
            now,
            delay,
            max_lag: Duration::from_millis(250),
        });
        assert_eq!(dropped, 0);
        assert_eq!(ready, vec![0xbb]);
        assert_eq!(host_buf.lock().unwrap().len(), 1);
    }

    #[test]
    fn drain_aged_host_audio_drops_stale_backlog() {
        let host_buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        {
            let mut b = host_buf.lock().unwrap();
            b.push_back((now - Duration::from_millis(2_000), chunk(vec![0xaa])));
            b.push_back((now - Duration::from_millis(1_000), chunk(vec![0xbb])));
            b.push_back((now - Duration::from_millis(150), chunk(vec![0xcc])));
            b.push_back((now + Duration::from_millis(50), chunk(vec![0xdd])));
        }
        let mut ready = Vec::new();
        let dropped = drain_aged_host_audio(DrainHostArgs {
            host_buf: &host_buf,
            ready_host: &mut ready,
            now,
            delay,
            max_lag: Duration::from_millis(250),
        });
        assert_eq!(dropped, 2);
        assert_eq!(ready, vec![0xcc]);
        assert_eq!(host_buf.lock().unwrap().len(), 1);
    }

    #[test]
    fn cap_ready_audio_to_live_preserves_normal_browser_audio_chunk() {
        // ScriptProcessor 4096 frames at s16le mono ≈93ms = 8192 bytes. This
        // must survive intact; otherwise original host audio sounds choppy.
        let mut ready = vec![1u8; 8192];
        let dropped = cap_ready_audio_to_live(&mut ready, DEFAULT_AUDIO_MAX_READY_TICKS);
        assert_eq!(dropped, 0);
        assert_eq!(ready, vec![1u8; 8192]);
    }

    #[test]
    fn cap_ready_audio_to_live_drops_only_when_ready_exceeds_live_budget() {
        let max_ready_ticks = 13;
        let max_ready_bytes = AUDIO_BYTES_PER_TICK * max_ready_ticks;
        let mut ready = vec![1u8; AUDIO_BYTES_PER_TICK * 2];
        ready.extend(vec![2u8; max_ready_bytes]);
        let dropped = cap_ready_audio_to_live(&mut ready, max_ready_ticks);
        assert_eq!(dropped, AUDIO_BYTES_PER_TICK * 2);
        assert_eq!(ready, vec![2u8; max_ready_bytes]);
    }

    #[test]
    fn wait_audio_tick_advances_next_tick_by_one_audio_tick() {
        let start = Instant::now();
        let mut next = start + Duration::from_millis(1);
        let actual = wait_audio_tick(&mut next);
        assert!(actual >= start);
        assert_eq!(next - start, Duration::from_millis(1) + AUDIO_TICK);
    }

    #[test]
    fn open_audio_fifo_returns_none_when_path_does_not_exist() {
        let stop = AtomicBool::new(false);
        let out = open_audio_fifo(
            "/tmp/definitely_not_a_real_brivva_fifo_0xdeadbeef",
            "sid",
            &stop,
        );
        assert!(out.is_none());
    }

    #[test]
    fn open_audio_fifo_returns_none_silently_when_stop_flag_already_set() {
        // stop=true suppresses the stderr log but still returns None on error.
        let stop = AtomicBool::new(true);
        let out = open_audio_fifo(
            "/tmp/definitely_not_a_real_brivva_fifo_0xcafebabe",
            "sid",
            &stop,
        );
        assert!(out.is_none());
    }

    #[test]
    fn open_audio_fifo_returns_some_when_path_is_writable() {
        let path =
            std::env::temp_dir().join(format!("brivva_drain_test_{}.bin", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        let path_str = path.to_str().unwrap();
        let stop = AtomicBool::new(false);
        let file = open_audio_fifo(path_str, "sid", &stop);
        assert!(file.is_some());
        drop(file);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn audio_drain_loop_exits_immediately_when_stop_flag_is_already_set() {
        // Use a real file as the "fifo" so open succeeds, then preset stop.
        let path = std::env::temp_dir().join(format!(
            "brivva_audio_drain_stop_{}.bin",
            std::process::id()
        ));
        std::fs::write(&path, b"").unwrap();

        let ctx = AudioDrainCtx {
            stream_id: "sid".into(),
            host_buf: Arc::new(StdMutex::new(VecDeque::new())),
            tts_queue: Arc::new(StdMutex::new(VecDeque::new())),
            fifo_path: path.to_str().unwrap().into(),
            delay: Duration::from_millis(0),
            is_source: false,
            host_gain: 1.0,
            stop: Arc::new(AtomicBool::new(true)),
            last_write_ms: Arc::new(AtomicI64::new(0)),
            metrics: None,
        };
        audio_drain_loop(ctx); // should return without entering the tick loop
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn audio_drain_loop_exits_when_fifo_cannot_be_opened() {
        // Nonexistent path; open_audio_fifo returns None and drain returns.
        let ctx = AudioDrainCtx {
            stream_id: "sid".into(),
            host_buf: Arc::new(StdMutex::new(VecDeque::new())),
            tts_queue: Arc::new(StdMutex::new(VecDeque::new())),
            fifo_path: "/tmp/brivva_audio_missing_0xfeedface".into(),
            delay: Duration::from_millis(0),
            is_source: true,
            host_gain: 1.0,
            stop: Arc::new(AtomicBool::new(false)),
            last_write_ms: Arc::new(AtomicI64::new(0)),
            metrics: None,
        };
        audio_drain_loop(ctx);
    }

    #[test]
    fn video_drain_loop_exits_when_stop_flag_preset() {
        let mut child = std::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let buf: Arc<StdMutex<VecDeque<TimedChunk>>> = Arc::new(StdMutex::new(VecDeque::new()));
        let ctx = VideoDrainCtx {
            stream_id: "sid".into(),
            h264_buf: buf,
            video_stdin: stdin,
            delay: Duration::from_millis(0),
            stop: Arc::new(AtomicBool::new(true)),
            last_write_ms: Arc::new(AtomicI64::new(0)),
            metrics: None,
        };
        video_drain_loop(ctx);
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn video_drain_loop_writes_ready_h264_chunk_to_pipe() {
        let path = std::env::temp_dir().join(format!(
            "brivva_video_pipe_test_{}.h264",
            std::process::id()
        ));
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("cat > {}", path.display()))
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let buf = Arc::new(StdMutex::new(VecDeque::new()));
        buf.lock().unwrap().push_back((
            Instant::now() - Duration::from_secs(1),
            chunk(vec![0, 0, 1, 5, 1, 2, 3]),
        ));
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = VideoDrainCtx {
            stream_id: "sid".into(),
            h264_buf: buf,
            video_stdin: stdin,
            delay: Duration::from_millis(0),
            stop: stop.clone(),
            last_write_ms: Arc::new(AtomicI64::new(0)),
            metrics: None,
        };
        let handle = thread::spawn(move || video_drain_loop(ctx));
        thread::sleep(Duration::from_millis(50));
        stop.store(true, Ordering::Release);
        handle.join().unwrap();
        let _ = child.wait();
        assert_eq!(std::fs::read(&path).unwrap(), vec![0, 0, 1, 5, 1, 2, 3]);
        let _ = std::fs::remove_file(&path);
    }
}
