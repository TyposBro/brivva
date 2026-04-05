//! Application configuration — reads ALL environment variables.
//! This is the ONLY place env vars are read (besides dotenvy loading in lib.rs).

use crate::core::config::{DEFAULT_VOICE_ID, SERVER_ADDR, MAX_BODY_SIZE};

/// Typed configuration populated from environment variables at startup.
pub struct AppConfig {
    pub stt_api_key: String,
    pub tts_api_key: String,
    pub translate_api_key: String,
    pub default_voice: String,
    pub server_addr: String,
    pub max_body_size: usize,
}

impl AppConfig {
    /// Read all environment variables once at application bootstrap.
    pub fn from_env() -> Self {
        Self {
            stt_api_key: std::env::var("STT_API_KEY").unwrap_or_default(),
            tts_api_key: std::env::var("TTS_API_KEY").unwrap_or_default(),
            translate_api_key: std::env::var("TRANSLATE_API_KEY").unwrap_or_default(),
            default_voice: std::env::var("DEFAULT_VOICE")
                .unwrap_or_else(|_| DEFAULT_VOICE_ID.to_string()),
            server_addr: SERVER_ADDR.to_string(),
            max_body_size: MAX_BODY_SIZE,
        }
    }
}
