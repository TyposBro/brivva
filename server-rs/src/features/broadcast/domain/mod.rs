pub mod gpu_worker_local;
pub mod gpu_worker_shadow;
pub mod media_timeline;
pub mod metrics;
pub mod output_health;
pub mod render_graph;

use axum::extract::ws::Message;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use webrtc::peer_connection::RTCPeerConnection;

pub use metrics::SessionMetrics;

// Fargate is host-only. There are no guest WebSockets — all translated
// audio leaves the server via RTMP to streaming platforms. The WS exists
// to accept host audio/control, signal host WebRTC video, and feed
// per-utterance progress + errors back to the host UI.

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

    /// Curated female default voice for this language from the user's
    /// ElevenLabs library. All support 32 languages via eleven_flash_v2_5.
    pub fn voice_id_female(&self) -> &'static str {
        match self {
            Lang::En => "4CrZuIW9am7gYAxgo2Af",
            Lang::Ja => "xwDy9oDEtzWzFo6FqAI9",
            Lang::Zh => "9lHjugDhwqoxA5MhX0az",
            Lang::Ko => "zgDzx5jLLCqEp6Fl7Kl7", // Jessica-ko
        }
    }

    /// Male counterpart for the female default. Same 32-lang model support.
    pub fn voice_id_male(&self) -> &'static str {
        match self {
            Lang::En => "JdwJ7jL68CWmQZuo7KgG",
            Lang::Ja => "LIisRj2veIKEBdr6KZ5y",
            Lang::Zh => "brChkoggsUHF1stW6omH",
            Lang::Ko => "m3gJBS8OofDJfycyA2Ip", // Eric-ko
        }
    }

    /// Back-compat alias for the historical single-default getter. Callers
    /// that don't yet know about voice presets keep the pre-split behaviour
    /// (female default) until they're migrated.
    pub fn voice_id(&self) -> &'static str {
        self.voice_id_female()
    }

    /// ISO-639-1 lowercase two-letter code expected by the ElevenLabs TTS
    /// request body's `language_code` field. For `eleven_multilingual_v2`
    /// (cloned voices) passing this is required to keep the inference path
    /// anchored to the enrollment language — omitting it defaults the model
    /// to the English path, which produced the April 2026 Indian-accent
    /// regression when a Korean host synthesised English with a KO clone.
    pub fn to_elevenlabs_code(&self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ja => "ja",
            Lang::Zh => "zh",
            Lang::Ko => "ko",
        }
    }
}

/// Which library default to use when the session hasn't picked a cloned voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoicePreset {
    Cloned,
    Female,
    Male,
}

impl VoicePreset {
    pub fn from_wire(s: &str) -> Self {
        match s {
            "cloned" => Self::Cloned,
            "male" => Self::Male,
            _ => Self::Female,
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

pub struct PipelineConfig {
    pub soniox_api_key: String,
    pub soniox_ws_url: String,
    pub elevenlabs_api_key: String,
    pub elevenlabs_base_url: String,
    /// Kill-switch: force default voice library, skip cloning. Forwarded
    /// from BroadcastState so TTS dispatch can consult without reaching
    /// up into orchestration. See `docs/runbook.md`.
    pub force_default_voice: bool,
    /// Kill-switch: downgrade `rtmps://` to `rtmp://` at FFmpeg spawn
    /// when a platform's TLS is flaking. See `docs/runbook.md`.
    pub force_rtmp_not_rtmps: bool,
    /// V2 Phase 3 output health/control logs. Logs/contracts only; no process control.
    pub v2_output_controls: bool,
    /// V2 Phase 4 render graph adapter logs. Wraps current FFmpeg path only.
    pub v2_render_graph: bool,
    /// V2 Phase 5 shared decode/fan-out planning. Shadow/contracts only.
    pub v2_shared_decode: bool,
    /// V2 Phase 6A GPU worker shadow proof. Logs/contracts only; no live route.
    pub v2_gpu_workers: bool,
    /// V2 FFmpeg tee fanout. One encoder per compatible language group,
    /// publishing to multiple RTMP destinations. Off by default because
    /// Grip/librtmp needs production validation on tee muxer failure modes.
    pub v2_encoded_fanout: bool,
    /// FFmpeg video encoder backend for this session. Injected from
    /// orchestration so the media layer does not read env vars.
    pub video_encoder: VideoEncoderKind,
    /// Runtime output cap for adaptive capture profiles. Weak nodes can stay
    /// 1080p30; GPU nodes can opt into 4K/high-fps.
    pub video_max_width: u32,
    pub video_max_height: u32,
    pub video_max_fps: u32,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            soniox_api_key: String::new(),
            soniox_ws_url: String::new(),
            elevenlabs_api_key: String::new(),
            elevenlabs_base_url: String::new(),
            force_default_voice: false,
            force_rtmp_not_rtmps: false,
            v2_output_controls: false,
            v2_render_graph: false,
            v2_shared_decode: false,
            v2_gpu_workers: false,
            v2_encoded_fanout: false,
            video_encoder: VideoEncoderKind::X264,
            video_max_width: 1920,
            video_max_height: 1080,
            video_max_fps: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoEncoderKind {
    X264,
    Nvenc,
}

impl Default for VideoEncoderKind {
    fn default() -> Self {
        Self::X264
    }
}

impl VideoEncoderKind {
    pub fn from_wire(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "nvenc" | "h264_nvenc" => Self::Nvenc,
            _ => Self::X264,
        }
    }

    pub fn codec_name(self) -> &'static str {
        match self {
            Self::X264 => "libx264",
            Self::Nvenc => "h264_nvenc",
        }
    }
}

/// Handle to a single live session — the pair every pipeline function needs
/// to look the session up. Carried by value so downstream tasks can move it
/// into `tokio::spawn` without additional cloning at the call site.
#[derive(Clone)]
pub struct LiveSessionHandle {
    pub id: String,
    pub sessions: LiveSessions,
}

impl LiveSessionHandle {
    pub fn new(id: String, sessions: LiveSessions) -> Self {
        Self { id, sessions }
    }
}

// ── TTS dispatch request ──────────────────────────────────

/// All inputs the per-lang TTS worker needs to render + broadcast a single
/// utterance. Lives in the domain layer (rather than next to the ElevenLabs
/// client) because `LiveSession.tts_workers` stores `mpsc::Sender<TtsRequest>`
/// and the domain layer cannot depend on `data/`. The data-layer worker
/// consumes requests from the channel and performs the network I/O.
pub struct TtsRequest {
    pub text: String,
    pub utterance_id: u64,
    pub target_lang: Lang,
    pub handle: LiveSessionHandle,
    pub selected_voice_id: Option<String>,
    pub selected_voice_enrollment_lang: Option<Lang>,
    pub voice_preset: VoicePreset,
}

// ── Live Session Runtime ──────────────────────────────────

pub struct LiveSession {
    pub id: String,
    pub source_lang: Lang,
    pub host_tx: Option<mpsc::UnboundedSender<Message>>,
    /// Voice ID currently selected for TTS. Workers owns voice lifecycle and
    /// provides this durable ElevenLabs voice id through the session bundle.
    pub selected_voice_id: Option<String>,
    /// Enrollment language of the cloned voice, when the session bundle
    /// carries it. Used by the TTS dispatcher to detect cross-lingual
    /// mismatch and fall back to the default voice rather than let v2 drift
    /// (the April 2026 Indian-accent regression). None today because the
    /// Workers `Voice` schema does not carry an `enrollment_lang` field yet;
    /// the plumbing is wired so switching to populated is a one-line change.
    pub selected_voice_enrollment_lang: Option<Lang>,
    /// Host-picked voice preset for this session. Populated from the Workers
    /// session bundle at bootstrap; defaults to female when absent.
    pub voice_preset: VoicePreset,
    /// Session ID from Workers (links to D1 session + streams)
    pub session_id: Option<String>,
    /// FFmpeg RTMP manager for streaming to platforms
    pub rtmp_manager: Option<crate::features::broadcast::data::ffmpeg::SharedRtmpManager>,
    /// Host WebRTC peer carrying encoded camera video into Fargate. Stored so
    /// the peer connection remains alive until session teardown.
    pub webrtc_peer: Option<Arc<RTCPeerConnection>>,
    /// Target languages being streamed via RTMP (one entry per configured stream).
    pub rtmp_langs: Vec<Lang>,
    /// Upstream-service config injected from orchestration at session start.
    pub pipeline_config: Arc<PipelineConfig>,
    /// Billing counters shared with the RTMP drains and the metrics reporter
    /// task. `None` for sessions that don't have a Workers session_id (no
    /// one to report to).
    pub metrics: Option<Arc<SessionMetrics>>,
    /// Per-target-language TTS worker channels. Populated in
    /// `start_stt_pipelines` before the Soniox tasks spawn. The STT response
    /// processor does a non-blocking `try_send` here instead of awaiting TTS
    /// inline — that was the April 2026 backpressure bug where one slow
    /// ElevenLabs call stalled the Soniox read loop. Dropping `LiveSession`
    /// drops every `Sender`, which closes the channels and lets workers exit
    /// naturally on `recv() == None`.
    pub tts_workers: HashMap<Lang, mpsc::Sender<TtsRequest>>,
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
            selected_voice_enrollment_lang: None,
            voice_preset: VoicePreset::Female,
            session_id,
            rtmp_manager: None,
            webrtc_peer: None,
            rtmp_langs: Vec::new(),
            pipeline_config,
            metrics: None,
            tts_workers: HashMap::new(),
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
        Arc::new(PipelineConfig::default())
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

    #[test]
    fn active_langs_returns_empty_when_no_rtmp_langs_registered() {
        let session = LiveSession::new("ROOM".into(), Lang::En, None, test_pipeline_config());
        assert!(session.active_langs().is_empty());
    }

    #[test]
    fn lang_display_uses_iso_code_for_every_variant() {
        assert_eq!(Lang::En.to_string(), "en");
        assert_eq!(Lang::Ja.to_string(), "ja");
        assert_eq!(Lang::Zh.to_string(), "zh");
        assert_eq!(Lang::Ko.to_string(), "ko");
    }

    #[test]
    fn lang_voice_id_returns_distinct_default_per_language() {
        assert_eq!(Lang::En.voice_id(), "4CrZuIW9am7gYAxgo2Af");
        assert_eq!(Lang::Ja.voice_id(), "xwDy9oDEtzWzFo6FqAI9");
        assert_eq!(Lang::Zh.voice_id(), "9lHjugDhwqoxA5MhX0az");
        assert_eq!(Lang::Ko.voice_id(), "zgDzx5jLLCqEp6Fl7Kl7");

        let ids = [
            Lang::En.voice_id(),
            Lang::Ja.voice_id(),
            Lang::Zh.voice_id(),
            Lang::Ko.voice_id(),
        ];
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 4, "every lang should map to a distinct voice");
    }

    #[test]
    fn server_msg_serializes_translation_with_camel_case_external_keys() {
        let msg = ServerMsg::Translation {
            text: "hi".into(),
            utterance_id: 7,
            target_lang: "ja".into(),
            translate_ms: 250,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"translation\""));
        assert!(json.contains("\"utteranceId\":7"));
        assert!(json.contains("\"targetLang\":\"ja\""));
        assert!(json.contains("\"translateMs\":250"));
    }

    #[test]
    fn server_msg_serializes_tts_end_with_camel_case_external_keys() {
        let msg = ServerMsg::TtsEnd {
            utterance_id: 11,
            target_lang: "ko".into(),
            tts_ms: 12,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"tts_end\""));
        assert!(json.contains("\"ttsMs\":12"));
    }

    #[test]
    fn server_msg_serializes_video_end_error_and_final_variants() {
        let final_msg = ServerMsg::Final {
            transcript: "hi".into(),
            utterance_id: 1,
        };
        let video = ServerMsg::VideoEnd { utterance_id: 9 };
        let error = ServerMsg::Error {
            message: "boom".into(),
        };
        let interim = ServerMsg::Interim {
            transcript: "uh".into(),
        };

        assert!(
            serde_json::to_string(&final_msg)
                .unwrap()
                .contains("\"type\":\"final\"")
        );
        assert!(
            serde_json::to_string(&video)
                .unwrap()
                .contains("\"type\":\"video_end\"")
        );
        assert!(
            serde_json::to_string(&error)
                .unwrap()
                .contains("\"type\":\"error\"")
        );
        assert!(
            serde_json::to_string(&interim)
                .unwrap()
                .contains("\"type\":\"interim\"")
        );
    }

    #[test]
    fn live_session_send_to_host_is_noop_when_host_tx_unset() {
        let session = LiveSession::new("ROOM".into(), Lang::En, None, test_pipeline_config());
        session.send_to_host(Message::Text("anything".into()));
    }

    #[test]
    fn live_session_send_to_host_forwards_to_channel_when_set() {
        let mut session = LiveSession::new("ROOM".into(), Lang::En, None, test_pipeline_config());
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        session.host_tx = Some(tx);
        session.send_to_host(Message::Text("hello".into()));
        match rx.try_recv().expect("message forwarded") {
            Message::Text(t) => assert_eq!(t.as_str(), "hello"),
            other => panic!("expected text message, got {:?}", other),
        }
    }

    #[test]
    fn live_session_handle_new_stores_id_and_sessions() {
        let sessions: LiveSessions = Arc::new(DashMap::new());
        let handle = LiveSessionHandle::new("Z".into(), sessions.clone());
        assert_eq!(handle.id, "Z");
        assert_eq!(Arc::as_ptr(&handle.sessions), Arc::as_ptr(&sessions));
    }
}
