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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_cap_deadline_at_hard_limit() {
        let deadline = compute_tts_deadline(20_000);

        assert_eq!(deadline, Duration::from_millis(TTS_DEADLINE_CAP_MS));
    }

    #[test]
    fn should_subtract_margin_from_delay() {
        let deadline = compute_tts_deadline(3000);

        assert_eq!(deadline, Duration::from_millis(3000 - TTS_DEADLINE_MARGIN_MS));
    }

    #[test]
    fn should_not_underflow_on_small_delay() {
        let deadline = compute_tts_deadline(100);

        assert_eq!(deadline, Duration::from_millis(0));
    }

    #[test]
    fn should_compute_pcm_bytes_from_duration() {
        let start = Instant::now();
        let end = start + Duration::from_secs(1);

        let bytes = compute_max_pcm_bytes(start, end, 0.5);

        let expected = (1.5 * BYTES_PER_SEC) as usize;
        assert_eq!(bytes, expected);
    }

    #[test]
    fn should_compute_streaming_max_bytes() {
        let bytes = compute_streaming_max_bytes(3000, 5.0);

        let expected = (8.0 * BYTES_PER_SEC) as usize;
        assert_eq!(bytes, expected);
    }
}
