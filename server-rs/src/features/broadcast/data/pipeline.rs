mod soniox;
mod stt;
mod stt_response;
mod stt_transport;
pub mod tts;

use crate::features::broadcast::domain::ServerMsg;
use axum::extract::ws::Message;

pub use stt::{PipelineSession, start_stt_pipelines};

fn to_ws(msg: &ServerMsg) -> Message {
    // ServerMsg is always serializable by construction.
    Message::Text(serde_json::to_string(msg).unwrap().into())
}
