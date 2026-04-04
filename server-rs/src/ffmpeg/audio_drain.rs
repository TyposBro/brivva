//! Dedicated OS thread: drains audio at 20ms ticks.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::constants::BYTES_PER_SEC;
use super::{
    QueuedAudio,
    AUDIO_TICK, AUDIO_BYTES_PER_TICK,
    JITTER_WARN_THRESHOLD, JITTER_RECOVERY_THRESHOLD,
    MAX_RECOVERY_TICKS,
};

/// State for draining queued audio chunk-by-chunk
struct ActiveAudio {
    pcm: Arc<StdMutex<Vec<u8>>>,
    complete: Arc<AtomicBool>,
    offset: usize,
}

// ── Audio Drain Thread ─────────────────────────────────────

/// Dedicated OS thread: drains audio at 20ms ticks.
///
/// Independent from the video thread — shares only the delayed clock reference.
/// Tracks cumulative samples written to detect drift over long sessions.
pub(crate) fn audio_drain_loop(
    stream_id: String,
    audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    fifo_path: String,
    delay: Duration,
    stop: Arc<AtomicBool>,
) {
    // Open FIFO for writing (blocks until FFmpeg opens it for reading)
    tracing::info!("[AUDIO:{}] opening FIFO (blocks until FFmpeg reads)...", stream_id);
    let mut fifo = match std::fs::OpenOptions::new()
        .write(true)
        .open(&fifo_path)
    {
        Ok(f) => f,
        Err(e) => {
            if !stop.load(Ordering::Acquire) {
                tracing::error!("[AUDIO:{}] failed to open FIFO: {}", stream_id, e);
            }
            return;
        }
    };
    tracing::info!("[AUDIO:{}] FIFO opened", stream_id);

    let silence = vec![0u8; AUDIO_BYTES_PER_TICK];
    let mut active_audio: Option<ActiveAudio> = None;
    let mut tick_count: u64 = 0;
    // Reset tick anchor AFTER FIFO opens (FIFO open blocks on FFmpeg startup)
    let mut next_tick = Instant::now() + AUDIO_TICK;

    // Cumulative sample tracking for drift detection
    let start_time = Instant::now();
    let mut total_bytes_written: u64 = 0;
    let mut jitter_warn_count: u64 = 0;

    tracing::info!(
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

        if jitter > JITTER_RECOVERY_THRESHOLD && tick_count > 0 {
            // Severe jitter: reset the tick anchor and write catch-up data to the FIFO.
            //
            // Previously this did `continue` without writing, which starved FFmpeg's
            // audio FIFO. FFmpeg blocks on audio read when the FIFO is empty, which
            // stalls the RTMP muxer entirely (both video AND audio stop), causing
            // YouTube to pause/buffer. Writing silence (or any available audio data)
            // keeps the FIFO fed so FFmpeg can keep muxing.
            let skipped_ticks = jitter.as_millis() / AUDIO_TICK.as_millis();
            // Cap burst size to avoid overwhelming RTMP with a multi-MB write.
            // Skip the remaining ticks — we've already lost sync.
            let write_ticks = (skipped_ticks as usize).min(MAX_RECOVERY_TICKS);
            let catch_up_bytes = write_ticks * AUDIO_BYTES_PER_TICK;

            // Build catch-up buffer: drain any active audio first, fill rest with silence
            let mut catch_up = vec![0u8; catch_up_bytes];
            let mut audio_used = 0usize;
            let mut should_clear_active = false;

            if let Some(ref mut active) = active_audio {
                let guard = active.pcm.lock().unwrap();
                let available = guard.len().saturating_sub(active.offset);
                let to_copy = available.min(catch_up_bytes);
                if to_copy > 0 {
                    catch_up[..to_copy].copy_from_slice(&guard[active.offset..active.offset + to_copy]);
                    active.offset += to_copy;
                    audio_used = to_copy;
                }
                let is_complete = active.complete.load(Ordering::Acquire);
                if active.offset >= guard.len() && is_complete {
                    should_clear_active = true;
                }
            }

            if should_clear_active {
                if let Some(ref a) = active_audio {
                    let total = a.pcm.lock().unwrap().len();
                    tracing::debug!(
                        "[AUDIO:{}] utterance finished during recovery: played {}B/{}B",
                        stream_id, a.offset, total
                    );
                }
                active_audio = None;
            }

            if write_ticks < skipped_ticks as usize {
                tracing::warn!(
                    "[AUDIO:{}] JITTER RECOVERY: {}ms behind at tick {}, writing {} of {} ticks (capped, {}B audio + {}B silence), skipping {}",
                    stream_id, jitter.as_millis(), tick_count, write_ticks, skipped_ticks,
                    audio_used, catch_up_bytes.saturating_sub(audio_used),
                    skipped_ticks as usize - write_ticks
                );
            } else {
                tracing::warn!(
                    "[AUDIO:{}] JITTER RECOVERY: {}ms behind at tick {}, writing {} ticks ({}B audio + {}B silence)",
                    stream_id, jitter.as_millis(), tick_count, write_ticks,
                    audio_used, catch_up_bytes.saturating_sub(audio_used)
                );
            }

            if fifo.write_all(&catch_up).is_err() {
                if !stop.load(Ordering::Acquire) {
                    tracing::error!("[AUDIO:{}] write error during recovery, exiting", stream_id);
                }
                break;
            }

            next_tick = actual + AUDIO_TICK;
            tick_count += skipped_ticks as u64;
            total_bytes_written += catch_up_bytes as u64;
            continue;
        } else if jitter > JITTER_WARN_THRESHOLD && tick_count > 0 {
            jitter_warn_count += 1;
            // Rate-limit: log every 25th warning, or the first one
            if jitter_warn_count == 1 || jitter_warn_count % 25 == 0 {
                tracing::warn!(
                    "[AUDIO:{}] jitter: tick {} was {}ms late (warning #{}, threshold={}ms)",
                    stream_id, tick_count, jitter.as_millis(), jitter_warn_count,
                    JITTER_WARN_THRESHOLD.as_millis()
                );
            }
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
                    let pcm_len = audio.pcm.lock().unwrap().len();
                    let is_complete = audio.complete.load(Ordering::Acquire);
                    let remaining = q.len();
                    tracing::debug!(
                        "[AUDIO:{}] starting utterance: {}B available, complete={}, queue_depth={}",
                        stream_id, pcm_len, is_complete, remaining
                    );
                    active_audio = Some(ActiveAudio {
                        pcm: audio.pcm,
                        complete: audio.complete,
                        offset: 0,
                    });
                }
            }
        }

        // Write one tick's worth of audio (1764 bytes = 20ms at 44100Hz mono 16-bit)
        // Supports streaming: reads from a growing buffer, writes silence if TTS
        // hasn't produced enough data yet.
        let (data_to_write, should_clear) = if let Some(ref mut active) = active_audio {
            let guard = active.pcm.lock().unwrap();
            let available = guard.len() - active.offset;
            let is_complete = active.complete.load(Ordering::Acquire);

            if available >= AUDIO_BYTES_PER_TICK {
                let start = active.offset;
                let end = start + AUDIO_BYTES_PER_TICK;
                let data = guard[start..end].to_vec();
                active.offset = end;
                let done = active.offset >= guard.len() && is_complete;
                (data, done)
            } else if is_complete {
                if available > 0 {
                    // Last partial chunk — pad with silence
                    let mut chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
                    chunk[..available].copy_from_slice(&guard[active.offset..]);
                    (chunk, true)
                } else {
                    (silence.clone(), true)
                }
            } else {
                // TTS still streaming, not enough data yet — write silence this tick
                (silence.clone(), false)
            }
        } else {
            (silence.clone(), false)
        };

        if should_clear {
            if let Some(ref a) = active_audio {
                let total = a.pcm.lock().unwrap().len();
                let played_ms = (a.offset as f64 / BYTES_PER_SEC * 1000.0) as u64;
                tracing::debug!(
                    "[AUDIO:{}] utterance done: played {}B/{}B ({}ms audio)",
                    stream_id, a.offset, total, played_ms
                );
            }
            active_audio = None;
        }

        let write_result = fifo.write_all(&data_to_write);

        total_bytes_written += AUDIO_BYTES_PER_TICK as u64;

        if write_result.is_err() {
            if !stop.load(Ordering::Acquire) {
                tracing::error!("[AUDIO:{}] write error, exiting", stream_id);
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
                tracing::warn!(
                    "[AUDIO:{}] drift warning: {}ms (written={}, expected={})",
                    stream_id, drift_ms, total_bytes_written, expected_bytes
                );
            }
        }
    }

    drop(fifo);
    tracing::info!(
        "[AUDIO:{}] drain thread exited after {} ticks",
        stream_id, tick_count
    );
}
