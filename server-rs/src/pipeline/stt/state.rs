//! Mutable state carried across Gladia messages within one WS connection.

use std::time::Instant;
use tokio::sync::mpsc;

/// Mutable state for one STT connection.
pub(super) struct SttState {
    pub utterance_counter: u64,
    pub utterance_start: Option<Instant>,
    pub chunk_detector: Box<dyn crate::stt::ChunkDetector>,
    pub progressive: crate::stt::ProgressiveChunkDetector,
    pub chunk_index: u16,
    pub chunk_pipeline_tx: Option<mpsc::Sender<crate::types::ChunkEvent>>,
    pub last_audio_byte_sent: usize,
    pub wpm_samples: Vec<u32>,
    pub adapted: bool,
    pub disconnected: bool,
    pub needs_adaptive_reconnect: bool,
    pub adaptive_params: Option<(f64, f64)>,
}

impl SttState {
    pub fn new(
        source_lang: &str,
        utterance_counter: u64,
        wpm_samples: Vec<u32>,
        adapted: bool,
    ) -> Self {
        Self {
            utterance_counter,
            utterance_start: None,
            chunk_detector: crate::stt::get_detector(source_lang),
            progressive: crate::stt::ProgressiveChunkDetector::new(source_lang),
            chunk_index: 0,
            chunk_pipeline_tx: None,
            last_audio_byte_sent: 0,
            wpm_samples,
            adapted,
            disconnected: false,
            needs_adaptive_reconnect: false,
            adaptive_params: None,
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

