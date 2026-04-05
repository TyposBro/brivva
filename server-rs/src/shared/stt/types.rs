//! Soniox v4 response types for STT messages.

use serde::Deserialize;

// ── Soniox Response Types ──────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SonioxResponse {
    #[serde(default)]
    pub tokens: Vec<SonioxToken>,
    pub final_audio_proc_ms: Option<u64>,
    pub total_audio_proc_ms: Option<u64>,
    pub finished: Option<bool>,
    pub error_code: Option<u16>,
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SonioxToken {
    pub text: String,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub confidence: Option<f32>,
    #[serde(default)]
    pub is_final: bool,
    pub language: Option<String>,
    pub translation_status: Option<String>,
    pub source_language: Option<String>,
}

impl SonioxResponse {
    pub fn is_error(&self) -> bool {
        self.error_code.is_some()
    }

    pub fn is_finished(&self) -> bool {
        self.finished.unwrap_or(false)
    }

    pub fn has_endpoint(&self) -> bool {
        self.tokens.iter().any(|t| t.text == "<end>" && t.is_final)
    }
}

impl SonioxToken {
    pub fn is_original(&self) -> bool {
        self.translation_status.as_deref() == Some("original")
    }

    pub fn is_translation(&self) -> bool {
        self.translation_status.as_deref() == Some("translation")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_error() ─────────────────────────────────────────

    #[test]
    fn should_report_error_when_error_code_present() {
        let resp = SonioxResponse {
            tokens: vec![],
            final_audio_proc_ms: None,
            total_audio_proc_ms: None,
            finished: None,
            error_code: Some(400),
            error_message: Some("bad request".into()),
        };

        assert!(resp.is_error());
    }

    #[test]
    fn should_not_report_error_when_error_code_absent() {
        let resp = SonioxResponse {
            tokens: vec![],
            final_audio_proc_ms: None,
            total_audio_proc_ms: None,
            finished: None,
            error_code: None,
            error_message: None,
        };

        assert!(!resp.is_error());
    }

    // ── is_finished() ──────────────────────────────────────

    #[test]
    fn should_report_finished_when_finished_is_true() {
        let resp = SonioxResponse {
            tokens: vec![],
            final_audio_proc_ms: None,
            total_audio_proc_ms: None,
            finished: Some(true),
            error_code: None,
            error_message: None,
        };

        assert!(resp.is_finished());
    }

    #[test]
    fn should_not_report_finished_when_finished_is_false() {
        let resp = SonioxResponse {
            tokens: vec![],
            final_audio_proc_ms: None,
            total_audio_proc_ms: None,
            finished: Some(false),
            error_code: None,
            error_message: None,
        };

        assert!(!resp.is_finished());
    }

    #[test]
    fn should_not_report_finished_when_finished_is_none() {
        let resp = SonioxResponse {
            tokens: vec![],
            final_audio_proc_ms: None,
            total_audio_proc_ms: None,
            finished: None,
            error_code: None,
            error_message: None,
        };

        assert!(!resp.is_finished());
    }

    // ── has_endpoint() ─────────────────────────────────────

    #[test]
    fn should_detect_endpoint_when_end_token_present_and_final() {
        let resp = SonioxResponse {
            tokens: vec![
                SonioxToken {
                    text: "hello".into(),
                    start_ms: Some(0),
                    end_ms: Some(500),
                    confidence: Some(0.95),
                    is_final: true,
                    language: Some("en".into()),
                    translation_status: None,
                    source_language: None,
                },
                SonioxToken {
                    text: "<end>".into(),
                    start_ms: None,
                    end_ms: None,
                    confidence: None,
                    is_final: true,
                    language: None,
                    translation_status: None,
                    source_language: None,
                },
            ],
            final_audio_proc_ms: None,
            total_audio_proc_ms: None,
            finished: None,
            error_code: None,
            error_message: None,
        };

        assert!(resp.has_endpoint());
    }

    #[test]
    fn should_not_detect_endpoint_when_end_token_is_not_final() {
        let resp = SonioxResponse {
            tokens: vec![SonioxToken {
                text: "<end>".into(),
                start_ms: None,
                end_ms: None,
                confidence: None,
                is_final: false,
                language: None,
                translation_status: None,
                source_language: None,
            }],
            final_audio_proc_ms: None,
            total_audio_proc_ms: None,
            finished: None,
            error_code: None,
            error_message: None,
        };

        assert!(!resp.has_endpoint());
    }

    #[test]
    fn should_not_detect_endpoint_when_no_end_token() {
        let resp = SonioxResponse {
            tokens: vec![SonioxToken {
                text: "hello".into(),
                start_ms: Some(0),
                end_ms: Some(500),
                confidence: Some(0.95),
                is_final: true,
                language: Some("en".into()),
                translation_status: None,
                source_language: None,
            }],
            final_audio_proc_ms: None,
            total_audio_proc_ms: None,
            finished: None,
            error_code: None,
            error_message: None,
        };

        assert!(!resp.has_endpoint());
    }

    // ── is_original() / is_translation() ───────────────────

    #[test]
    fn should_identify_original_token() {
        let token = SonioxToken {
            text: "hola".into(),
            start_ms: Some(0),
            end_ms: Some(300),
            confidence: Some(0.9),
            is_final: true,
            language: Some("es".into()),
            translation_status: Some("original".into()),
            source_language: Some("es".into()),
        };

        assert!(token.is_original());
        assert!(!token.is_translation());
    }

    #[test]
    fn should_identify_translation_token() {
        let token = SonioxToken {
            text: "hello".into(),
            start_ms: Some(0),
            end_ms: Some(300),
            confidence: Some(0.85),
            is_final: true,
            language: Some("en".into()),
            translation_status: Some("translation".into()),
            source_language: Some("es".into()),
        };

        assert!(token.is_translation());
        assert!(!token.is_original());
    }

    #[test]
    fn should_not_identify_token_without_translation_status() {
        let token = SonioxToken {
            text: "hello".into(),
            start_ms: Some(0),
            end_ms: Some(300),
            confidence: Some(0.9),
            is_final: true,
            language: Some("en".into()),
            translation_status: None,
            source_language: None,
        };

        assert!(!token.is_original());
        assert!(!token.is_translation());
    }

    // ── deserialization ────────────────────────────────────

    #[test]
    fn should_deserialize_response_with_tokens() {
        let json = r#"{
            "tokens": [
                {
                    "text": "hello",
                    "start_ms": 100,
                    "end_ms": 500,
                    "confidence": 0.95,
                    "is_final": true,
                    "language": "en"
                }
            ],
            "final_audio_proc_ms": 500,
            "total_audio_proc_ms": 1000
        }"#;

        let resp: SonioxResponse = serde_json::from_str(json).unwrap();

        assert_eq!(resp.tokens.len(), 1);
        assert_eq!(resp.tokens[0].text, "hello");
        assert!(resp.tokens[0].is_final);
        assert_eq!(resp.final_audio_proc_ms, Some(500));
    }

    #[test]
    fn should_deserialize_error_response() {
        let json = r#"{
            "error_code": 401,
            "error_message": "invalid api key"
        }"#;

        let resp: SonioxResponse = serde_json::from_str(json).unwrap();

        assert!(resp.is_error());
        assert_eq!(resp.error_code, Some(401));
        assert_eq!(resp.error_message.as_deref(), Some("invalid api key"));
    }

    #[test]
    fn should_deserialize_finished_response() {
        let json = r#"{"finished": true}"#;

        let resp: SonioxResponse = serde_json::from_str(json).unwrap();

        assert!(resp.is_finished());
        assert!(resp.tokens.is_empty());
    }

    #[test]
    fn should_deserialize_empty_json_object() {
        let json = r#"{}"#;

        let resp: SonioxResponse = serde_json::from_str(json).unwrap();

        assert!(resp.tokens.is_empty());
        assert!(!resp.is_error());
        assert!(!resp.is_finished());
        assert!(!resp.has_endpoint());
    }

    #[test]
    fn should_deserialize_translation_token() {
        let json = r#"{
            "tokens": [{
                "text": "hello",
                "is_final": true,
                "translation_status": "translation",
                "source_language": "es"
            }]
        }"#;

        let resp: SonioxResponse = serde_json::from_str(json).unwrap();

        assert!(resp.tokens[0].is_translation());
        assert_eq!(resp.tokens[0].source_language.as_deref(), Some("es"));
    }
}
