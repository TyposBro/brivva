use axum::extract::ws::Message;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

// ── Language ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ja,
    Zh,
    Ko,
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Lang::En => write!(f, "en"),
            Lang::Ja => write!(f, "ja"),
            Lang::Zh => write!(f, "zh"),
            Lang::Ko => write!(f, "ko"),
        }
    }
}

impl Lang {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "en" => Some(Lang::En),
            "ja" => Some(Lang::Ja),
            "zh" => Some(Lang::Zh),
            "ko" => Some(Lang::Ko),
            _ => None,
        }
    }

    /// ElevenLabs default voice ID for this language
    /// All premade voices support 32 languages via eleven_flash_v2_5
    pub fn voice_id(&self) -> &'static str {
        match self {
            Lang::En => "EXAVITQu4vr4xnSDxMaL",  // Sarah
            Lang::Ja => "pFZP5JQG7iQjIQuC4Bku",  // Lily
            Lang::Zh => "Xb7hH8MSUJpSbSDYk0k2",  // Alice
            Lang::Ko => "cgSgspJ2msm6clMCkdW9",  // Jessica
        }
    }
}

// ── Query params from frontend ────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RoomQuery {
    pub role: String,
    #[serde(rename = "sourceLang")]
    pub source_lang: Option<String>,
    #[serde(rename = "roomId")]
    pub room_id: Option<String>,
    pub lang: Option<String>,
}

// ── Guest ─────────────────────────────────────────────────

pub struct Guest {
    pub lang: Lang,
    pub tx: mpsc::UnboundedSender<Message>,
}

// ── Room ──────────────────────────────────────────────────

pub struct Room {
    pub id: String,
    pub source_lang: Lang,
    pub host_tx: Option<mpsc::UnboundedSender<Message>>,
    pub guests: DashMap<String, Guest>,
    pub latest_face: Option<String>,
}

impl Room {
    pub fn new(id: String, source_lang: Lang) -> Self {
        Self {
            id,
            source_lang,
            host_tx: None,
            guests: DashMap::new(),
            latest_face: None,
        }
    }

    /// Which languages have at least one guest?
    pub fn active_langs(&self) -> Vec<Lang> {
        let mut langs = std::collections::HashSet::new();
        for entry in self.guests.iter() {
            langs.insert(entry.value().lang.clone());
        }
        langs.into_iter().collect()
    }

    /// Send a message to all guests listening in a specific language
    pub fn send_to_lang(&self, lang: &Lang, msg: Message) {
        for entry in self.guests.iter() {
            if &entry.value().lang == lang {
                let _ = entry.value().tx.send(msg.clone());
            }
        }
    }

    /// Send a message to all guests (all languages)
    pub fn send_to_all_guests(&self, msg: Message) {
        for entry in self.guests.iter() {
            let _ = entry.value().tx.send(msg.clone());
        }
    }

    /// Send a message to the host
    pub fn send_to_host(&self, msg: Message) {
        if let Some(tx) = &self.host_tx {
            let _ = tx.send(msg);
        }
    }

    /// Guest count per language (for host dashboard)
    pub fn guest_counts(&self) -> std::collections::HashMap<String, usize> {
        let mut counts = std::collections::HashMap::new();
        for entry in self.guests.iter() {
            *counts.entry(entry.value().lang.to_string()).or_insert(0) += 1;
        }
        counts
    }
}

// ── Shared State ──────────────────────────────────────────

pub type Rooms = Arc<DashMap<String, Room>>;

// ── WebSocket Messages (server → client) ──────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    #[serde(rename = "room:created")]
    RoomCreated {
        #[serde(rename = "roomId")]
        room_id: String,
    },

    #[serde(rename = "room:joined")]
    RoomJoined {
        #[serde(rename = "roomId")]
        room_id: String,
    },

    #[serde(rename = "room:closed")]
    RoomClosed,

    #[serde(rename = "room:guest_count")]
    GuestCount {
        counts: std::collections::HashMap<String, usize>,
    },

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
        text: String,
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
        #[serde(rename = "translateMs")]
        translate_ms: u64,
    },

    #[serde(rename = "tts_start")]
    TtsStart {
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
    },

    #[serde(rename = "tts_end")]
    TtsEnd {
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
        #[serde(rename = "ttsMs")]
        tts_ms: u64,
    },

    #[serde(rename = "video_start")]
    VideoStart {
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
    },

    #[serde(rename = "video_frame")]
    VideoFrame {
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
        data: String,
    },

    #[serde(rename = "video_end")]
    VideoEnd {
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
        #[serde(rename = "lipsyncMs")]
        lipsync_ms: u64,
    },

    #[serde(rename = "face:frame")]
    FaceFrame { data: String },

    #[serde(rename = "error")]
    Error { message: String },
}
