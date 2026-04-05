//! Translation pipeline: STT → Translate → TTS → RTMP
//!
//! Host audio flows through:
//! 1. Gladia Solaria-1 STT (direct WebSocket)
//! 2. Google Cloud Translation API v2 — per target language, parallel
//! 3. ElevenLabs TTS — streaming PCM output (with optional cloned voice)
//! 4. PCM truncate+fadeout → queue to RTMP manager for synced playback
//! 5. Source-language passthrough: host audio queued directly to RTMP (no TTS)

mod chunk;
mod stt;
mod tts;

use axum::extract::ws::Message;
use std::sync::LazyLock;

use crate::types::ServerMsg;

// ── Service Keys ──────────────────────────────────────────

/// STT provider API key (currently: Gladia)
pub(super) static STT_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("STT_API_KEY").unwrap_or_default()
});

pub(crate) fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}

pub use stt::start_stt;
