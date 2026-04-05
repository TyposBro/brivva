use std::collections::HashMap;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    #[serde(rename = "session:created")]
    SessionCreated { id: String },

    #[serde(rename = "interim")]
    Interim { transcript: String },

    #[serde(rename = "final")]
    Final {
        transcript: String,
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
    },

    #[serde(rename = "translation")]
    Translation {
        lang: String,
        text: String,
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
        #[serde(rename = "translateMs")]
        translate_ms: u64,
    },

    #[serde(rename = "chunk_translation")]
    ChunkTranslation {
        lang: String,
        text: String,
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
        #[serde(rename = "chunkIndex")]
        chunk_index: u16,
        #[serde(rename = "translateMs")]
        translate_ms: u64,
    },

    #[serde(rename = "tts_start")]
    TtsStart {
        lang: String,
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
    },

    #[serde(rename = "tts_end")]
    TtsEnd {
        lang: String,
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
        #[serde(rename = "ttsMs")]
        tts_ms: u64,
    },

    #[serde(rename = "pipeline_health")]
    PipelineHealth {
        #[serde(rename = "sttConnected")]
        stt_connected: bool,
        #[serde(rename = "queueDepth")]
        queue_depth: HashMap<String, usize>,
        #[serde(rename = "droppedChunks")]
        dropped_chunks: u64,
        #[serde(rename = "ttsTimeouts")]
        tts_timeouts: u64,
        #[serde(rename = "translateErrors")]
        translate_errors: u64,
    },

    #[serde(rename = "pipeline_warning")]
    PipelineWarning {
        kind: String,
        lang: String,
        detail: String,
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
    },

    #[serde(rename = "error")]
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_serialize_session_created_with_tag() {
        let msg = ServerMsg::SessionCreated { id: "abc-123".to_string() };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["type"], "session:created");
        assert_eq!(json["id"], "abc-123");
    }

    #[test]
    fn should_serialize_interim_with_transcript() {
        let msg = ServerMsg::Interim { transcript: "hello".to_string() };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["type"], "interim");
        assert_eq!(json["transcript"], "hello");
    }

    #[test]
    fn should_serialize_final_with_renamed_utterance_id() {
        let msg = ServerMsg::Final {
            transcript: "hello world".to_string(),
            utterance_id: 42,
        };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["type"], "final");
        assert_eq!(json["transcript"], "hello world");
        assert_eq!(json["utteranceId"], 42);
        assert!(json.get("utterance_id").is_none());
    }

    #[test]
    fn should_serialize_translation_with_all_renamed_fields() {
        let msg = ServerMsg::Translation {
            lang: "ja".to_string(),
            text: "konnichiwa".to_string(),
            utterance_id: 7,
            translate_ms: 150,
        };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["type"], "translation");
        assert_eq!(json["lang"], "ja");
        assert_eq!(json["text"], "konnichiwa");
        assert_eq!(json["utteranceId"], 7);
        assert_eq!(json["translateMs"], 150);
    }

    #[test]
    fn should_serialize_chunk_translation_with_chunk_index() {
        let msg = ServerMsg::ChunkTranslation {
            lang: "zh".to_string(),
            text: "nihao".to_string(),
            utterance_id: 3,
            chunk_index: 1,
            translate_ms: 200,
        };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["type"], "chunk_translation");
        assert_eq!(json["lang"], "zh");
        assert_eq!(json["text"], "nihao");
        assert_eq!(json["utteranceId"], 3);
        assert_eq!(json["chunkIndex"], 1);
        assert_eq!(json["translateMs"], 200);
    }

    #[test]
    fn should_serialize_tts_start_with_lang_and_utterance_id() {
        let msg = ServerMsg::TtsStart {
            lang: "ko".to_string(),
            utterance_id: 10,
        };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["type"], "tts_start");
        assert_eq!(json["lang"], "ko");
        assert_eq!(json["utteranceId"], 10);
    }

    #[test]
    fn should_serialize_tts_end_with_tts_ms_renamed() {
        let msg = ServerMsg::TtsEnd {
            lang: "en".to_string(),
            utterance_id: 5,
            tts_ms: 320,
        };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["type"], "tts_end");
        assert_eq!(json["lang"], "en");
        assert_eq!(json["utteranceId"], 5);
        assert_eq!(json["ttsMs"], 320);
    }

    #[test]
    fn should_serialize_error_with_message() {
        let msg = ServerMsg::Error { message: "something broke".to_string() };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["type"], "error");
        assert_eq!(json["message"], "something broke");
    }

    #[test]
    fn should_not_include_snake_case_utterance_id_in_final() {
        let msg = ServerMsg::Final {
            transcript: "test".to_string(),
            utterance_id: 1,
        };

        let json_str = serde_json::to_string(&msg).unwrap();

        assert!(!json_str.contains("utterance_id"));
        assert!(json_str.contains("utteranceId"));
    }

    #[test]
    fn should_not_include_snake_case_fields_in_translation() {
        let msg = ServerMsg::Translation {
            lang: "ja".to_string(),
            text: "test".to_string(),
            utterance_id: 1,
            translate_ms: 100,
        };

        let json_str = serde_json::to_string(&msg).unwrap();

        assert!(!json_str.contains("utterance_id"));
        assert!(!json_str.contains("translate_ms"));
        assert!(json_str.contains("utteranceId"));
        assert!(json_str.contains("translateMs"));
    }

    #[test]
    fn should_not_include_snake_case_fields_in_chunk_translation() {
        let msg = ServerMsg::ChunkTranslation {
            lang: "zh".to_string(),
            text: "test".to_string(),
            utterance_id: 2,
            chunk_index: 0,
            translate_ms: 50,
        };

        let json_str = serde_json::to_string(&msg).unwrap();

        assert!(!json_str.contains("utterance_id"));
        assert!(!json_str.contains("chunk_index"));
        assert!(!json_str.contains("translate_ms"));
    }

    #[test]
    fn should_not_include_snake_case_tts_ms_in_tts_end() {
        let msg = ServerMsg::TtsEnd {
            lang: "en".to_string(),
            utterance_id: 3,
            tts_ms: 250,
        };

        let json_str = serde_json::to_string(&msg).unwrap();

        assert!(!json_str.contains("tts_ms"));
        assert!(json_str.contains("ttsMs"));
    }

    #[test]
    fn should_serialize_session_created_with_only_type_and_id_keys() {
        let msg = ServerMsg::SessionCreated { id: "s1".to_string() };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();
        let obj = json.as_object().unwrap();

        assert_eq!(obj.len(), 2);
        assert!(obj.contains_key("type"));
        assert!(obj.contains_key("id"));
    }

    #[test]
    fn should_serialize_error_with_only_type_and_message_keys() {
        let msg = ServerMsg::Error { message: "fail".to_string() };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();
        let obj = json.as_object().unwrap();

        assert_eq!(obj.len(), 2);
        assert!(obj.contains_key("type"));
        assert!(obj.contains_key("message"));
    }

    #[test]
    fn should_serialize_pipeline_health_with_renamed_fields() {
        let mut queue = HashMap::new();
        queue.insert("ko".to_string(), 3_usize);
        let msg = ServerMsg::PipelineHealth {
            stt_connected: true,
            queue_depth: queue,
            dropped_chunks: 1,
            tts_timeouts: 2,
            translate_errors: 0,
        };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["type"], "pipeline_health");
        assert_eq!(json["sttConnected"], true);
        assert_eq!(json["queueDepth"]["ko"], 3);
        assert_eq!(json["droppedChunks"], 1);
        assert_eq!(json["ttsTimeouts"], 2);
        assert_eq!(json["translateErrors"], 0);
    }

    #[test]
    fn should_serialize_pipeline_warning_with_all_fields() {
        let msg = ServerMsg::PipelineWarning {
            kind: "tts_timeout".to_string(),
            lang: "ko".to_string(),
            detail: "exceeded deadline".to_string(),
            utterance_id: 5,
        };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["type"], "pipeline_warning");
        assert_eq!(json["kind"], "tts_timeout");
        assert_eq!(json["lang"], "ko");
        assert_eq!(json["detail"], "exceeded deadline");
        assert_eq!(json["utteranceId"], 5);
    }

    #[test]
    fn should_serialize_translation_with_exactly_five_keys() {
        let msg = ServerMsg::Translation {
            lang: "ko".to_string(),
            text: "hello".to_string(),
            utterance_id: 1,
            translate_ms: 50,
        };

        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();
        let obj = json.as_object().unwrap();

        assert_eq!(obj.len(), 5);
    }
}
