//! Dedicated OS thread: forwards encoded video chunks to FFmpeg stdin
//! after the broadcast delay has elapsed.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};

// ── Video Chunk Drain Thread ──────────────────────────────

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
    let poll_interval = Duration::from_millis(20);
    let mut chunks_written: u64 = 0;
    let mut total_bytes_written: u64 = 0;
    let drain_start = Instant::now();

    tracing::info!(
        "[VIDEO:{}] chunk drain thread started ({}ms delay)",
        stream_id, delay.as_millis()
    );

    // On restart: replay the init segment so FFmpeg can parse the fMP4 container.
    // Without this, mid-stream moof/mdat fragments cause "could not find trex" errors.
    if is_restart {
        let init = init_segment.lock().unwrap();
        if let Some(ref seg) = *init {
            tracing::info!("[VIDEO:{}] replaying init segment ({}B) for restart", stream_id, seg.len());
            if stdin.write_all(seg).is_err() {
                tracing::error!("[VIDEO:{}] write error replaying init segment, exiting", stream_id);
                drop(stdin);
                return;
            }
            total_bytes_written += seg.len() as u64;
        } else {
            tracing::warn!("[VIDEO:{}] restart but no init segment saved", stream_id);
        }

        // Flush stale chunks that are too old — they're from before the crash
        // and their timestamps won't match the new FFmpeg timeline
        let mut buf = chunk_buffer.lock().unwrap();
        let now = Instant::now();
        let stale_cutoff = now - delay - Duration::from_secs(1);
        let before = buf.len();
        buf.retain(|(ts, _)| *ts > stale_cutoff);
        let dropped = before - buf.len();
        if dropped > 0 {
            tracing::warn!("[VIDEO:{}] flushed {} stale chunks on restart", stream_id, dropped);
        }
    }

    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }

        thread::sleep(poll_interval);

        let deadline = Instant::now() - delay;

        // Drain all chunks that are old enough
        loop {
            let chunk = {
                let mut buf = chunk_buffer.lock().unwrap();
                match buf.front() {
                    Some((ts, _)) if *ts <= deadline => buf.pop_front(),
                    _ => None,
                }
            };

            match chunk {
                Some((_, data)) => {
                    let data_len = data.len();
                    if stdin.write_all(&data).is_err() {
                        if !stop.load(Ordering::Acquire) {
                            tracing::error!("[VIDEO:{}] write error, exiting", stream_id);
                        }
                        drop(stdin);
                        return;
                    }
                    chunks_written += 1;
                    total_bytes_written += data_len as u64;

                    // Periodic stats every 100 chunks (~10s at 10 chunks/sec)
                    if chunks_written % 100 == 0 {
                        tracing::debug!(
                            "[VIDEO:{}] stats: {} chunks, {}KB written, {:.0}s elapsed",
                            stream_id, chunks_written, total_bytes_written / 1024,
                            drain_start.elapsed().as_secs_f64()
                        );
                    }
                }
                None => break,
            }
        }
    }

    drop(stdin);
    tracing::info!(
        "[VIDEO:{}] chunk drain thread exited after {} chunks ({}KB, {:.0}s)",
        stream_id, chunks_written, total_bytes_written / 1024, drain_start.elapsed().as_secs_f64()
    );
}
