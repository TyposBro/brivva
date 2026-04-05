//! TTS configuration: API keys, style params, response types.

use serde::Deserialize;
use std::sync::LazyLock;

use crate::core::config::DEFAULT_VOICE_ID;

/// TTS provider API key (ElevenLabs)
pub static TTS_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("TTS_API_KEY").unwrap_or_default()
});

/// Default ElevenLabs voice (Rachel — multilingual).
pub static DEFAULT_VOICE: LazyLock<String> = LazyLock::new(|| {
    std::env::var("DEFAULT_VOICE")
        .unwrap_or_else(|_| DEFAULT_VOICE_ID.to_string())
});

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
