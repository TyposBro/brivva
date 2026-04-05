//! Dedicated OS thread: forwards encoded video chunks to FFmpeg stdin
//! after the broadcast delay has elapsed.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};

use super::types::{STALE_CHUNK_MARGIN_SECS, VIDEO_POLL_INTERVAL, VIDEO_STATS_INTERVAL};

// ── Config / State structs (keep every function ≤ 2 params) ──

/// Immutable configuration for the video drain thread.
pub(crate) struct VideoDrainConfig {
    pub(crate) stream_id: String,
    pub(crate) chunk_buffer: Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    pub(crate) init_segment: Arc<StdMutex<Option<Vec<u8>>>>,
    pub(crate) is_restart: bool,
    pub(crate) delay: Duration,
    pub(crate) stop: Arc<AtomicBool>,
}

/// Mutable state passed through the drain helpers (stdin + stats).
struct VideoDrainState<'a> {
    stdin: &'a mut std::process::ChildStdin,
    stats: &'a mut DrainStats,
}

// ── Stats ────────────────────────────────────────────────

struct DrainStats {
    chunks_written: u64,
    total_bytes_written: u64,
    started_at: Instant,
}

impl DrainStats {
    fn new() -> Self {
        Self {
            chunks_written: 0,
            total_bytes_written: 0,
            started_at: Instant::now(),
        }
    }

    fn record(&mut self, bytes: u64) {
        self.chunks_written += 1;
        self.total_bytes_written += bytes;
    }

    fn log_periodic(&self, stream_id: &str) {
        if self.chunks_written.is_multiple_of(VIDEO_STATS_INTERVAL) {
            tracing::debug!(
                "[VIDEO:{}] stats: {} chunks, {}KB written, {:.0}s elapsed",
                stream_id,
                self.chunks_written,
                self.total_bytes_written / 1024,
                self.started_at.elapsed().as_secs_f64()
            );
        }
    }

    fn log_final(&self, stream_id: &str) {
        tracing::info!(
            "[VIDEO:{}] chunk drain thread exited after {} chunks ({}KB, {:.0}s)",
            stream_id,
            self.chunks_written,
            self.total_bytes_written / 1024,
            self.started_at.elapsed().as_secs_f64()
        );
    }
}

// ── Public API ───────────────────────────────────────────

/// Dedicated OS thread: forwards encoded video chunks to FFmpeg stdin
/// after the broadcast delay has elapsed.
///
/// Polls every 20ms. Chunks older than D seconds are written to FFmpeg.
/// Much lighter than the old per-frame JPEG approach since the browser's
/// hardware encoder already compressed the video.
pub(crate) fn video_chunk_drain_loop(
    config: VideoDrainConfig,
    mut stdin: std::process::ChildStdin,
) {
    let mut stats = DrainStats::new();

    tracing::info!(
        "[VIDEO:{}] chunk drain thread started ({}ms delay)",
        config.stream_id,
        config.delay.as_millis()
    );

    {
        let mut state = VideoDrainState { stdin: &mut stdin, stats: &mut stats };
        if config.is_restart {
            if handle_restart(&config, &mut state).is_err() {
                return;
            }
        } else if write_init_segment_on_first_spawn(&config, &mut state).is_err() {
            return;
        }
    }

    loop {
        if config.stop.load(Ordering::Acquire) {
            break;
        }
        thread::sleep(VIDEO_POLL_INTERVAL);
        let mut state = VideoDrainState { stdin: &mut stdin, stats: &mut stats };
        if drain_ready_chunks(&config, &mut state).is_err() {
            return;
        }
    }

    drop(stdin);
    stats.log_final(&config.stream_id);
}

// ── Private helpers ──────────────────────────────────────

/// Replay the init segment and flush stale chunks after an FFmpeg restart.
fn handle_restart(
    config: &VideoDrainConfig,
    state: &mut VideoDrainState,
) -> Result<(), ()> {
    replay_init_segment(config, state)?;
    flush_stale_chunks(config);
    Ok(())
}

/// Wait for the init segment on first spawn and write it before any data chunks.
/// Without this, trim_video_for_activation() may have removed the init segment from
/// the chunk buffer, causing FFmpeg to see moof fragments without a preceding moov/trex.
fn write_init_segment_on_first_spawn(
    config: &VideoDrainConfig,
    state: &mut VideoDrainState,
) -> Result<(), ()> {
    const MAX_WAIT: Duration = Duration::from_secs(30);
    let start = Instant::now();

    loop {
        {
            let init = config.init_segment.lock().unwrap();
            if let Some(ref seg) = *init {
                tracing::info!(
                    "[VIDEO:{}] writing init segment ({}B) on first spawn",
                    config.stream_id,
                    seg.len()
                );
                if state.stdin.write_all(seg).is_err() {
                    tracing::error!(
                        "[VIDEO:{}] write error on init segment, exiting",
                        config.stream_id
                    );
                    return Err(());
                }
                state.stats.total_bytes_written += seg.len() as u64;
                return Ok(());
            }
        }
        if config.stop.load(Ordering::Acquire) {
            return Err(());
        }
        if start.elapsed() > MAX_WAIT {
            tracing::error!(
                "[VIDEO:{}] init segment not available after {}s, starting without it",
                config.stream_id,
                MAX_WAIT.as_secs()
            );
            return Ok(());
        }
        thread::sleep(VIDEO_POLL_INTERVAL);
    }
}

/// Replay the init segment so FFmpeg can parse the fMP4 container.
/// Without this, mid-stream moof/mdat fragments cause "could not find trex" errors.
fn replay_init_segment(
    config: &VideoDrainConfig,
    state: &mut VideoDrainState,
) -> Result<(), ()> {
    let init = config.init_segment.lock().unwrap();
    match *init {
        Some(ref seg) => {
            tracing::info!(
                "[VIDEO:{}] replaying init segment ({}B) for restart",
                config.stream_id,
                seg.len()
            );
            if state.stdin.write_all(seg).is_err() {
                tracing::error!(
                    "[VIDEO:{}] write error replaying init segment, exiting",
                    config.stream_id
                );
                return Err(());
            }
            state.stats.total_bytes_written += seg.len() as u64;
            Ok(())
        }
        None => {
            tracing::warn!("[VIDEO:{}] restart but no init segment saved", config.stream_id);
            Ok(())
        }
    }
}

/// Remove chunks from before the crash — their timestamps won't match
/// the new FFmpeg timeline.
fn flush_stale_chunks(config: &VideoDrainConfig) -> usize {
    let mut buf = config.chunk_buffer.lock().unwrap();
    let stale_cutoff = Instant::now() - config.delay - Duration::from_secs(STALE_CHUNK_MARGIN_SECS);
    let before = buf.len();
    buf.retain(|(ts, _)| *ts > stale_cutoff);
    let dropped = before - buf.len();
    if dropped > 0 {
        tracing::warn!(
            "[VIDEO:{}] flushed {} stale chunks on restart",
            config.stream_id,
            dropped
        );
    }
    dropped
}

/// Pop and write all chunks whose delay has elapsed.
fn drain_ready_chunks(
    config: &VideoDrainConfig,
    state: &mut VideoDrainState,
) -> Result<(), ()> {
    let deadline = Instant::now() - config.delay;
    loop {
        let chunk = pop_ready_chunk(&config.chunk_buffer, deadline);
        match chunk {
            Some((_, data)) => write_chunk(config, state, data)?,
            None => return Ok(()),
        }
    }
}

/// Pop the front chunk if its timestamp is at or before the deadline.
fn pop_ready_chunk(
    chunk_buffer: &Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    deadline: Instant,
) -> Option<(Instant, Vec<u8>)> {
    let mut buf = chunk_buffer.lock().unwrap();
    match buf.front() {
        Some((ts, _)) if *ts <= deadline => buf.pop_front(),
        _ => None,
    }
}

/// Write one chunk to FFmpeg stdin; returns Err(()) on pipe failure.
fn write_chunk(
    config: &VideoDrainConfig,
    state: &mut VideoDrainState,
    data: Vec<u8>,
) -> Result<(), ()> {
    let data_len = data.len() as u64;
    if state.stdin.write_all(&data).is_err() {
        if !config.stop.load(Ordering::Acquire) {
            tracing::error!("[VIDEO:{}] write error, exiting", config.stream_id);
        }
        return Err(());
    }
    state.stats.record(data_len);
    state.stats.log_periodic(&config.stream_id);
    Ok(())
}
