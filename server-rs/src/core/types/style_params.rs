//! TTS style parameters — pure value object used by STT prosody and TTS voice settings.

use serde::{Deserialize, Serialize};

/// TTS style parameters mapped from prosody/emotion analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleParams {
    #[serde(default = "default_speed")]
    pub speed: f64,
    #[serde(default = "default_emotion")]
    pub emotion: String,
}

fn default_speed() -> f64 { 1.0 }
fn default_emotion() -> String { "neutral".to_string() }

impl Default for StyleParams {
    fn default() -> Self {
        Self { speed: 1.0, emotion: "neutral".to_string() }
    }
}
