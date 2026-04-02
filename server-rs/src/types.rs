use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use dashmap::DashMap;
use tokio::sync::mpsc;
use axum::extract::ws::Message;

use crate::ffmpeg::SharedRtmpManager;

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
    pub fn voice_id(&self) -> &'static str {
        match self {
            Lang::En => "EXAVITQu4vr4xnSDxMaL",  // Sarah
            Lang::Ja => "pFZP5JQG7iQjIQuC4Bku",  // Lily
            Lang::Zh => "Xb7hH8MSUJpSbSDYk0k2",  // Alice
            Lang::Ko => "cgSgspJ2msm6clMCkdW9",  // Jessica
        }
    }
}

// ── Session (replaces Room) ──────────────────────────────

pub struct Session {
    pub id: String,
    pub source_lang: Lang,
    pub target_langs: Vec<Lang>,
    pub host_tx: Option<mpsc::UnboundedSender<Message>>,
    /// Cloned voice ID from ElevenLabs
    pub voice_clone_id: Option<String>,
    /// Translation tier: 1 = subtitles only, 2 = voice + subtitles
    pub tier: u8,
    /// FFmpeg RTMP manager for streaming to platforms
    pub rtmp_manager: Option<SharedRtmpManager>,
    /// Languages being streamed via RTMP
    pub rtmp_langs: Vec<Lang>,
    /// Signal to stop RTMP health monitor
    pub rtmp_stop: Arc<AtomicBool>,
    /// Video codec from MediaRecorder ("h264" or "vp8")
    pub video_codec: Option<String>,
}

impl Session {
    pub fn new(id: String, source_lang: Lang, target_langs: Vec<Lang>, tier: u8) -> Self {
        Self {
            id,
            source_lang,
            target_langs,
            host_tx: None,
            voice_clone_id: None,
            tier,
            rtmp_manager: None,
            rtmp_langs: Vec::new(),
            rtmp_stop: Arc::new(AtomicBool::new(false)),
            video_codec: None,
        }
    }

    pub fn active_langs(&self) -> Vec<Lang> {
        let mut langs = self.target_langs.clone();
        for lang in &self.rtmp_langs {
            if !langs.contains(lang) {
                langs.push(lang.clone());
            }
        }
        langs
    }

    pub fn send_to_host(&self, msg: Message) {
        if let Some(tx) = &self.host_tx {
            let _ = tx.send(msg);
        }
    }
}

pub type Sessions = Arc<DashMap<String, Session>>;

// ── WebSocket Messages (server → client) ──────────────────

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

    #[serde(rename = "voice:ready")]
    VoiceReady {
        #[serde(rename = "voiceId")]
        voice_id: String,
    },

    #[serde(rename = "error")]
    Error { message: String },
}
