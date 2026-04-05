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
    DRIFT_CHECK_INTERVAL_TICKS, DRIFT_WARN_THRESHOLD_MS,
    JITTER_WARN_LOG_INTERVAL,
};

/// State for draining queued audio chunk-by-chunk
struct ActiveAudio {
    pcm: Arc<StdMutex<Vec<u8>>>,
    complete: Arc<AtomicBool>,
    offset: usize,
}

enum JitterLevel {
    Recovery(Duration),
    Warn(Duration),
    Normal,
}

/// Holds all mutable state for the audio drain loop.
///
/// Methods on this struct replace free functions that previously took 8-10 params.
struct DrainState {
    stream_id: String,
    fifo: std::fs::File,
    audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    stop: Arc<AtomicBool>,
    delay: Duration,
    silence: Vec<u8>,
    active_audio: Option<ActiveAudio>,
    tick_count: u64,
    next_tick: Instant,
    start_time: Instant,
    total_bytes_written: u64,
    jitter_warn_count: u64,
}

impl DrainState {
    /// Main drain loop: check stop → sleep → classify jitter → handle.
    fn run(&mut self) {
        tracing::info!(
            "[AUDIO:{}] drain thread started (20ms ticks, {}ms delay)",
            self.stream_id,
            self.delay.as_millis()
        );

        loop {
            if self.stop.load(Ordering::Acquire) {
                break;
            }

            let now = Instant::now();
            if self.next_tick > now {
                thread::sleep(self.next_tick - now);
            }

            let actual = Instant::now();
            let jitter = actual.saturating_duration_since(self.next_tick);

            match classify_jitter(jitter, self.tick_count) {
                JitterLevel::Recovery(jitter) => {
                    if self.handle_recovery(jitter) {
                        break;
                    }
                    continue;
                }
                JitterLevel::Warn(jitter) => {
                    self.log_jitter_warn(jitter);
                }
                JitterLevel::Normal => {}
            }

            self.next_tick += AUDIO_TICK;
            self.tick_count += 1;

            if self.process_tick(actual) {
                break;
            }
        }

        drop(std::mem::replace(
            &mut self.fifo,
            // This is never used — we're about to return from audio_drain_loop.
            // We need to move `self.fifo` out so its Drop runs, but struct fields
            // can't be partially moved. Replace with /dev/null as a dummy.
            std::fs::File::open("/dev/null").unwrap(),
        ));
        tracing::info!(
            "[AUDIO:{}] drain thread exited after {} ticks",
            self.stream_id, self.tick_count
        );
    }

    /// Process a normal tick: start utterance → drain audio → write → drift check.
    /// Returns `true` if the loop should break (write error).
    fn process_tick(&mut self, actual: Instant) -> bool {
        let target_ts = actual - self.delay;
        self.try_start_next_utterance(target_ts);

        let (data, should_clear) = drain_tick_audio(&mut self.active_audio, &self.silence);

        if should_clear {
            self.log_utterance_done("");
            self.active_audio = None;
        }

        if self.write_fifo(&data) {
            return true;
        }

        self.total_bytes_written += AUDIO_BYTES_PER_TICK as u64;
        self.check_drift();
        false
    }

    /// Handle severe jitter: write catch-up data and reset tick anchor.
    /// Returns `true` if the loop should break (write error).
    fn handle_recovery(&mut self, jitter: Duration) -> bool {
        let actual = self.next_tick + jitter;
        let skipped_ticks = jitter.as_millis() / AUDIO_TICK.as_millis();
        let write_ticks = (skipped_ticks as usize).min(MAX_RECOVERY_TICKS);
        let catch_up_bytes = write_ticks * AUDIO_BYTES_PER_TICK;

        let (buffer, audio_used) = self.build_catchup_buffer(catch_up_bytes);
        self.log_recovery(jitter, write_ticks, skipped_ticks, audio_used, catch_up_bytes);

        let should_break = self.write_catchup_and_advance(&buffer, actual);
        self.tick_count += skipped_ticks as u64;
        should_break
    }

    /// Build catch-up buffer: drain any active audio first, fill rest with silence.
    fn build_catchup_buffer(&mut self, catch_up_bytes: usize) -> (Vec<u8>, usize) {
        let mut buffer = vec![0u8; catch_up_bytes];
        let mut audio_used = 0usize;
        let mut should_clear = false;

        if let Some(active) = self.active_audio.as_mut() {
            let guard = active.pcm.lock().unwrap();
            let available = guard.len().saturating_sub(active.offset);
            let to_copy = available.min(catch_up_bytes);
            if to_copy > 0 {
                buffer[..to_copy]
                    .copy_from_slice(&guard[active.offset..active.offset + to_copy]);
                active.offset += to_copy;
                audio_used = to_copy;
            }
            let is_complete = active.complete.load(Ordering::Acquire);
            if active.offset >= guard.len() && is_complete {
                should_clear = true;
            }
        }

        if should_clear {
            self.log_utterance_done("during recovery");
            self.active_audio = None;
        }

        (buffer, audio_used)
    }

    /// Write catch-up buffer and advance the tick anchor.
    /// Returns `true` if the loop should break (write error).
    fn write_catchup_and_advance(&mut self, buffer: &[u8], actual: Instant) -> bool {
        if self.fifo.write_all(buffer).is_err() {
            if !self.stop.load(Ordering::Acquire) {
                tracing::error!(
                    "[AUDIO:{}] write error during recovery, exiting",
                    self.stream_id
                );
            }
            return true;
        }

        self.next_tick = actual + AUDIO_TICK;
        self.total_bytes_written += buffer.len() as u64;
        false
    }

    /// Log recovery details (capped vs uncapped).
    fn log_recovery(
        &self,
        jitter: Duration,
        write_ticks: usize,
        skipped_ticks: u128,
        audio_used: usize,
        catch_up_bytes: usize,
    ) {
        let silence_bytes = catch_up_bytes.saturating_sub(audio_used);
        if write_ticks < skipped_ticks as usize {
            tracing::warn!(
                "[AUDIO:{}] JITTER RECOVERY: {}ms behind at tick {}, writing {} of {} ticks (capped, {}B audio + {}B silence), skipping {}",
                self.stream_id, jitter.as_millis(), self.tick_count, write_ticks, skipped_ticks,
                audio_used, silence_bytes,
                skipped_ticks as usize - write_ticks
            );
        } else {
            tracing::warn!(
                "[AUDIO:{}] JITTER RECOVERY: {}ms behind at tick {}, writing {} ticks ({}B audio + {}B silence)",
                self.stream_id, jitter.as_millis(), self.tick_count, write_ticks,
                audio_used, silence_bytes
            );
        }
    }

    /// Try to pop next utterance from queue if ready.
    fn try_start_next_utterance(&mut self, target_ts: Instant) {
        if self.active_audio.is_some() {
            return;
        }
        let mut q = self.audio_queue.lock().unwrap();
        if let Some(front) = q.front()
            && target_ts >= front.play_at
        {
            let audio = q.pop_front().unwrap();
            let pcm_len = audio.pcm.lock().unwrap().len();
            let is_complete = audio.complete.load(Ordering::Acquire);
            let remaining = q.len();
            tracing::debug!(
                "[AUDIO:{}] starting utterance: {}B available, complete={}, queue_depth={}",
                self.stream_id, pcm_len, is_complete, remaining
            );
            self.active_audio = Some(ActiveAudio {
                pcm: audio.pcm,
                complete: audio.complete,
                offset: 0,
            });
        }
    }

    /// Write data to the FIFO. Returns `true` on error (caller should break).
    fn write_fifo(&mut self, data: &[u8]) -> bool {
        if self.fifo.write_all(data).is_err() {
            if !self.stop.load(Ordering::Acquire) {
                tracing::error!("[AUDIO:{}] write error, exiting", self.stream_id);
            }
            return true;
        }
        false
    }

    /// Periodic drift check (every ~5 seconds = 250 ticks at 20ms).
    fn check_drift(&self) {
        if !self.tick_count.is_multiple_of(DRIFT_CHECK_INTERVAL_TICKS) {
            return;
        }
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let expected_bytes = (elapsed * BYTES_PER_SEC) as u64;
        let drift_bytes =
            (self.total_bytes_written as i64 - expected_bytes as i64).unsigned_abs();
        let drift_ms = (drift_bytes as f64 / BYTES_PER_SEC * 1000.0) as u64;
        if drift_ms > DRIFT_WARN_THRESHOLD_MS {
            tracing::warn!(
                "[AUDIO:{}] drift warning: {}ms (written={}, expected={})",
                self.stream_id, drift_ms, self.total_bytes_written, expected_bytes
            );
        }
    }

    /// Rate-limited jitter warning log.
    fn log_jitter_warn(&mut self, jitter: Duration) {
        self.jitter_warn_count += 1;
        if self.jitter_warn_count == 1
            || self.jitter_warn_count.is_multiple_of(JITTER_WARN_LOG_INTERVAL)
        {
            tracing::warn!(
                "[AUDIO:{}] jitter: tick {} was {}ms late (warning #{}, threshold={}ms)",
                self.stream_id, self.tick_count, jitter.as_millis(), self.jitter_warn_count,
                JITTER_WARN_THRESHOLD.as_millis()
            );
        }
    }

    /// Log that the current utterance finished.
    fn log_utterance_done(&self, context: &str) {
        if let Some(a) = self.active_audio.as_ref() {
            let total = a.pcm.lock().unwrap().len();
            let played_ms = (a.offset as f64 / BYTES_PER_SEC * 1000.0) as u64;
            tracing::debug!(
                "[AUDIO:{}] utterance finished {}: played {}B/{}B ({}ms audio)",
                self.stream_id, context, a.offset, total, played_ms
            );
        }
    }
}

/// Classify jitter level based on threshold constants.
fn classify_jitter(jitter: Duration, tick_count: u64) -> JitterLevel {
    if jitter > JITTER_RECOVERY_THRESHOLD && tick_count > 0 {
        JitterLevel::Recovery(jitter)
    } else if jitter > JITTER_WARN_THRESHOLD && tick_count > 0 {
        JitterLevel::Warn(jitter)
    } else {
        JitterLevel::Normal
    }
}

/// Drain one tick of audio from the active utterance, or produce silence.
fn drain_tick_audio(
    active_audio: &mut Option<ActiveAudio>,
    silence: &[u8],
) -> (Vec<u8>, bool) {
    let Some(active) = active_audio.as_mut() else {
        return (silence.to_vec(), false);
    };

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
            let mut chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
            chunk[..available].copy_from_slice(&guard[active.offset..]);
            (chunk, true)
        } else {
            (silence.to_vec(), true)
        }
    } else {
        // TTS still streaming, not enough data yet — write silence this tick
        (silence.to_vec(), false)
    }
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
    let fifo = match open_fifo(&stream_id, &fifo_path, &stop) {
        Some(f) => f,
        None => return,
    };

    let now = Instant::now();
    let mut state = DrainState {
        stream_id,
        fifo,
        audio_queue,
        stop,
        delay,
        silence: vec![0u8; AUDIO_BYTES_PER_TICK],
        active_audio: None,
        tick_count: 0,
        next_tick: now + AUDIO_TICK,
        start_time: now,
        total_bytes_written: 0,
        jitter_warn_count: 0,
    };

    state.run();
}

/// Open FIFO for writing (blocks until FFmpeg opens it for reading).
fn open_fifo(
    stream_id: &str,
    fifo_path: &str,
    stop: &AtomicBool,
) -> Option<std::fs::File> {
    tracing::info!("[AUDIO:{}] opening FIFO (blocks until FFmpeg reads)...", stream_id);
    match std::fs::OpenOptions::new().write(true).open(fifo_path) {
        Ok(f) => {
            tracing::info!("[AUDIO:{}] FIFO opened", stream_id);
            Some(f)
        }
        Err(e) => {
            if !stop.load(Ordering::Acquire) {
                tracing::error!("[AUDIO:{}] failed to open FIFO: {}", stream_id, e);
            }
            None
        }
    }
}
