use axum::extract::ws::Message;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;
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
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

// ── Guest ─────────────────────────────────────────────────

pub struct Guest {
    pub lang: Lang,
    pub tx: mpsc::UnboundedSender<Message>,
}

// ── Timestamped video frame ───────────────────────────────

#[derive(Clone)]
pub struct TimestampedFrame {
    pub data: String,       // base64 JPEG
    pub timestamp: Instant, // when received from host
}

/// Ring buffer of recent host video frames (thread-safe)
pub type FrameBuffer = Arc<Mutex<VecDeque<TimestampedFrame>>>;

/// Max seconds of video to buffer
pub const FRAME_BUFFER_SECS: u64 = 10;
/// Assumed host FPS for buffer capacity
const HOST_FPS: usize = 30;
/// Max frames to keep
pub const FRAME_BUFFER_CAP: usize = HOST_FPS * FRAME_BUFFER_SECS as usize; // 300

// ── Room ──────────────────────────────────────────────────

pub struct Room {
    pub id: String,
    pub source_lang: Lang,
    pub host_tx: Option<mpsc::UnboundedSender<Message>>,
    pub guests: DashMap<String, Guest>,
    /// Cloned voice ID from ElevenLabs (None until clone completes)
    pub voice_clone_id: Option<String>,
    /// Ring buffer of timestamped host video frames
    pub frame_buffer: FrameBuffer,
    /// When the room was created (epoch for Instant math)
    pub created_at: Instant,
    /// Dashboard session ID (links to DB session + streams)
    pub session_id: Option<String>,
    /// FFmpeg RTMP manager for streaming to platforms
    pub rtmp_manager: Option<crate::ffmpeg::SharedRtmpManager>,
}

impl Room {
    pub fn new(id: String, source_lang: Lang, session_id: Option<String>) -> Self {
        Self {
            id,
            source_lang,
            host_tx: None,
            guests: DashMap::new(),
            voice_clone_id: None,
            frame_buffer: Arc::new(Mutex::new(VecDeque::with_capacity(FRAME_BUFFER_CAP))),
            created_at: Instant::now(),
            session_id,
            rtmp_manager: None,
        }
    }

    /// Push a new video frame into the ring buffer, evicting old frames
    pub fn push_frame(&self, data: String) {
        let frame = TimestampedFrame {
            data,
            timestamp: Instant::now(),
        };
        if let Ok(mut buf) = self.frame_buffer.lock() {
            if buf.len() >= FRAME_BUFFER_CAP {
                buf.pop_front();
            }
            buf.push_back(frame);
        }
    }

    /// Extract frames between two timestamps (inclusive)
    pub fn get_frames_between(&self, start: Instant, end: Instant) -> Vec<TimestampedFrame> {
        if let Ok(buf) = self.frame_buffer.lock() {
            buf.iter()
                .filter(|f| f.timestamp >= start && f.timestamp <= end)
                .cloned()
                .collect()
        } else {
            Vec::new()
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

    #[serde(rename = "voice:ready")]
    VoiceReady {
        #[serde(rename = "voiceId")]
        voice_id: String,
    },

    #[serde(rename = "face:frame")]
    FaceFrame { data: String },

    #[serde(rename = "video_start")]
    VideoStart {
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
        #[serde(rename = "frameCount")]
        frame_count: u32,
    },

    #[serde(rename = "video_frame")]
    VideoFrame { data: String },

    #[serde(rename = "video_end")]
    VideoEnd {
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
    },

    #[serde(rename = "error")]
    Error { message: String },
}
