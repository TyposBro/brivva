//! Translation pipeline helper — shared serialization for ServerMsg.

use axum::extract::ws::Message;

use crate::features::broadcast::domain::ServerMsg;

pub(crate) fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}
