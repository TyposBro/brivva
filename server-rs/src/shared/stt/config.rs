//! STT configuration: thresholds, timing, and per-language settings.

use std::time::Duration;

// ── Prosody Extraction ──────────────────────────────────

pub const PITCH_MIN_HZ: f32 = 50.0;
pub const PITCH_MAX_HZ: f32 = 500.0;
pub const AUTOCORRELATION_THRESHOLD: f32 = 0.3;
pub const FRAME_DURATION_SECS: f32 = 0.03;
pub const HOP_DURATION_SECS: f32 = 0.01;
pub const MIN_AUDIO_BYTES: usize = 640;
pub const MIN_DURATION_SECS: f32 = 0.1;
pub const PAUSE_ENERGY_FRACTION: f32 = 0.1;

// ── Emotion Classification ──────────────────────────────

pub const LOUD_ENERGY: f32 = 0.035;
pub const QUIET_ENERGY: f32 = 0.018;
pub const EXPRESSIVE_PITCH_STD: f32 = 85.0;
pub const MONOTONE_PITCH_STD: f32 = 55.0;
pub const HIGH_PITCH_MEAN: f32 = 200.0;
pub const HESITANT_PAUSE_DENSITY: f32 = 0.4;

// ── Adaptive Endpointing ────────────────────────────────

pub const ADAPTIVE_SAMPLE_COUNT: usize = 5;
pub const FAST_WPM_THRESHOLD: f32 = 180.0;
pub const SLOW_WPM_THRESHOLD: f32 = 120.0;
pub const MAX_VALID_WPM: u32 = 500;

pub const DEFAULT_ENDPOINTING_SECS: f64 = 0.25;
pub const DEFAULT_MAX_DURATION_SECS: f64 = 5.0;
pub const FAST_ENDPOINTING_SECS: f64 = 0.20;
pub const FAST_MAX_DURATION_SECS: f64 = 5.0;
pub const SLOW_ENDPOINTING_SECS: f64 = 0.35;
pub const SLOW_MAX_DURATION_SECS: f64 = 10.0;

// ── Gladia Connection ───────────────────────────────────

pub const INITIAL_CONNECT_MAX_ATTEMPTS: u32 = 10;
pub const RECONNECT_RETRY_DELAY_SECS: u64 = 3;
pub const NORMALIZED_MATCH_THRESHOLD_PCT: usize = 80;
pub const MIN_CHARS_FOR_FORCE_SPLIT: usize = 4;

// ── Per-Language Detector Config ────────────────────────

pub struct LangDetectorConfig {
    pub markers: &'static [&'static str],
    pub min_chars_after: usize,
    pub min_duration: Duration,
    pub max_duration: Duration,
}

pub struct ProgressiveLangConfig {
    pub markers: &'static [&'static str],
    pub min_chars_after: usize,
    pub min_duration: Duration,
    pub max_duration: Duration,
}

// ── Source-Lang Passthrough ─────────────────────────────

pub const PASSTHROUGH_PADDING_SECS: f64 = 2.0;

#[cfg(test)]
mod tests {
    use super::*;

    // ── Prosody Extraction Constants ────────────────────────

    #[test]
    fn should_have_pitch_min_less_than_pitch_max() {
        assert!(PITCH_MIN_HZ < PITCH_MAX_HZ);
    }

    #[test]
    fn should_have_positive_autocorrelation_threshold() {
        assert!(AUTOCORRELATION_THRESHOLD > 0.0);
        assert!(AUTOCORRELATION_THRESHOLD < 1.0);
    }

    #[test]
    fn should_have_frame_duration_greater_than_hop_duration() {
        assert!(FRAME_DURATION_SECS > HOP_DURATION_SECS);
    }

    #[test]
    fn should_have_positive_min_audio_bytes() {
        assert!(MIN_AUDIO_BYTES > 0);
    }

    #[test]
    fn should_have_positive_min_duration_secs() {
        assert!(MIN_DURATION_SECS > 0.0);
    }

    #[test]
    fn should_have_pause_energy_fraction_between_zero_and_one() {
        assert!(PAUSE_ENERGY_FRACTION > 0.0);
        assert!(PAUSE_ENERGY_FRACTION < 1.0);
    }

    // ── Emotion Classification Constants ────────────────────

    #[test]
    fn should_have_loud_energy_greater_than_quiet_energy() {
        assert!(LOUD_ENERGY > QUIET_ENERGY);
    }

    #[test]
    fn should_have_expressive_pitch_std_greater_than_monotone() {
        assert!(EXPRESSIVE_PITCH_STD > MONOTONE_PITCH_STD);
    }

    #[test]
    fn should_have_hesitant_pause_density_between_zero_and_one() {
        assert!(HESITANT_PAUSE_DENSITY > 0.0);
        assert!(HESITANT_PAUSE_DENSITY <= 1.0);
    }

    // ── Adaptive Endpointing Constants ──────────────────────

    #[test]
    fn should_have_fast_wpm_greater_than_slow_wpm() {
        assert!(FAST_WPM_THRESHOLD > SLOW_WPM_THRESHOLD);
    }

    #[test]
    fn should_have_max_valid_wpm_above_fast_threshold() {
        assert!((MAX_VALID_WPM as f32) > FAST_WPM_THRESHOLD);
    }

    #[test]
    fn should_have_fast_endpointing_less_than_slow_endpointing() {
        assert!(FAST_ENDPOINTING_SECS < SLOW_ENDPOINTING_SECS);
    }

    #[test]
    fn should_have_default_endpointing_between_fast_and_slow() {
        assert!(DEFAULT_ENDPOINTING_SECS >= FAST_ENDPOINTING_SECS);
        assert!(DEFAULT_ENDPOINTING_SECS <= SLOW_ENDPOINTING_SECS);
    }

    #[test]
    fn should_have_slow_max_duration_greater_than_fast_max_duration() {
        assert!(SLOW_MAX_DURATION_SECS > FAST_MAX_DURATION_SECS);
    }

    #[test]
    fn should_have_positive_adaptive_sample_count() {
        assert!(ADAPTIVE_SAMPLE_COUNT > 0);
    }

    // ── Gladia Connection Constants ────────────────────────

    #[test]
    fn should_have_positive_initial_connect_max_attempts() {
        assert!(INITIAL_CONNECT_MAX_ATTEMPTS > 0);
    }

    #[test]
    fn should_have_positive_reconnect_retry_delay() {
        assert!(RECONNECT_RETRY_DELAY_SECS > 0);
    }

    #[test]
    fn should_have_normalized_match_threshold_within_percent_range() {
        assert!(NORMALIZED_MATCH_THRESHOLD_PCT > 0);
        assert!(NORMALIZED_MATCH_THRESHOLD_PCT <= 100);
    }

    #[test]
    fn should_have_positive_min_chars_for_force_split() {
        assert!(MIN_CHARS_FOR_FORCE_SPLIT > 0);
    }

    // ── Source-Lang Passthrough ────────────────────────────

    #[test]
    fn should_have_positive_passthrough_padding() {
        assert!(PASSTHROUGH_PADDING_SECS > 0.0);
    }
}
