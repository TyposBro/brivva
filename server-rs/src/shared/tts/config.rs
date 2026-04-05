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
