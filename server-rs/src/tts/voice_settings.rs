//! Voice style parameters for ElevenLabs TTS.
//!
//! Maps emotion labels from prosody analysis to ElevenLabs voice_settings.
//! Single source of truth — replaces duplicated map_style() calls across modules.

/// Named voice style parameters for ElevenLabs TTS.
#[derive(Debug, Clone)]
pub struct VoiceStyle {
    pub stability: f64,
    pub similarity_boost: f64,
    pub style: f64,
    pub speed: f64,
}

impl VoiceStyle {
    /// Map an emotion label to ElevenLabs voice style parameters.
    pub fn from_emotion(emotion: &str) -> Self {
        match emotion {
            "excited" => Self { stability: 0.20, similarity_boost: 0.50, style: 0.90, speed: 1.20 },
            "happy"   => Self { stability: 0.30, similarity_boost: 0.60, style: 0.70, speed: 1.10 },
            "angry"   => Self { stability: 0.25, similarity_boost: 0.70, style: 0.85, speed: 1.05 },
            "sad"     => Self { stability: 0.70, similarity_boost: 0.80, style: 0.40, speed: 0.85 },
            "serious" => Self { stability: 0.60, similarity_boost: 0.80, style: 0.30, speed: 0.95 },
            _         => Self { stability: 0.50, similarity_boost: 0.75, style: 0.00, speed: 1.00 },
        }
    }

    /// Build the ElevenLabs voice_settings JSON object.
    /// Combines style parameters with a custom speed override.
    pub fn to_voice_settings(&self, speed: f64) -> serde_json::Value {
        serde_json::json!({
            "stability": self.stability,
            "similarity_boost": self.similarity_boost,
            "style": self.style,
            "speed": speed,
        })
    }
}
