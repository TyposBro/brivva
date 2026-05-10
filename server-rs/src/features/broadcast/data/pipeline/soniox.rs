use crate::features::broadcast::domain::Lang;
use serde::{Deserialize, Serialize};

pub const SONIOX_MODEL: &str = "stt-rt-preview";
pub const HOST_SAMPLE_RATE: u32 = 44_100;
pub const SONIOX_END_TOKEN: &str = "<end>";
pub const SONIOX_FIN_TOKEN: &str = "<fin>";

pub fn is_soniox_endpoint_token(text: &str) -> bool {
    matches!(text, SONIOX_END_TOKEN | SONIOX_FIN_TOKEN)
}

#[derive(Debug, Serialize)]
pub struct SonioxConfig<'a> {
    pub api_key: &'a str,
    pub model: &'a str,
    pub audio_format: &'a str,
    pub sample_rate: u32,
    pub num_channels: u32,
    pub language_hints: Vec<String>,
    pub enable_endpoint_detection: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_endpoint_delay_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<SonioxContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation: Option<SonioxTranslation>,
}

#[derive(Debug, Serialize)]
pub struct SonioxContext {
    pub general: Vec<SonioxContextItem>,
}

#[derive(Debug, Serialize)]
pub struct SonioxContextItem {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct SonioxTranslation {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub target_language: String,
}

#[derive(Debug, Deserialize)]
pub struct SonioxResponse {
    #[serde(default)]
    pub tokens: Vec<SonioxToken>,
    /// Soniox sends `error_code` as an integer (e.g. `408`) for transport
    /// errors but as a string (e.g. `"auth_failed"`) for application errors.
    /// Accept both shapes so the caller can detect either kind — earlier the
    /// strict `Option<String>` deserializer rejected every `408`/`429`/etc.
    /// response, which made `parse_soniox_response` return `None` and the
    /// processor loop treated those errors as "no usable response, keep going"
    /// rather than tearing down the pipeline, so translation utterances died
    /// in this swallowed-error path.
    #[serde(default, deserialize_with = "deserialize_error_code")]
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

fn deserialize_error_code<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct ErrorCodeVisitor;

    impl<'de> Visitor<'de> for ErrorCodeVisitor {
        type Value = Option<String>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a string, integer, or null Soniox error_code")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            deserializer.deserialize_any(ErrorCodeVisitor)
        }
    }

    deserializer.deserialize_option(ErrorCodeVisitor)
}

#[derive(Debug, Deserialize)]
pub struct SonioxToken {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub start_ms: Option<u64>,
    #[serde(default)]
    pub end_ms: Option<u64>,
    #[serde(default)]
    pub is_final: bool,
    #[serde(default)]
    pub translation_status: Option<String>,
}

impl SonioxToken {
    pub fn timing_ms(&self) -> Option<(u64, u64)> {
        match (self.start_ms, self.end_ms) {
            (Some(start), Some(end)) if end >= start => Some((start, end)),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub enum SonioxMode {
    Source {
        lang: Lang,
    },
    Translate {
        source_lang: Lang,
        target_lang: Lang,
        terms: Vec<String>,
    },
}

impl SonioxMode {
    pub fn tag(&self) -> String {
        match self {
            SonioxMode::Source { lang } => format!("src:{}", lang),
            SonioxMode::Translate {
                source_lang,
                target_lang,
                ..
            } => format!("{}→{}", source_lang, target_lang),
        }
    }

    pub fn target_lang_string(&self) -> Option<String> {
        match self {
            SonioxMode::Source { .. } => None,
            SonioxMode::Translate { target_lang, .. } => Some(target_lang.to_string()),
        }
    }

    pub fn build_config<'a>(&self, api_key: &'a str) -> SonioxConfig<'a> {
        let (hint_lang, translation, context) = match self {
            SonioxMode::Source { lang } => (lang.to_string(), None, None),
            SonioxMode::Translate {
                source_lang,
                target_lang,
                terms,
            } => (
                source_lang.to_string(),
                Some(SonioxTranslation {
                    kind: "one_way",
                    target_language: target_lang.to_string(),
                }),
                Some(live_commerce_translation_context(terms)),
            ),
        };

        SonioxConfig {
            api_key,
            model: SONIOX_MODEL,
            audio_format: "pcm_s16le",
            sample_rate: HOST_SAMPLE_RATE,
            num_channels: 1,
            language_hints: vec![hint_lang],
            enable_endpoint_detection: true,
            max_endpoint_delay_ms: Some(500),
            context,
            translation,
        }
    }

    pub fn accepts(&self, token: &SonioxToken) -> bool {
        if is_soniox_endpoint_token(&token.text) {
            return true;
        }

        match self {
            SonioxMode::Source { .. } => true,
            SonioxMode::Translate { .. } => {
                matches!(token.translation_status.as_deref(), Some("translation"))
            }
        }
    }
}

fn live_commerce_translation_context(terms: &[String]) -> SonioxContext {
    let mut general = vec![
        SonioxContextItem {
            key: "domain".into(),
            value: "live commerce".into(),
        },
        SonioxContextItem {
            key: "setting".into(),
            value: "real-time sales livestream".into(),
        },
        SonioxContextItem {
            key: "instructions".into(),
            value: "Translate in natural spoken live-commerce style. Preserve the host's meaning, tone, prices, product names, stock counts, discounts, dates, and calls to action exactly. Do not summarize or omit details.".into(),
        },
    ];
    if !terms.is_empty() {
        general.push(SonioxContextItem {
            key: "translation_terms".into(),
            value: terms.join(", "),
        });
    }
    SonioxContext { general }
}

#[cfg(test)]
mod tests {
    use super::*;

    // §0.5.1 — real-captured fixtures. Paths are compile-time resolved from
    // soniox.rs → server-rs/tests/fixtures/soniox/. Do not inline new shapes
    // here; add them as captured JSON files under the fixtures directory and
    // document provenance in its README.
    mod fixtures {
        pub const ERROR_AUTH_FAILED: &str =
            include_str!("../../../../../tests/fixtures/soniox/error_auth_failed.json");
        pub const ERROR_408_TIMEOUT: &str =
            include_str!("../../../../../tests/fixtures/soniox/error_408_timeout.json");
        pub const ERROR_ABSENT: &str =
            include_str!("../../../../../tests/fixtures/soniox/error_absent.json");
        pub const ERROR_NULL: &str =
            include_str!("../../../../../tests/fixtures/soniox/error_null.json");
    }

    fn token(text: &str, translation_status: Option<&str>) -> SonioxToken {
        SonioxToken {
            text: text.to_string(),
            start_ms: None,
            end_ms: None,
            is_final: false,
            translation_status: translation_status.map(str::to_string),
        }
    }

    #[test]
    fn source_mode_accepts_any_token_regardless_of_translation_status() {
        let mode = SonioxMode::Source { lang: Lang::En };
        assert!(mode.accepts(&token("hello", None)));
        assert!(mode.accepts(&token("world", Some("original"))));
        assert!(mode.accepts(&token("x", Some("translation"))));
    }

    #[test]
    fn translate_mode_accepts_only_tokens_tagged_translation() {
        let mode = SonioxMode::Translate {
            source_lang: Lang::En,
            target_lang: Lang::Ja,
            terms: Vec::new(),
        };
        assert!(mode.accepts(&token("konnichiwa", Some("translation"))));
        assert!(!mode.accepts(&token("hello", Some("original"))));
        assert!(!mode.accepts(&token("x", None)));
    }

    #[test]
    fn end_token_always_accepted_even_when_translation_tag_missing() {
        let translate = SonioxMode::Translate {
            source_lang: Lang::En,
            target_lang: Lang::Ko,
            terms: Vec::new(),
        };
        let source = SonioxMode::Source { lang: Lang::En };

        let end_token = token(SONIOX_END_TOKEN, None);
        assert!(translate.accepts(&end_token));
        assert!(source.accepts(&end_token));
    }

    #[test]
    fn source_config_has_no_translation_block() {
        let mode = SonioxMode::Source { lang: Lang::Ja };
        let config = mode.build_config("api-key");
        assert_eq!(config.api_key, "api-key");
        assert_eq!(config.model, SONIOX_MODEL);
        assert_eq!(config.audio_format, "pcm_s16le");
        assert_eq!(config.sample_rate, HOST_SAMPLE_RATE);
        assert_eq!(config.num_channels, 1);
        assert_eq!(config.language_hints, vec!["ja".to_string()]);
        assert!(config.enable_endpoint_detection);
        assert_eq!(config.max_endpoint_delay_ms, Some(500));
        assert!(config.context.is_none());
        assert!(config.translation.is_none());
    }

    #[test]
    fn translate_config_hints_source_lang_and_targets_target() {
        let mode = SonioxMode::Translate {
            source_lang: Lang::En,
            target_lang: Lang::Zh,
            terms: Vec::new(),
        };
        let config = mode.build_config("k");

        assert_eq!(config.language_hints, vec!["en".to_string()]);
        let translation = config.translation.expect("translate mode has block");
        assert_eq!(translation.kind, "one_way");
        assert_eq!(translation.target_language, "zh");
        let context = config.context.expect("translate mode has context");
        let instructions = context
            .general
            .iter()
            .find(|item| item.key == "instructions")
            .expect("has instructions");
        assert!(instructions.value.contains("natural spoken"));
        assert!(instructions.value.contains("Do not summarize"));
    }

    #[test]
    fn translate_config_includes_host_translation_terms_when_present() {
        let mode = SonioxMode::Translate {
            source_lang: Lang::Ko,
            target_lang: Lang::Ja,
            terms: vec!["Brivva Pro serum".into(), "SUMMER20".into()],
        };
        let config = mode.build_config("k");
        let context = config.context.expect("translate mode has context");
        let terms = context
            .general
            .iter()
            .find(|item| item.key == "translation_terms")
            .expect("has host terms");
        assert_eq!(terms.value, "Brivva Pro serum, SUMMER20");
    }

    #[test]
    fn config_serializes_skips_translation_when_none() {
        let mode = SonioxMode::Source { lang: Lang::En };
        let json = serde_json::to_string(&mode.build_config("k")).unwrap();
        assert!(!json.contains("translation"), "serialized: {json}");
        assert!(!json.contains("context"), "serialized: {json}");
    }

    #[test]
    fn translate_config_serializes_context_instructions() {
        let mode = SonioxMode::Translate {
            source_lang: Lang::Ko,
            target_lang: Lang::Ja,
            terms: Vec::new(),
        };
        let json = serde_json::to_string(&mode.build_config("k")).unwrap();
        assert!(json.contains("\"context\""), "serialized: {json}");
        assert!(json.contains("live commerce"), "serialized: {json}");
        assert!(json.contains("natural spoken"), "serialized: {json}");
        assert!(
            json.contains("\"max_endpoint_delay_ms\":500"),
            "serialized: {json}"
        );
    }

    #[test]
    fn translate_tag_uses_arrow() {
        let mode = SonioxMode::Translate {
            source_lang: Lang::En,
            target_lang: Lang::Ja,
            terms: Vec::new(),
        };
        assert_eq!(mode.tag(), "en→ja");
    }

    #[test]
    fn source_tag_uses_src_prefix() {
        let mode = SonioxMode::Source { lang: Lang::Ko };
        assert_eq!(mode.tag(), "src:ko");
    }

    #[test]
    fn deserializes_error_code_when_soniox_sends_it_as_a_string() {
        let resp: SonioxResponse =
            serde_json::from_str(fixtures::ERROR_AUTH_FAILED).expect("parses");
        assert_eq!(resp.error_code.as_deref(), Some("auth_failed"));
    }

    #[test]
    fn deserializes_error_code_when_soniox_sends_it_as_an_integer() {
        // Production regression: Soniox sends transport errors with numeric
        // codes (`408 "Request timeout."`, `429 "Rate limited."`) but used
        // to deserialize into `Option<String>` strictly, which made every
        // such response fail parse and disappear silently. The processor
        // then never tore down the pipeline on Soniox errors and translation
        // utterances stalled forever.
        let resp: SonioxResponse =
            serde_json::from_str(fixtures::ERROR_408_TIMEOUT).expect("parses");
        assert_eq!(resp.error_code.as_deref(), Some("408"));
        assert_eq!(resp.error_message.as_deref(), Some("Request timeout."));
    }

    #[test]
    fn deserializes_error_code_as_none_when_field_absent() {
        let resp: SonioxResponse = serde_json::from_str(fixtures::ERROR_ABSENT).expect("parses");
        assert!(resp.error_code.is_none());
    }

    #[test]
    fn deserializes_error_code_as_none_when_field_explicitly_null() {
        let resp: SonioxResponse = serde_json::from_str(fixtures::ERROR_NULL).expect("parses");
        assert!(resp.error_code.is_none());
    }
}
