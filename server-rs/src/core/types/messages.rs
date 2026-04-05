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

    #[serde(rename = "error")]
    Error { message: String },
}
