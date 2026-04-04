use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
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

}

// ── Session (replaces Room) ──────────────────────────────

pub struct Session {
    pub id: String,
    pub source_lang: Lang,
    pub target_langs: Vec<Lang>,
    pub host_tx: Option<mpsc::UnboundedSender<Message>>,
    /// Cloned voice ID (ElevenLabs IVC)
    pub voice_clone_id: Option<String>,
    /// ElevenLabs TTS model: "eleven_turbo_v2_5" or "eleven_flash_v2_5"
    pub tts_model: String,
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
    /// Broadcast delay in milliseconds (sent from frontend, used by RTMP + TTS timeout)
    pub broadcast_delay_ms: u64,
}

impl Session {
    pub fn new(id: String, source_lang: Lang, target_langs: Vec<Lang>, tier: u8) -> Self {
        Self {
            id,
            source_lang,
            target_langs,
            host_tx: None,
            voice_clone_id: None,
            tts_model: "eleven_turbo_v2_5".to_string(),
            tier,
            rtmp_manager: None,
            rtmp_langs: Vec::new(),
            rtmp_stop: Arc::new(AtomicBool::new(false)),
            video_codec: None,
            broadcast_delay_ms: 3000,
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

// ── Progressive Chunk Types ─────────────────────────────────

/// A sub-utterance chunk ready for translation + TTS.
/// Emitted by ProgressiveChunkDetector when a clause boundary is found
/// during interim transcripts, or when Gladia emits a final.
pub struct ChunkEvent {
    /// The chunk's text (substring of the full utterance)
    pub text: String,
    /// 0-based index within the current utterance
    pub chunk_index: u16,
    /// Previous chunk's source text (for context-aware translation)
    pub context: Option<String>,
    /// True only when Gladia emits its FINAL event (last chunk)
    pub is_utterance_final: bool,
    /// Parent utterance ID
    pub utterance_id: u64,
    /// When the host started speaking this utterance (for A/V sync play_at)
    pub utterance_start: Instant,
    /// Host PCM audio for this chunk (for passthrough on source lang)
    pub host_audio: Vec<u8>,
}

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

    /// Progressive chunk translation (arrives before full-sentence translation)
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
