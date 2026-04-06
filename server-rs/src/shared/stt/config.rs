//! STT configuration: thresholds, timing, and per-language settings.

// ── Audio Accumulator ──────────────────────────────────

/// Max bytes retained in the audio accumulator (3s at 44.1kHz 16-bit mono).
/// Prosody analysis only needs the last few seconds; keeping more wastes RAM.
pub const MAX_AUDIO_ACC_BYTES: usize = 3 * 88_200;

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

// ── Soniox Connection ──────────────────────────────────

pub const SONIOX_WS_URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
pub const SONIOX_MODEL: &str = "stt-rt-v4";
pub const SONIOX_MAX_ENDPOINT_DELAY_MS: u64 = 1500;
pub const SONIOX_KEEPALIVE_INTERVAL_SECS: u64 = 15;
pub const SONIOX_CONNECT_MAX_ATTEMPTS: u32 = 10;
pub const SONIOX_CONNECT_RETRY_DELAY_SECS: u64 = 2;

// ── Progressive Chunking ──────────────────────────────

/// Force-emit partial translation if utterance exceeds this duration
/// without a semantic endpoint. Prevents long continuous speech from
/// accumulating into a single giant chunk.
pub const FORCE_CHUNK_AFTER_SECS: u64 = 4;

// ── Source-Lang Passthrough ─────────────────────────────

pub const PASSTHROUGH_PADDING_SECS: f64 = 2.0;

#[cfg(test)]
mod tests {
    use super::*;

    // ── Audio Accumulator Constants ────────────────────────

    #[test]
    fn should_have_positive_max_audio_acc_bytes() {
        assert!(MAX_AUDIO_ACC_BYTES > 0);
    }

    #[test]
    fn should_cap_audio_acc_to_a_few_seconds() {
        let bytes_per_sec = crate::core::config::BYTES_PER_SEC as usize;
        let secs = MAX_AUDIO_ACC_BYTES / bytes_per_sec;
        assert!(secs >= 1 && secs <= 10);
    }

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

    // ── Soniox Connection Constants ────────────────────────

    #[test]
    fn should_have_positive_connect_max_attempts() {
        assert!(SONIOX_CONNECT_MAX_ATTEMPTS > 0);
    }

    #[test]
    fn should_have_positive_connect_retry_delay() {
        assert!(SONIOX_CONNECT_RETRY_DELAY_SECS > 0);
    }

    #[test]
    fn should_have_positive_keepalive_interval() {
        assert!(SONIOX_KEEPALIVE_INTERVAL_SECS > 0);
    }

    #[test]
    fn should_have_positive_max_endpoint_delay() {
        assert!(SONIOX_MAX_ENDPOINT_DELAY_MS > 0);
    }

    #[test]
    fn should_have_valid_soniox_ws_url() {
        assert!(SONIOX_WS_URL.starts_with("wss://"));
    }

    // ── Source-Lang Passthrough ────────────────────────────

    #[test]
    fn should_have_positive_passthrough_padding() {
        assert!(PASSTHROUGH_PADDING_SECS > 0.0);
    }
}
