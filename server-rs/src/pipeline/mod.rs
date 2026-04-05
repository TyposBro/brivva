//! Translation pipeline: STT → Translate → TTS → RTMP

pub(crate) mod budget;
pub(crate) mod chunk;
mod stt;
pub(crate) mod tts;

use axum::extract::ws::Message;
use std::sync::LazyLock;

use crate::types::ServerMsg;

pub(super) static STT_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("STT_API_KEY").unwrap_or_default()
});

pub(crate) fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}

pub use stt::start_stt;
