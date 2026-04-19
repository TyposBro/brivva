use crate::features::broadcast::domain::Lang;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const SONIOX_WS_URL_DEFAULT: &str = "wss://stt-rt.soniox.com/transcribe-websocket";

pub static SONIOX_WS_URL: LazyLock<String> = LazyLock::new(|| {
    std::env::var("SONIOX_WS_URL").unwrap_or_else(|_| SONIOX_WS_URL_DEFAULT.to_string())
});

pub const SONIOX_MODEL: &str = "stt-rt-preview";
pub const HOST_SAMPLE_RATE: u32 = 44_100;
pub const SONIOX_END_TOKEN: &str = "<end>";

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
    pub translation: Option<SonioxTranslation>,
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
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SonioxToken {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub is_final: bool,
    #[serde(default)]
    pub translation_status: Option<String>,
}

#[derive(Clone)]
pub enum SonioxMode {
    Source {
        lang: Lang,
    },
    Translate {
        source_lang: Lang,
        target_lang: Lang,
    },
}

impl SonioxMode {
    pub fn tag(&self) -> String {
        match self {
            SonioxMode::Source { lang } => format!("src:{}", lang),
            SonioxMode::Translate {
                source_lang,
                target_lang,
            } => format!("{}→{}", source_lang, target_lang),
        }
    }

    pub fn build_config<'a>(&self, api_key: &'a str) -> SonioxConfig<'a> {
        let (hint_lang, translation) = match self {
            SonioxMode::Source { lang } => (lang.to_string(), None),
            SonioxMode::Translate {
                source_lang,
                target_lang,
            } => (
                source_lang.to_string(),
                Some(SonioxTranslation {
                    kind: "one_way",
                    target_language: target_lang.to_string(),
                }),
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
            translation,
        }
    }

    pub fn accepts(&self, token: &SonioxToken) -> bool {
        if token.text == SONIOX_END_TOKEN {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn token(text: &str, translation_status: Option<&str>) -> SonioxToken {
        SonioxToken {
            text: text.to_string(),
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
        assert!(config.translation.is_none());
    }

    #[test]
    fn translate_config_hints_source_lang_and_targets_target() {
        let mode = SonioxMode::Translate {
            source_lang: Lang::En,
            target_lang: Lang::Zh,
        };
        let config = mode.build_config("k");

        assert_eq!(config.language_hints, vec!["en".to_string()]);
        let translation = config.translation.expect("translate mode has block");
        assert_eq!(translation.kind, "one_way");
        assert_eq!(translation.target_language, "zh");
    }

    #[test]
    fn config_serializes_skips_translation_when_none() {
        let mode = SonioxMode::Source { lang: Lang::En };
        let json = serde_json::to_string(&mode.build_config("k")).unwrap();
        assert!(!json.contains("translation"), "serialized: {json}");
    }

    #[test]
    fn translate_tag_uses_arrow() {
        let mode = SonioxMode::Translate {
            source_lang: Lang::En,
            target_lang: Lang::Ja,
        };
        assert_eq!(mode.tag(), "en→ja");
    }

    #[test]
    fn source_tag_uses_src_prefix() {
        let mode = SonioxMode::Source { lang: Lang::Ko };
        assert_eq!(mode.tag(), "src:ko");
    }
}
