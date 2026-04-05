//! Shared TTS budget computation.
//!
//! Single source of truth for deadline and max PCM byte calculations
//! used by both the chunked and legacy pipeline paths.

use std::time::{Duration, Instant};
use crate::core::config::{BYTES_PER_SEC, TTS_DEADLINE_CAP_MS, TTS_DEADLINE_MARGIN_MS};

/// Compute the TTS deadline from broadcast delay.
/// Returns the lesser of (delay - margin) and the hard cap.
pub fn compute_tts_deadline(broadcast_delay_ms: u64) -> Duration {
    let sync_deadline = Duration::from_millis(
        broadcast_delay_ms.saturating_sub(TTS_DEADLINE_MARGIN_MS),
    );
    let hard_cap = Duration::from_millis(TTS_DEADLINE_CAP_MS);
    sync_deadline.min(hard_cap)
}

/// Compute max PCM bytes for an utterance duration plus tolerance.
pub fn compute_max_pcm_bytes(utterance_start: Instant, utterance_end: Instant, tolerance_secs: f64) -> usize {
    let duration = utterance_end.duration_since(utterance_start);
    let max_secs = duration.as_secs_f64() + tolerance_secs;
    (max_secs * BYTES_PER_SEC) as usize
}

/// Compute max PCM bytes for streaming chunks (based on broadcast delay).
#[allow(dead_code)]
pub fn compute_streaming_max_bytes(broadcast_delay_ms: u64, padding_secs: f64) -> usize {
    let max_secs = (broadcast_delay_ms as f64 / 1000.0) + padding_secs;
    (max_secs * BYTES_PER_SEC) as usize
}
