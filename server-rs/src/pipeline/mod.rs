//! Translation pipeline: STT → Translate → TTS → RTMP

pub(crate) mod budget;
pub(crate) mod chunk;
pub(crate) mod full;

use axum::extract::ws::Message;

use crate::features::broadcast::domain::ServerMsg;

pub(crate) fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}
