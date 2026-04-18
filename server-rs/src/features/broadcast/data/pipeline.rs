mod soniox;
mod stt;
mod tts;
mod voice_clone;

use axum::extract::ws::Message;
use crate::features::broadcast::domain::ServerMsg;

pub use stt::start_stt_pipelines;
pub use voice_clone::{clone_voice, delete_cloned_voice};

fn to_ws(msg: &ServerMsg) -> Message {
    // ServerMsg is always serializable by construction.
    Message::Text(serde_json::to_string(msg).unwrap().into())
}
