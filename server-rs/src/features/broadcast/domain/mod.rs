use axum::extract::ws::Message;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

// Fargate is host-only. There are no guest WebSockets — all translated
// audio leaves the server via RTMP to streaming platforms. The WS exists
// solely to (a) accept host audio/video uplink, (b) feed per-utterance
// progress + errors back to the host UI.

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
    // PRAGMATIC: callers want `Option<Self>` today; implementing `FromStr`
    // would force a `Result` shape through several hot call sites for no gain.
    #[allow(clippy::should_implement_trait)]
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
            Lang::En => "EXAVITQu4vr4xnSDxMaL", // Sarah
            Lang::Ja => "pFZP5JQG7iQjIQuC4Bku", // Lily
            Lang::Zh => "Xb7hH8MSUJpSbSDYk0k2", // Alice
            Lang::Ko => "cgSgspJ2msm6clMCkdW9", // Jessica
        }
    }
}

// ── Query params from frontend ────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SessionQuery {
    #[serde(rename = "sourceLang")]
    pub source_lang: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    /// JWT issued by Workers. Verified in `session_ws_handler`.
    pub token: Option<String>,
}

// ── Pipeline Configuration ────────────────────────────────
//
// Holds the upstream-service settings the STT + TTS pipeline needs at
// runtime. Built once at session start from `AppConfig` so lower layers
// never read env vars directly (CLAUDE.md §7).

#[derive(Default)]
pub struct PipelineConfig {
    pub soniox_api_key: String,
    pub soniox_ws_url: String,
    pub elevenlabs_api_key: String,
    pub elevenlabs_base_url: String,
}

// ── Live Session Runtime ──────────────────────────────────

pub struct LiveSession {
    pub id: String,
    pub source_lang: Lang,
    pub host_tx: Option<mpsc::UnboundedSender<Message>>,
    /// Voice ID currently selected for TTS. Workers owns voice lifecycle and
    /// provides this durable ElevenLabs voice id through the session bundle.
    pub selected_voice_id: Option<String>,
    /// Session ID from Workers (links to D1 session + streams)
    pub session_id: Option<String>,
    /// FFmpeg RTMP manager for streaming to platforms
    pub rtmp_manager: Option<crate::features::broadcast::data::ffmpeg::SharedRtmpManager>,
    /// Target languages being streamed via RTMP (one entry per configured stream).
    pub rtmp_langs: Vec<Lang>,
    /// Upstream-service config injected from orchestration at session start.
    pub pipeline_config: Arc<PipelineConfig>,
}

impl LiveSession {
    pub fn new(
        id: String,
        source_lang: Lang,
        session_id: Option<String>,
        pipeline_config: Arc<PipelineConfig>,
    ) -> Self {
        Self {
            id,
            source_lang,
            host_tx: None,
            selected_voice_id: None,
            session_id,
            rtmp_manager: None,
            rtmp_langs: Vec::new(),
            pipeline_config,
        }
    }

    /// Unique languages that require a translated RTMP track.
    pub fn active_langs(&self) -> Vec<Lang> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for lang in &self.rtmp_langs {
            if seen.insert(lang.clone()) {
                out.push(lang.clone());
            }
        }
        out
    }

    /// Forward a message to the host WS (transcripts, latency markers, errors).
    pub fn send_to_host(&self, msg: Message) {
        if let Some(tx) = &self.host_tx {
            let _ = tx.send(msg);
        }
    }
}

// ── Shared State ──────────────────────────────────────────

pub type LiveSessions = Arc<DashMap<String, LiveSession>>;

// ── WebSocket Messages (server → client) ──────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
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
        #[serde(rename = "targetLang")]
        target_lang: String,
        #[serde(rename = "translateMs")]
        translate_ms: u64,
    },

    #[serde(rename = "tts_end")]
    TtsEnd {
        #[serde(rename = "utteranceId")]
        utterance_id: u64,
        #[serde(rename = "targetLang")]
        target_lang: String,
        #[serde(rename = "ttsMs")]
        tts_ms: u64,
    },

    /// Per-utterance pipeline-done marker; host UI uses it to finalize latency.
    #[serde(rename = "video_end")]
    VideoEnd {
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
    fn lang_from_str_accepts_supported_values_only() {
        assert_eq!(Lang::from_str("en"), Some(Lang::En));
        assert_eq!(Lang::from_str("ja"), Some(Lang::Ja));
        assert_eq!(Lang::from_str("zh"), Some(Lang::Zh));
        assert_eq!(Lang::from_str("ko"), Some(Lang::Ko));
        assert_eq!(Lang::from_str("EN"), None);
        assert_eq!(Lang::from_str("fr"), None);
    }

    fn test_pipeline_config() -> Arc<PipelineConfig> {
        Arc::new(PipelineConfig {
            soniox_api_key: String::new(),
            soniox_ws_url: String::new(),
            elevenlabs_api_key: String::new(),
            elevenlabs_base_url: String::new(),
        })
    }

    #[test]
    fn active_langs_dedupes_and_preserves_first_seen_order() {
        let mut session = LiveSession::new(
            "ROOM01".into(),
            Lang::En,
            Some("session-1".into()),
            test_pipeline_config(),
        );
        session.rtmp_langs = vec![Lang::Ja, Lang::Ko, Lang::Ja, Lang::Zh, Lang::Ko];

        assert_eq!(session.active_langs(), vec![Lang::Ja, Lang::Ko, Lang::Zh]);
    }
}
