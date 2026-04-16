//! Dedicated OS thread: drains audio at 20ms ticks.
//!
//! Audio plays as soon as TTS completes — no A/V sync scheduling.
//! Video is independently delayed by broadcast_delay in the video drain.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::core::config::BYTES_PER_SEC;
use super::{
    QueuedAudio,
    AUDIO_TICK, AUDIO_BYTES_PER_TICK,
    JITTER_WARN_THRESHOLD, JITTER_RECOVERY_THRESHOLD,
    MAX_RECOVERY_TICKS,
    JITTER_WARN_LOG_INTERVAL,
    MAX_AUDIO_QUEUE_DEPTH,
};

// ── Config struct ────────────────────────────────────────

/// Everything needed to start an audio drain thread.
pub(crate) struct AudioDrainConfig {
    pub(crate) stream_id: String,
    pub(crate) audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    pub(crate) fifo_path: String,
    pub(crate) stop: Arc<AtomicBool>,
    /// Continuous host PCM chunks (20ms each) for mixing under TTS audio.
    /// None for source streams (they receive host audio directly in audio_queue).
    pub(crate) host_audio_queue: Option<Arc<StdMutex<VecDeque<Vec<u8>>>>>,
    /// Host volume percentage (0–100). 20 for target streams, 0 for source.
    pub(crate) host_volume_pct: u8,
}

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
struct DrainState {
    stream_id: String,
    fifo: std::fs::File,
    audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    stop: Arc<AtomicBool>,
    silence: Vec<u8>,
    active_audio: Option<ActiveAudio>,
    tick_count: u64,
    next_tick: Instant,
    started: bool,
    jitter_warn_count: u64,
    host_audio_queue: Option<Arc<StdMutex<VecDeque<Vec<u8>>>>>,
    host_volume_pct: u8,
}

impl DrainState {
    /// Main drain loop: wait for first audio → then tick at 20ms.
    fn run(&mut self) {
        tracing::info!("[AUDIO:{}] drain thread started (20ms ticks)", self.stream_id);
        self.wait_for_first_audio();
        loop {
            if self.stop.load(Ordering::Acquire) { break; }
            self.sleep_until_next_tick();
            if self.handle_jitter() { break; }
        }
        self.cleanup_and_log();
    }

    /// Block until first audio arrives in queue. Prevents silence build-up
    /// that causes audio to run 30-45s ahead of video.
    fn wait_for_first_audio(&mut self) {
        tracing::info!("[AUDIO:{}] waiting for first audio before starting ticks...", self.stream_id);
        loop {
            if self.stop.load(Ordering::Acquire) { return; }
            {
                let q = self.audio_queue.lock().unwrap();
                if !q.is_empty() { break; }
            }
            thread::sleep(Duration::from_millis(50));
        }
        // Reset tick anchor to now so no catch-up silence is generated.
        let now = Instant::now();
        self.next_tick = now + AUDIO_TICK;
        self.started = true;
        tracing::info!("[AUDIO:{}] first audio arrived, starting ticks", self.stream_id);
    }

    fn sleep_until_next_tick(&self) {
        let now = Instant::now();
        if self.next_tick > now {
            thread::sleep(self.next_tick - now);
        }
    }

    /// Classify jitter and dispatch. Returns `true` to break.
    fn handle_jitter(&mut self) -> bool {
        let actual = Instant::now();
        let jitter = actual.saturating_duration_since(self.next_tick);
        match classify_jitter(jitter, self.tick_count) {
            JitterLevel::Recovery(j) => self.handle_recovery(j),
            JitterLevel::Warn(j) => { self.log_jitter_warn(j); self.advance_and_tick() }
            JitterLevel::Normal => self.advance_and_tick(),
        }
    }

    fn advance_and_tick(&mut self) -> bool {
        self.next_tick += AUDIO_TICK;
        self.tick_count += 1;
        self.process_tick()
    }

    fn cleanup_and_log(&mut self) {
        drop(std::mem::replace(
            &mut self.fifo,
            std::fs::File::open("/dev/null").unwrap(),
        ));
        tracing::info!(
            "[AUDIO:{}] drain thread exited after {} ticks",
            self.stream_id, self.tick_count
        );
    }

    /// Process a normal tick: start utterance → drain audio → mix host → write.
    fn process_tick(&mut self) -> bool {
        self.try_start_next_utterance();

        let (tts_data, should_clear) = drain_tick_audio(&mut self.active_audio, &self.silence);

        let data = if self.host_volume_pct > 0 {
            mix_with_host_audio(&tts_data, &self.host_audio_queue, self.host_volume_pct)
        } else {
            tts_data
        };

        if should_clear {
            self.log_utterance_done();
            self.active_audio = None;
        }

        if self.write_fifo(&data) {
            return true;
        }

        false
    }

    /// Handle severe jitter: write catch-up data and reset tick anchor.
    fn handle_recovery(&mut self, jitter: Duration) -> bool {
        let actual = self.next_tick + jitter;
        let skipped_ticks = jitter.as_millis() / AUDIO_TICK.as_millis();
        let write_ticks = (skipped_ticks as usize).min(MAX_RECOVERY_TICKS);
        let catch_up_bytes = write_ticks * AUDIO_BYTES_PER_TICK;

        let (buffer, audio_used) = self.build_catchup_buffer(catch_up_bytes);
        let silence_bytes = catch_up_bytes.saturating_sub(audio_used);
        tracing::warn!(
            "[AUDIO:{}] JITTER RECOVERY: {}ms behind at tick {}, writing {} ticks ({}B audio + {}B silence)",
            self.stream_id, jitter.as_millis(), self.tick_count, write_ticks,
            audio_used, silence_bytes
        );

        // Drain stale host audio to stay in sync.
        drain_host_audio_catchup(&self.host_audio_queue, write_ticks);

        let should_break = self.write_catchup_and_advance(&buffer, actual);
        self.tick_count += skipped_ticks as u64;
        should_break
    }

    fn build_catchup_buffer(&mut self, catch_up_bytes: usize) -> (Vec<u8>, usize) {
        let mut buffer = vec![0u8; catch_up_bytes];
        let audio_used = self.copy_active_audio(&mut buffer);
        self.clear_if_complete();
        (buffer, audio_used)
    }

    fn copy_active_audio(&mut self, buffer: &mut [u8]) -> usize {
        let Some(active) = self.active_audio.as_mut() else { return 0 };
        let guard = active.pcm.lock().unwrap();
        let available = guard.len().saturating_sub(active.offset);
        let to_copy = available.min(buffer.len());
        if to_copy > 0 {
            buffer[..to_copy].copy_from_slice(&guard[active.offset..active.offset + to_copy]);
            active.offset += to_copy;
        }
        to_copy
    }

    fn clear_if_complete(&mut self) {
        let should_clear = self.active_audio.as_ref().is_some_and(|a| {
            let len = a.pcm.lock().unwrap().len();
            a.offset >= len && a.complete.load(Ordering::Acquire)
        });
        if should_clear {
            self.log_utterance_done();
            self.active_audio = None;
        }
    }

    fn write_catchup_and_advance(&mut self, buffer: &[u8], actual: Instant) -> bool {
        if self.fifo.write_all(buffer).is_err() {
            if !self.stop.load(Ordering::Acquire) {
                tracing::error!("[AUDIO:{}] write error during recovery, exiting", self.stream_id);
            }
            return true;
        }
        self.next_tick = actual + AUDIO_TICK;
        false
    }

    /// Pop next utterance immediately — no play_at scheduling, no pacing.
    fn try_start_next_utterance(&mut self) {
        if self.active_audio.is_some() { return; }
        let queue_arc = self.audio_queue.clone();
        let mut q = queue_arc.lock().unwrap();
        evict_overflow_items(&self.stream_id, &mut q);
        if let Some(audio) = q.pop_front() {
            let pcm_len = audio.pcm.lock().unwrap().len();
            let is_complete = audio.complete.load(Ordering::Acquire);
            tracing::debug!(
                "[AUDIO:{}] starting utterance: {}B available, complete={}, queue_depth={}",
                self.stream_id, pcm_len, is_complete, q.len()
            );
            self.active_audio = Some(ActiveAudio {
                pcm: audio.pcm,
                complete: audio.complete,
                offset: 0,
            });
        }
    }

    fn write_fifo(&mut self, data: &[u8]) -> bool {
        if self.fifo.write_all(data).is_err() {
            if !self.stop.load(Ordering::Acquire) {
                tracing::error!("[AUDIO:{}] write error, exiting", self.stream_id);
            }
            return true;
        }
        false
    }

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

    fn log_utterance_done(&self) {
        if let Some(a) = self.active_audio.as_ref() {
            let total = a.pcm.lock().unwrap().len();
            let played_ms = (a.offset as f64 / BYTES_PER_SEC * 1000.0) as u64;
            tracing::debug!(
                "[AUDIO:{}] utterance finished: played {}B/{}B ({}ms audio)",
                self.stream_id, a.offset, total, played_ms
            );
        }
    }
}

fn classify_jitter(jitter: Duration, tick_count: u64) -> JitterLevel {
    if jitter > JITTER_RECOVERY_THRESHOLD && tick_count > 0 {
        JitterLevel::Recovery(jitter)
    } else if jitter > JITTER_WARN_THRESHOLD && tick_count > 0 {
        JitterLevel::Warn(jitter)
    } else {
        JitterLevel::Normal
    }
}

/// If queue exceeds MAX_AUDIO_QUEUE_DEPTH, drop oldest complete items.
fn evict_overflow_items(stream_id: &str, q: &mut VecDeque<QueuedAudio>) {
    let mut dropped = 0usize;
    while q.len() > MAX_AUDIO_QUEUE_DEPTH {
        let oldest_complete = q.iter().position(|item| {
            item.complete.load(Ordering::Acquire)
        });
        match oldest_complete {
            Some(idx) => { q.remove(idx); dropped += 1; }
            None => break,
        }
    }
    if dropped > 0 {
        tracing::warn!(
            "[AUDIO:{}] evicted {} overflow utterance(s), queue_depth={}",
            stream_id, dropped, q.len()
        );
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
        drain_full_tick(&guard, &mut active.offset, is_complete)
    } else if is_complete {
        (drain_final_partial(&guard, active.offset, available, silence), true)
    } else {
        (silence.to_vec(), false) // TTS still streaming, not enough data yet
    }
}

fn drain_full_tick(guard: &[u8], offset: &mut usize, is_complete: bool) -> (Vec<u8>, bool) {
    let data = guard[*offset..*offset + AUDIO_BYTES_PER_TICK].to_vec();
    *offset += AUDIO_BYTES_PER_TICK;
    let done = *offset >= guard.len() && is_complete;
    (data, done)
}

fn drain_final_partial(guard: &[u8], offset: usize, available: usize, silence: &[u8]) -> Vec<u8> {
    if available > 0 {
        let mut chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
        chunk[..available].copy_from_slice(&guard[offset..]);
        chunk
    } else {
        silence.to_vec()
    }
}

// ── Audio Drain Thread ─────────────────────────────────────

/// Dedicated OS thread: drains audio at 20ms ticks.
///
/// Audio plays immediately as queued — no A/V sync delay.
/// Video is independently delayed by per-stream delay_ms in the video drain.
pub(crate) fn audio_drain_loop(config: AudioDrainConfig) {
    let fifo = match open_fifo(&config) {
        Some(f) => f,
        None => return,
    };

    let now = Instant::now();
    let mut state = DrainState {
        stream_id: config.stream_id,
        fifo,
        audio_queue: config.audio_queue,
        stop: config.stop,
        silence: vec![0u8; AUDIO_BYTES_PER_TICK],
        active_audio: None,
        tick_count: 0,
        next_tick: now + AUDIO_TICK,
        started: false,
        jitter_warn_count: 0,
        host_audio_queue: config.host_audio_queue,
        host_volume_pct: config.host_volume_pct,
    };

    state.run();
}

// ── Host audio mixing ─────────────────────────────────────

/// Pop one host PCM chunk and mix it under `tts` at `host_volume_pct`%.
/// If no host chunk is available, returns `tts` unchanged.
fn mix_with_host_audio(
    tts: &[u8],
    host_queue: &Option<Arc<StdMutex<VecDeque<Vec<u8>>>>>,
    host_volume_pct: u8,
) -> Vec<u8> {
    let Some(queue) = host_queue else { return tts.to_vec() };
    let host_chunk = queue.lock().unwrap().pop_front();
    match host_chunk {
        Some(host) => mix_pcm_samples(tts, &host, host_volume_pct as i32),
        None => tts.to_vec(),
    }
}

/// Mix two s16le mono buffers. `tts` at 100%, `host` at `host_vol`%.
/// Samples are clamped to i16 range to prevent clipping.
fn mix_pcm_samples(tts: &[u8], host: &[u8], host_vol: i32) -> Vec<u8> {
    let n = tts.len() / 2;
    let mut result = Vec::with_capacity(tts.len());
    for i in 0..n {
        let t = i16::from_le_bytes([tts[i * 2], tts[i * 2 + 1]]) as i32;
        let h = if i * 2 + 1 < host.len() {
            i16::from_le_bytes([host[i * 2], host[i * 2 + 1]]) as i32
        } else {
            0
        };
        let mixed = (t + h * host_vol / 100).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        let bytes = mixed.to_le_bytes();
        result.push(bytes[0]);
        result.push(bytes[1]);
    }
    result
}

/// During jitter recovery, discard stale host chunks to stay aligned.
fn drain_host_audio_catchup(
    host_queue: &Option<Arc<StdMutex<VecDeque<Vec<u8>>>>>,
    ticks: usize,
) {
    let Some(queue) = host_queue else { return };
    let mut guard = queue.lock().unwrap();
    let to_drain = ticks.min(guard.len());
    for _ in 0..to_drain {
        guard.pop_front();
    }
}

fn open_fifo(config: &AudioDrainConfig) -> Option<std::fs::File> {
    tracing::info!("[AUDIO:{}] opening FIFO (blocks until FFmpeg reads)...", config.stream_id);
    match std::fs::OpenOptions::new().write(true).open(&config.fifo_path) {
        Ok(f) => {
            tracing::info!("[AUDIO:{}] FIFO opened", config.stream_id);
            Some(f)
        }
        Err(e) => {
            if !config.stop.load(Ordering::Acquire) {
                tracing::error!("[AUDIO:{}] failed to open FIFO: {}", config.stream_id, e);
            }
            None
        }
    }
}
