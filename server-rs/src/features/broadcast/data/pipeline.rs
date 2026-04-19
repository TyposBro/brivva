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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_ws_serializes_interim_to_text_message() {
        let msg = ServerMsg::Interim {
            transcript: "hi".into(),
        };
        match to_ws(&msg) {
            Message::Text(t) => assert!(t.as_str().contains("\"type\":\"interim\"")),
            other => panic!("expected text message, got {other:?}"),
        }
    }

    #[test]
    fn to_ws_serializes_translation_with_camel_case_target_lang() {
        let msg = ServerMsg::Translation {
            text: "hola".into(),
            utterance_id: 1,
            target_lang: "es".into(),
            translate_ms: 10,
        };
        match to_ws(&msg) {
            Message::Text(t) => {
                let s = t.as_str();
                assert!(s.contains("\"targetLang\":\"es\""));
                assert!(s.contains("\"utteranceId\":1"));
            }
            other => panic!("expected text message, got {other:?}"),
        }
    }
}
