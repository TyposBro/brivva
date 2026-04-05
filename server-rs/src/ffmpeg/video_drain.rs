//! Dedicated OS thread: forwards encoded video chunks to FFmpeg stdin
//! after the broadcast delay has elapsed.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};

use super::types::{STALE_CHUNK_MARGIN_SECS, VIDEO_POLL_INTERVAL, VIDEO_STATS_INTERVAL};

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
    stream_id: String,
    chunk_buffer: Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    init_segment: Arc<StdMutex<Option<Vec<u8>>>>,
    is_restart: bool,
    mut stdin: std::process::ChildStdin,
    delay: Duration,
    stop: Arc<AtomicBool>,
) {
    let mut stats = DrainStats::new();

    tracing::info!(
        "[VIDEO:{}] chunk drain thread started ({}ms delay)",
        stream_id,
        delay.as_millis()
    );

    if is_restart {
        if handle_restart(&stream_id, &init_segment, &chunk_buffer, &mut stdin, delay, &mut stats)
            .is_err()
        {
            return;
        }
    }

    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        thread::sleep(VIDEO_POLL_INTERVAL);
        if drain_ready_chunks(&stream_id, &chunk_buffer, &mut stdin, delay, &stop, &mut stats)
            .is_err()
        {
            return;
        }
    }

    drop(stdin);
    stats.log_final(&stream_id);
}

// ── Private helpers ──────────────────────────────────────

/// Replay the init segment and flush stale chunks after an FFmpeg restart.
fn handle_restart(
    stream_id: &str,
    init_segment: &Arc<StdMutex<Option<Vec<u8>>>>,
    chunk_buffer: &Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    stdin: &mut std::process::ChildStdin,
    delay: Duration,
    stats: &mut DrainStats,
) -> Result<(), ()> {
    replay_init_segment(stream_id, init_segment, stdin, stats)?;
    flush_stale_chunks(stream_id, chunk_buffer, delay);
    Ok(())
}

/// Replay the init segment so FFmpeg can parse the fMP4 container.
/// Without this, mid-stream moof/mdat fragments cause "could not find trex" errors.
fn replay_init_segment(
    stream_id: &str,
    init_segment: &Arc<StdMutex<Option<Vec<u8>>>>,
    stdin: &mut std::process::ChildStdin,
    stats: &mut DrainStats,
) -> Result<(), ()> {
    let init = init_segment.lock().unwrap();
    match *init {
        Some(ref seg) => {
            tracing::info!(
                "[VIDEO:{}] replaying init segment ({}B) for restart",
                stream_id,
                seg.len()
            );
            if stdin.write_all(seg).is_err() {
                tracing::error!(
                    "[VIDEO:{}] write error replaying init segment, exiting",
                    stream_id
                );
                return Err(());
            }
            stats.total_bytes_written += seg.len() as u64;
            Ok(())
        }
        None => {
            tracing::warn!("[VIDEO:{}] restart but no init segment saved", stream_id);
            Ok(())
        }
    }
}

/// Remove chunks from before the crash — their timestamps won't match
/// the new FFmpeg timeline.
fn flush_stale_chunks(
    stream_id: &str,
    chunk_buffer: &Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    delay: Duration,
) -> usize {
    let mut buf = chunk_buffer.lock().unwrap();
    let stale_cutoff = Instant::now() - delay - Duration::from_secs(STALE_CHUNK_MARGIN_SECS);
    let before = buf.len();
    buf.retain(|(ts, _)| *ts > stale_cutoff);
    let dropped = before - buf.len();
    if dropped > 0 {
        tracing::warn!(
            "[VIDEO:{}] flushed {} stale chunks on restart",
            stream_id,
            dropped
        );
    }
    dropped
}

/// Pop and write all chunks whose delay has elapsed.
fn drain_ready_chunks(
    stream_id: &str,
    chunk_buffer: &Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    stdin: &mut std::process::ChildStdin,
    delay: Duration,
    stop: &Arc<AtomicBool>,
    stats: &mut DrainStats,
) -> Result<(), ()> {
    let deadline = Instant::now() - delay;
    loop {
        let chunk = pop_ready_chunk(chunk_buffer, deadline);
        match chunk {
            Some((_, data)) => write_chunk(stream_id, stdin, data, stop, stats)?,
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
    stream_id: &str,
    stdin: &mut std::process::ChildStdin,
    data: Vec<u8>,
    stop: &Arc<AtomicBool>,
    stats: &mut DrainStats,
) -> Result<(), ()> {
    let data_len = data.len() as u64;
    if stdin.write_all(&data).is_err() {
        if !stop.load(Ordering::Acquire) {
            tracing::error!("[VIDEO:{}] write error, exiting", stream_id);
        }
        return Err(());
    }
    stats.record(data_len);
    stats.log_periodic(stream_id);
    Ok(())
}
