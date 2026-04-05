use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use dashmap::DashMap;
use tokio::sync::mpsc;
use axum::extract::ws::Message;

use crate::core::config::{DEFAULT_BROADCAST_DELAY_MS, DEFAULT_TTS_MODEL};
use crate::core::types::Lang;

/// Type-erased RTMP manager handle.
///
/// Core cannot depend on the concrete `RtmpManager` in features.
/// Feature code stores `Arc<tokio::sync::Mutex<RtmpManager>>` erased as this
/// type, and recovers the concrete type via `Arc::downcast`.
pub type ErasedRtmpManager = Arc<dyn std::any::Any + Send + Sync>;

pub struct Session {
    pub id: String,
    pub source_lang: Lang,
    pub target_langs: Vec<Lang>,
    pub host_tx: Option<mpsc::UnboundedSender<Message>>,
    pub voice_clone_id: Option<String>,
    pub tts_model: String,
    pub tier: u8,
    pub rtmp_manager: Option<ErasedRtmpManager>,
    pub rtmp_langs: Vec<Lang>,
    pub rtmp_stop: Arc<AtomicBool>,
    pub video_codec: Option<String>,
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
            tts_model: DEFAULT_TTS_MODEL.to_string(),
            tier,
            rtmp_manager: None,
            rtmp_langs: Vec::new(),
            rtmp_stop: Arc::new(AtomicBool::new(false)),
            video_codec: None,
            broadcast_delay_ms: DEFAULT_BROADCAST_DELAY_MS,
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
