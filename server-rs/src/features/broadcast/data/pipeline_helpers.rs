//! Translation pipeline helper — shared serialization for ServerMsg.

use axum::extract::ws::Message;

use crate::features::broadcast::domain::ServerMsg;

pub(crate) fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_serialize_interim_msg_to_ws_text() {
        let msg = ServerMsg::Interim {
            transcript: "hello".to_string(),
        };

        let ws_msg = to_ws(&msg);

        match ws_msg {
            Message::Text(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["type"], "interim");
                assert_eq!(parsed["transcript"], "hello");
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn should_serialize_final_msg_with_utterance_id() {
        let msg = ServerMsg::Final {
            transcript: "world".to_string(),
            utterance_id: 42,
        };

        let ws_msg = to_ws(&msg);

        match ws_msg {
            Message::Text(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["type"], "final");
                assert_eq!(parsed["transcript"], "world");
                assert_eq!(parsed["utteranceId"], 42);
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn should_serialize_translation_msg_with_all_fields() {
        let msg = ServerMsg::Translation {
            lang: "es".to_string(),
            text: "hola".to_string(),
            utterance_id: 7,
            translate_ms: 120,
        };

        let ws_msg = to_ws(&msg);

        match ws_msg {
            Message::Text(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["type"], "translation");
                assert_eq!(parsed["lang"], "es");
                assert_eq!(parsed["text"], "hola");
                assert_eq!(parsed["utteranceId"], 7);
                assert_eq!(parsed["translateMs"], 120);
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn should_serialize_error_msg() {
        let msg = ServerMsg::Error {
            message: "something broke".to_string(),
        };

        let ws_msg = to_ws(&msg);

        match ws_msg {
            Message::Text(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["type"], "error");
                assert_eq!(parsed["message"], "something broke");
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn should_serialize_session_created_msg() {
        let msg = ServerMsg::SessionCreated {
            id: "abc-123".to_string(),
        };

        let ws_msg = to_ws(&msg);

        match ws_msg {
            Message::Text(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["type"], "session:created");
                assert_eq!(parsed["id"], "abc-123");
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn should_serialize_chunk_translation_msg() {
        let msg = ServerMsg::ChunkTranslation {
            lang: "fr".to_string(),
            text: "bonjour".to_string(),
            utterance_id: 3,
            chunk_index: 1,
            translate_ms: 55,
        };

        let ws_msg = to_ws(&msg);

        match ws_msg {
            Message::Text(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["type"], "chunk_translation");
                assert_eq!(parsed["lang"], "fr");
                assert_eq!(parsed["text"], "bonjour");
                assert_eq!(parsed["utteranceId"], 3);
                assert_eq!(parsed["chunkIndex"], 1);
                assert_eq!(parsed["translateMs"], 55);
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn should_serialize_tts_start_msg() {
        let msg = ServerMsg::TtsStart {
            lang: "de".to_string(),
            utterance_id: 10,
        };

        let ws_msg = to_ws(&msg);

        match ws_msg {
            Message::Text(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["type"], "tts_start");
                assert_eq!(parsed["lang"], "de");
                assert_eq!(parsed["utteranceId"], 10);
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn should_serialize_tts_end_msg() {
        let msg = ServerMsg::TtsEnd {
            lang: "de".to_string(),
            utterance_id: 10,
            tts_ms: 300,
        };

        let ws_msg = to_ws(&msg);

        match ws_msg {
            Message::Text(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["type"], "tts_end");
                assert_eq!(parsed["lang"], "de");
                assert_eq!(parsed["utteranceId"], 10);
                assert_eq!(parsed["ttsMs"], 300);
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }
}
