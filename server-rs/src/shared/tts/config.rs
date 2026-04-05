//! TTS configuration: response types and protocol constants.

use serde::Deserialize;

/// ElevenLabs WebSocket response shape.
#[derive(Debug, Deserialize)]
pub(super) struct ElevenLabsTtsResponse {
    #[serde(default)]
    pub audio: Option<String>,
    #[serde(default, rename = "isFinal")]
    pub is_final: Option<bool>,
}

/// ElevenLabs low-latency chunk schedule length.
pub const CHUNK_LENGTH_SCHEDULE: u32 = 50;

#[cfg(test)]
mod tests {
    use super::*;

    // ── deserialization ──────────────────────────────────

    #[test]
    fn should_deserialize_response_with_audio() {
        let json = r#"{"audio": "YWJj", "isFinal": false}"#;

        let resp: ElevenLabsTtsResponse = serde_json::from_str(json).unwrap();

        assert_eq!(resp.audio, Some("YWJj".to_string()));
        assert_eq!(resp.is_final, Some(false));
    }

    #[test]
    fn should_deserialize_final_response_without_audio() {
        let json = r#"{"isFinal": true}"#;

        let resp: ElevenLabsTtsResponse = serde_json::from_str(json).unwrap();

        assert!(resp.audio.is_none());
        assert_eq!(resp.is_final, Some(true));
    }

    #[test]
    fn should_deserialize_empty_object_with_defaults() {
        let json = r#"{}"#;

        let resp: ElevenLabsTtsResponse = serde_json::from_str(json).unwrap();

        assert!(resp.audio.is_none());
        assert!(resp.is_final.is_none());
    }

    // ── constants ────────────────────────────────────────

    #[test]
    fn should_have_positive_chunk_length_schedule() {
        assert!(CHUNK_LENGTH_SCHEDULE > 0);
    }
}
