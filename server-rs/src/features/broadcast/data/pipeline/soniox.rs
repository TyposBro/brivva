use crate::features::broadcast::domain::Lang;
use serde::{Deserialize, Serialize};

pub const SONIOX_WS_URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
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
    Source { lang: Lang },
    Translate { source_lang: Lang, target_lang: Lang },
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
