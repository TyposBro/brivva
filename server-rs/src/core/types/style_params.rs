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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_default_speed_to_one() {
        let params = StyleParams::default();

        assert!((params.speed - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn should_default_emotion_to_neutral() {
        let params = StyleParams::default();

        assert_eq!(params.emotion, "neutral");
    }

    #[test]
    fn should_deserialize_full_json() {
        let json = r#"{"speed": 1.5, "emotion": "happy"}"#;

        let params: StyleParams = serde_json::from_str(json).unwrap();

        assert!((params.speed - 1.5).abs() < f64::EPSILON);
        assert_eq!(params.emotion, "happy");
    }

    #[test]
    fn should_fallback_speed_when_missing_from_json() {
        let json = r#"{"emotion": "sad"}"#;

        let params: StyleParams = serde_json::from_str(json).unwrap();

        assert!((params.speed - 1.0).abs() < f64::EPSILON);
        assert_eq!(params.emotion, "sad");
    }

    #[test]
    fn should_fallback_emotion_when_missing_from_json() {
        let json = r#"{"speed": 2.0}"#;

        let params: StyleParams = serde_json::from_str(json).unwrap();

        assert!((params.speed - 2.0).abs() < f64::EPSILON);
        assert_eq!(params.emotion, "neutral");
    }

    #[test]
    fn should_fallback_all_fields_when_json_is_empty_object() {
        let json = r#"{}"#;

        let params: StyleParams = serde_json::from_str(json).unwrap();

        assert!((params.speed - 1.0).abs() < f64::EPSILON);
        assert_eq!(params.emotion, "neutral");
    }

    #[test]
    fn should_roundtrip_through_serde() {
        let original = StyleParams { speed: 0.75, emotion: "excited".to_string() };

        let json = serde_json::to_string(&original).unwrap();
        let restored: StyleParams = serde_json::from_str(&json).unwrap();

        assert!((restored.speed - 0.75).abs() < f64::EPSILON);
        assert_eq!(restored.emotion, "excited");
    }

    #[test]
    fn should_serialize_to_json_with_both_fields() {
        let params = StyleParams { speed: 2.5, emotion: "angry".to_string() };

        let json: serde_json::Value = serde_json::to_value(&params).unwrap();

        assert_eq!(json["speed"], 2.5);
        assert_eq!(json["emotion"], "angry");
    }

    #[test]
    fn should_preserve_zero_speed_through_serde() {
        let params = StyleParams { speed: 0.0, emotion: "neutral".to_string() };

        let json = serde_json::to_string(&params).unwrap();
        let restored: StyleParams = serde_json::from_str(&json).unwrap();

        assert!((restored.speed - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn should_preserve_empty_emotion_through_serde() {
        let params = StyleParams { speed: 1.0, emotion: String::new() };

        let json = serde_json::to_string(&params).unwrap();
        let restored: StyleParams = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.emotion, "");
    }

    #[test]
    fn should_reject_invalid_json_type_for_speed() {
        let json = r#"{"speed": "fast", "emotion": "happy"}"#;

        let result = serde_json::from_str::<StyleParams>(json);

        assert!(result.is_err());
    }
}
