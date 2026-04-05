//! Mutable state carried across Gladia messages within one WS connection.

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;

use crate::core::types::Lang; use crate::features::broadcast::domain::Sessions;

/// Immutable session context shared across all STT message handlers.
#[derive(Clone)]
pub(super) struct SttContext {
    pub sessions: Sessions,
    pub session_id: String,
    pub source_lang: Lang,
    pub audio_acc: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    pub sink: Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
    pub translate_api_key: String,
    pub tts_api_key: String,
    pub default_voice: String,
    pub http_client: reqwest::Client,
}

/// Why the STT connection exited (or hasn't yet).
#[derive(Debug)]
pub(super) enum ExitReason {
    /// Connection is still active (no exit yet).
    Running,
    /// WebSocket closed or errored — standard reconnect.
    Disconnected,
    /// Need to reconnect with new endpointing/duration params.
    AdaptiveReconnect { endpointing: f64, max_duration: f64 },
}

/// Mutable state for one STT connection.
pub(super) struct SttState {
    pub utterance_counter: u64,
    pub utterance_start: Option<Instant>,
    pub chunk_detector: Box<dyn crate::shared::stt::ChunkDetector>,
    pub progressive: crate::shared::stt::ProgressiveChunkDetector,
    pub chunk_index: u16,
    pub chunk_pipeline_tx: Option<mpsc::Sender<crate::core::types::ChunkEvent>>,
    pub last_audio_byte_sent: usize,
    pub wpm_samples: Vec<u32>,
    pub adapted: bool,
    pub exit_reason: ExitReason,
}

/// Carry-over state from a previous STT connection for seamless reconnects.
pub(super) struct SttCarryOver {
    pub utterance_counter: u64,
    pub wpm_samples: Vec<u32>,
    pub adapted: bool,
}

impl SttState {
    pub fn new(source_lang: &str, carry: SttCarryOver) -> Self {
        Self {
            utterance_counter: carry.utterance_counter,
            utterance_start: None,
            chunk_detector: crate::shared::stt::get_detector(source_lang),
            progressive: crate::shared::stt::ProgressiveChunkDetector::new(source_lang),
            chunk_index: 0,
            chunk_pipeline_tx: None,
            last_audio_byte_sent: 0,
            wpm_samples: carry.wpm_samples,
            adapted: carry.adapted,
            exit_reason: ExitReason::Running,
        }
    }

    pub fn reset_utterance(&mut self) {
        self.progressive.reset();
        self.chunk_index = 0;
        self.last_audio_byte_sent = 0;
        self.chunk_detector.reset();
    }
}

/// Control-flow signal returned by message processing.
pub(super) enum MessageAction {
    Continue,
    Break,
}

/// WebSocket stream type alias.
pub(super) type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

