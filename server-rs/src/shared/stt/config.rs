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
