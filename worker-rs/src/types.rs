use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ja,
    Zh,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Ja, Lang::Zh];

    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ja => "ja",
            Lang::Zh => "zh",
        }
    }

    pub fn from_str(s: &str) -> Option<Lang> {
        match s {
            "en" => Some(Lang::En),
            "ja" => Some(Lang::Ja),
            "zh" => Some(Lang::Zh),
            _ => None,
        }
    }

    pub fn voice(&self) -> &'static str {
        match self {
            Lang::En => "af_bella",
            Lang::Ja => "jf_alpha",
            Lang::Zh => "zf_xiaobei",
        }
    }
}

// --- WebSocket messages (server → client) ---

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    #[serde(rename = "room:created")]
    RoomCreated { #[serde(rename = "roomId")] room_id: String },

    #[serde(rename = "room:joined")]
    RoomJoined { #[serde(rename = "roomId")] room_id: String, lang: Lang },

    #[serde(rename = "room:guest_count")]
    GuestCount { counts: GuestCounts },

    #[serde(rename = "room:closed")]
    RoomClosed,

    #[serde(rename = "interim")]
    Interim { transcript: String },

    #[serde(rename = "final")]
    Final { transcript: String, #[serde(rename = "utteranceId")] utterance_id: u64 },

    #[serde(rename = "translation")]
    Translation {
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(rename = "utteranceId")] utterance_id: u64,
        #[serde(rename = "translateMs")] translate_ms: u64,
    },

    #[serde(rename = "tts_start")]
    TtsStart { #[serde(rename = "utteranceId")] utterance_id: u64 },

    #[serde(rename = "tts_end")]
    TtsEnd { #[serde(rename = "utteranceId")] utterance_id: u64, #[serde(rename = "ttsMs")] tts_ms: u64 },

    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Serialize, Default)]
pub struct GuestCounts {
    pub en: usize,
    pub ja: usize,
    pub zh: usize,
}

// --- WebSocket messages (client → server) ---

#[derive(Deserialize)]
pub struct ClientMsg {
    #[serde(rename = "type")]
    pub msg_type: String,
}

// --- Deepgram Nova response ---

#[derive(Deserialize)]
pub struct NovaMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub channel: Option<NovaChannel>,
    pub is_final: Option<bool>,
    pub speech_final: Option<bool>,
}

#[derive(Deserialize)]
pub struct NovaChannel {
    pub alternatives: Vec<NovaAlternative>,
}

#[derive(Deserialize)]
pub struct NovaAlternative {
    pub transcript: String,
}

// --- Workers AI translation response ---

#[derive(Serialize)]
pub struct TranslateInput {
    pub text: String,
    pub source_lang: String,
    pub target_lang: String,
}

#[derive(Deserialize)]
pub struct TranslateOutput {
    pub translated_text: Option<String>,
}

// --- Kokoro TTS request ---

#[derive(Serialize)]
pub struct TtsRequest {
    pub model: String,
    pub voice: String,
    pub input: String,
}
