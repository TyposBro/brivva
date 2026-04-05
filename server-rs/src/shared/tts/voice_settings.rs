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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_return_excited_style() {
        let style = VoiceStyle::from_emotion("excited");

        assert_eq!(style.stability, 0.20);
        assert_eq!(style.similarity_boost, 0.50);
        assert_eq!(style.style, 0.90);
        assert_eq!(style.speed, 1.20);
    }

    #[test]
    fn should_return_default_for_unknown_emotion() {
        let style = VoiceStyle::from_emotion("confused");

        assert_eq!(style.stability, 0.50);
        assert_eq!(style.speed, 1.00);
        assert_eq!(style.style, 0.00);
    }

    #[test]
    fn should_generate_voice_settings_json_with_custom_speed() {
        let style = VoiceStyle::from_emotion("happy");
        let json = style.to_voice_settings(1.15);

        assert_eq!(json["stability"], 0.30);
        assert_eq!(json["similarity_boost"], 0.60);
        assert_eq!(json["style"], 0.70);
        assert_eq!(json["speed"], 1.15);
    }
}
