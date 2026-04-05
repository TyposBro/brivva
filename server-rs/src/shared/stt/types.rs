//! Gladia response types for STT messages.

// ── Gladia Response Types ────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct GladiaMessage {
    #[serde(rename = "type", default)]
    pub msg_type: String,
    #[serde(default)]
    pub data: Option<GladiaData>,
    #[serde(default)]
    pub error: Option<GladiaError>,
}

#[derive(Debug, serde::Deserialize)]
pub struct GladiaData {
    #[serde(default)]
    pub is_final: bool,
    #[serde(default)]
    pub utterance: Option<GladiaUtterance>,
}

#[derive(Debug, serde::Deserialize)]
pub struct GladiaUtterance {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub confidence: f64,
}

#[derive(Debug, serde::Deserialize)]
pub struct GladiaError {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub status_code: u16,
}

#[derive(Debug, serde::Deserialize)]
pub struct GladiaSession {
    pub id: String,
    pub url: String,
}

impl GladiaMessage {
    /// Extract the transcript text from a transcript message.
    pub fn transcript(&self) -> Option<String> {
        self.data
            .as_ref()
            .and_then(|d| d.utterance.as_ref())
            .map(|u| u.text.trim().to_string())
            .filter(|t| !t.is_empty())
    }

    pub fn is_final(&self) -> bool {
        self.data.as_ref().map(|d| d.is_final).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── transcript() ─────────────────────────────────────

    #[test]
    fn should_extract_transcript_when_utterance_present() {
        let msg = GladiaMessage {
            msg_type: "transcript".into(),
            data: Some(GladiaData {
                is_final: false,
                utterance: Some(GladiaUtterance {
                    text: "hello world".into(),
                    language: "en".into(),
                    confidence: 0.95,
                }),
            }),
            error: None,
        };

        assert_eq!(msg.transcript(), Some("hello world".to_string()));
    }

    #[test]
    fn should_return_none_when_data_is_missing() {
        let msg = GladiaMessage {
            msg_type: "transcript".into(),
            data: None,
            error: None,
        };

        assert_eq!(msg.transcript(), None);
    }

    #[test]
    fn should_return_none_when_utterance_is_missing() {
        let msg = GladiaMessage {
            msg_type: "transcript".into(),
            data: Some(GladiaData {
                is_final: false,
                utterance: None,
            }),
            error: None,
        };

        assert_eq!(msg.transcript(), None);
    }

    #[test]
    fn should_return_none_when_text_is_empty() {
        let msg = GladiaMessage {
            msg_type: "transcript".into(),
            data: Some(GladiaData {
                is_final: false,
                utterance: Some(GladiaUtterance {
                    text: "".into(),
                    language: "en".into(),
                    confidence: 0.0,
                }),
            }),
            error: None,
        };

        assert_eq!(msg.transcript(), None);
    }

    #[test]
    fn should_trim_whitespace_from_transcript() {
        let msg = GladiaMessage {
            msg_type: "transcript".into(),
            data: Some(GladiaData {
                is_final: true,
                utterance: Some(GladiaUtterance {
                    text: "  trimmed  ".into(),
                    language: "en".into(),
                    confidence: 0.9,
                }),
            }),
            error: None,
        };

        assert_eq!(msg.transcript(), Some("trimmed".to_string()));
    }

    #[test]
    fn should_return_none_when_text_is_only_whitespace() {
        let msg = GladiaMessage {
            msg_type: "transcript".into(),
            data: Some(GladiaData {
                is_final: false,
                utterance: Some(GladiaUtterance {
                    text: "   ".into(),
                    language: "en".into(),
                    confidence: 0.5,
                }),
            }),
            error: None,
        };

        assert_eq!(msg.transcript(), None);
    }

    // ── is_final() ───────────────────────────────────────

    #[test]
    fn should_return_true_when_data_is_final() {
        let msg = GladiaMessage {
            msg_type: "transcript".into(),
            data: Some(GladiaData {
                is_final: true,
                utterance: None,
            }),
            error: None,
        };

        assert!(msg.is_final());
    }

    #[test]
    fn should_return_false_when_data_is_not_final() {
        let msg = GladiaMessage {
            msg_type: "transcript".into(),
            data: Some(GladiaData {
                is_final: false,
                utterance: None,
            }),
            error: None,
        };

        assert!(!msg.is_final());
    }

    #[test]
    fn should_return_false_when_data_is_none() {
        let msg = GladiaMessage {
            msg_type: "error".into(),
            data: None,
            error: None,
        };

        assert!(!msg.is_final());
    }

    // ── deserialization ──────────────────────────────────

    #[test]
    fn should_deserialize_full_transcript_message() {
        let json = r#"{
            "type": "transcript",
            "data": {
                "is_final": true,
                "utterance": {
                    "text": "hello",
                    "language": "en",
                    "confidence": 0.98
                }
            }
        }"#;

        let msg: GladiaMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.msg_type, "transcript");
        assert!(msg.is_final());
        assert_eq!(msg.transcript(), Some("hello".to_string()));
    }

    #[test]
    fn should_deserialize_message_with_missing_optional_fields() {
        let json = r#"{"type": "unknown"}"#;

        let msg: GladiaMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.msg_type, "unknown");
        assert!(msg.data.is_none());
        assert!(msg.error.is_none());
    }

    #[test]
    fn should_deserialize_error_message() {
        let json = r#"{
            "type": "error",
            "error": {
                "message": "rate limited",
                "status_code": 429
            }
        }"#;

        let msg: GladiaMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.msg_type, "error");
        let err = msg.error.unwrap();
        assert_eq!(err.message, "rate limited");
        assert_eq!(err.status_code, 429);
    }

    #[test]
    fn should_deserialize_empty_json_object() {
        let json = r#"{}"#;

        let msg: GladiaMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.msg_type, "");
        assert!(msg.data.is_none());
        assert!(msg.error.is_none());
    }

    #[test]
    fn should_deserialize_session_response() {
        let json = r#"{
            "id": "session-123",
            "url": "wss://gladia.io/ws/session-123"
        }"#;

        let session: GladiaSession = serde_json::from_str(json).unwrap();

        assert_eq!(session.id, "session-123");
        assert_eq!(session.url, "wss://gladia.io/ws/session-123");
    }
}
