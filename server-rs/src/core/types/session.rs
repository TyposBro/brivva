use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use dashmap::DashMap;
use tokio::sync::mpsc;
use axum::extract::ws::Message;

use crate::core::config::{DEFAULT_BROADCAST_DELAY_MS, DEFAULT_TTS_MODEL, DEFAULT_TTS_PROVIDER};
use crate::core::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
use crate::core::latency_tracker::LatencyTracker;
use crate::core::pipeline_counters::PipelineCounters;
use crate::core::types::Lang;
use crate::shared::recording::SessionRecorder;

/// Type-erased RTMP manager handle.
///
/// Core cannot depend on the concrete `RtmpManager` in features.
/// Feature code stores `Arc<tokio::sync::Mutex<RtmpManager>>` erased as this
/// type, and recovers the concrete type via `Arc::downcast`.
pub type ErasedRtmpManager = Arc<dyn std::any::Any + Send + Sync>;

/// Circuit breaker config for TTS and STT APIs.
const CB_FAILURE_THRESHOLD: u32 = 5;
const CB_RESET_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub struct Session {
    pub id: String,
    pub source_lang: Lang,
    pub target_langs: Vec<Lang>,
    pub host_tx: Option<mpsc::UnboundedSender<Message>>,
    pub voice_clone_id: Option<String>,
    /// Languages that skip clone and use the provider's built-in default voice.
    pub use_default_voice_langs: std::collections::HashSet<String>,
    /// "female" or "male" — global fallback for default voice gender.
    pub tts_voice_gender: String,
    /// Per-language gender override. Key = lang code, value = "female" | "male".
    pub voice_gender_map: std::collections::HashMap<String, String>,
    pub tts_model: String,
    pub tts_provider: String,
    pub tier: u8,
    pub rtmp_manager: Option<ErasedRtmpManager>,
    pub rtmp_langs: Vec<Lang>,
    pub rtmp_stop: Arc<AtomicBool>,
    pub video_codec: Option<String>,
    pub broadcast_delay_ms: u64,
    /// Per-language video delay (ms). Used for TTS deadline + video hold.
    /// Overrides broadcast_delay_ms when present for a given language.
    pub lang_delay_ms: std::collections::HashMap<String, u64>,
    pub pipeline_counters: Arc<PipelineCounters>,
    pub latency_tracker: Arc<LatencyTracker>,
    pub tts_circuit_breaker: Arc<CircuitBreaker>,
    pub stt_circuit_breaker: Arc<CircuitBreaker>,
    pub recorder: Option<Arc<SessionRecorder>>,
}

impl Session {
    pub fn new(id: String, source_lang: Lang, target_langs: Vec<Lang>, tier: u8) -> Self {
        let cb_config = || CircuitBreakerConfig {
            failure_threshold: CB_FAILURE_THRESHOLD,
            reset_timeout: CB_RESET_TIMEOUT,
        };
        Self {
            id,
            source_lang,
            target_langs,
            host_tx: None,
            voice_clone_id: None,
            use_default_voice_langs: std::collections::HashSet::new(),
            tts_voice_gender: "female".to_string(),
            voice_gender_map: std::collections::HashMap::new(),
            tts_model: DEFAULT_TTS_MODEL.to_string(),
            tts_provider: DEFAULT_TTS_PROVIDER.to_string(),
            tier,
            rtmp_manager: None,
            rtmp_langs: Vec::new(),
            rtmp_stop: Arc::new(AtomicBool::new(false)),
            video_codec: None,
            broadcast_delay_ms: DEFAULT_BROADCAST_DELAY_MS,
            lang_delay_ms: std::collections::HashMap::new(),
            pipeline_counters: Arc::new(PipelineCounters::new()),
            latency_tracker: Arc::new(LatencyTracker::new()),
            tts_circuit_breaker: Arc::new(CircuitBreaker::new(cb_config())),
            stt_circuit_breaker: Arc::new(CircuitBreaker::new(cb_config())),
            recorder: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{DEFAULT_BROADCAST_DELAY_MS, DEFAULT_TTS_MODEL};

    fn make_session() -> Session {
        Session::new(
            "sess-1".to_string(),
            Lang::En,
            vec![Lang::Ja, Lang::Zh],
            2,
        )
    }

    #[test]
    fn should_set_id_source_and_targets_from_constructor() {
        let session = make_session();

        assert_eq!(session.id, "sess-1");
        assert_eq!(session.source_lang, Lang::En);
        assert_eq!(session.target_langs, vec![Lang::Ja, Lang::Zh]);
        assert_eq!(session.tier, 2);
    }

    #[test]
    fn should_default_tts_model_to_constant() {
        let session = make_session();

        assert_eq!(session.tts_model, DEFAULT_TTS_MODEL);
    }

    #[test]
    fn should_default_broadcast_delay_to_constant() {
        let session = make_session();

        assert_eq!(session.broadcast_delay_ms, DEFAULT_BROADCAST_DELAY_MS);
    }

    #[test]
    fn should_default_rtmp_manager_to_none() {
        let session = make_session();

        assert!(session.rtmp_manager.is_none());
    }

    #[test]
    fn should_default_host_tx_to_none() {
        let session = make_session();

        assert!(session.host_tx.is_none());
    }

    #[test]
    fn should_default_voice_clone_id_to_none() {
        let session = make_session();

        assert!(session.voice_clone_id.is_none());
    }

    #[test]
    fn should_default_video_codec_to_none() {
        let session = make_session();

        assert!(session.video_codec.is_none());
    }

    #[test]
    fn should_default_rtmp_langs_to_empty() {
        let session = make_session();

        assert!(session.rtmp_langs.is_empty());
    }

    #[test]
    fn should_default_rtmp_stop_to_false() {
        let session = make_session();

        assert!(!session.rtmp_stop.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn should_return_only_target_langs_when_no_rtmp_langs() {
        let session = make_session();

        let active = session.active_langs();

        assert_eq!(active, vec![Lang::Ja, Lang::Zh]);
    }

    #[test]
    fn should_not_duplicate_langs_when_rtmp_overlaps_target() {
        let mut session = make_session();
        session.rtmp_langs = vec![Lang::Ja];

        let active = session.active_langs();

        assert_eq!(active, vec![Lang::Ja, Lang::Zh]);
    }

    #[test]
    fn should_append_new_rtmp_langs_not_in_target() {
        let mut session = make_session();
        session.rtmp_langs = vec![Lang::Ko];

        let active = session.active_langs();

        assert_eq!(active, vec![Lang::Ja, Lang::Zh, Lang::Ko]);
    }

    #[test]
    fn should_merge_overlapping_and_new_rtmp_langs() {
        let mut session = make_session();
        session.rtmp_langs = vec![Lang::Ja, Lang::Ko];

        let active = session.active_langs();

        assert_eq!(active, vec![Lang::Ja, Lang::Zh, Lang::Ko]);
    }

    #[test]
    fn should_return_empty_when_both_target_and_rtmp_langs_are_empty() {
        let session = Session::new(
            "sess-empty".to_string(),
            Lang::En,
            vec![],
            1,
        );

        let active = session.active_langs();

        assert!(active.is_empty());
    }

    #[test]
    fn should_return_rtmp_langs_when_target_langs_are_empty() {
        let mut session = Session::new(
            "sess-rtmp-only".to_string(),
            Lang::En,
            vec![],
            1,
        );
        session.rtmp_langs = vec![Lang::Ko, Lang::Zh];

        let active = session.active_langs();

        assert_eq!(active, vec![Lang::Ko, Lang::Zh]);
    }

    #[test]
    fn should_not_duplicate_when_rtmp_langs_fully_overlap_target_langs() {
        let mut session = make_session();
        session.rtmp_langs = vec![Lang::Ja, Lang::Zh];

        let active = session.active_langs();

        assert_eq!(active, vec![Lang::Ja, Lang::Zh]);
    }

    #[test]
    fn should_silently_succeed_send_to_host_when_host_tx_is_none() {
        let session = make_session();

        session.send_to_host(Message::Text("hello".to_string().into()));

        assert!(session.host_tx.is_none());
    }

    #[test]
    fn should_deliver_message_via_send_to_host_when_host_tx_is_set() {
        let mut session = make_session();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        session.host_tx = Some(tx);

        session.send_to_host(Message::Text("ping".to_string().into()));

        let received = rx.try_recv().unwrap();
        assert_eq!(received, Message::Text("ping".to_string().into()));
    }
}
