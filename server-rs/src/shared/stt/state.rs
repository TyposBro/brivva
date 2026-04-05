//! Mutable state carried across Soniox messages within one WS connection.

use std::sync::Arc;
use std::time::Instant;

use crate::core::types::{Lang, Sessions};

/// Immutable session context shared across all STT message handlers.
#[derive(Clone)]
pub(super) struct SttContext {
    pub sessions: Sessions,
    pub session_id: String,
    pub source_lang: Lang,
    pub audio_acc: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    pub tts_api_key: String,
    pub default_voice: String,
    pub http_client: reqwest::Client,
}

/// Why the STT connection exited (or hasn't yet).
#[derive(Debug)]
pub(super) enum ExitReason {
    Running,
    Disconnected,
}

/// Mutable state for one STT connection.
pub(super) struct SttState {
    pub utterance_counter: u64,
    pub utterance_start: Option<Instant>,
    pub transcript_acc: String,
    pub translation_acc: String,
    pub target_lang: Option<String>,
    pub is_transcript_provider: bool,
    pub exit_reason: ExitReason,
}

/// Carry-over state from a previous STT connection for seamless reconnects.
pub(super) struct SttCarryOver {
    pub utterance_counter: u64,
}

impl SttState {
    pub fn new(carry: SttCarryOver, target_lang: Option<String>, is_transcript_provider: bool) -> Self {
        Self {
            utterance_counter: carry.utterance_counter,
            utterance_start: None,
            transcript_acc: String::new(),
            translation_acc: String::new(),
            target_lang,
            is_transcript_provider,
            exit_reason: ExitReason::Running,
        }
    }

    pub fn reset_utterance(&mut self) {
        self.transcript_acc.clear();
        self.translation_acc.clear();
        self.utterance_start = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_carry_over() -> SttCarryOver {
        SttCarryOver { utterance_counter: 5 }
    }

    #[test]
    fn should_initialize_state_with_carry_over_counter() {
        let state = SttState::new(make_carry_over(), None, false);

        assert_eq!(state.utterance_counter, 5);
    }

    #[test]
    fn should_initialize_with_empty_accumulators() {
        let state = SttState::new(make_carry_over(), Some("ja".to_string()), false);

        assert!(state.transcript_acc.is_empty());
        assert!(state.translation_acc.is_empty());
    }

    #[test]
    fn should_store_target_lang() {
        let state = SttState::new(make_carry_over(), Some("ja".to_string()), false);

        assert_eq!(state.target_lang, Some("ja".to_string()));
    }

    #[test]
    fn should_default_target_lang_to_none_for_source_connection() {
        let state = SttState::new(make_carry_over(), None, false);

        assert!(state.target_lang.is_none());
    }

    #[test]
    fn should_reset_utterance_clearing_accumulators() {
        let mut state = SttState::new(make_carry_over(), Some("zh".to_string()), false);
        state.transcript_acc.push_str("hello");
        state.translation_acc.push_str("nihao");
        state.utterance_start = Some(Instant::now());

        state.reset_utterance();

        assert!(state.transcript_acc.is_empty());
        assert!(state.translation_acc.is_empty());
        assert!(state.utterance_start.is_none());
    }

    #[test]
    fn should_preserve_utterance_counter_on_reset() {
        let mut state = SttState::new(make_carry_over(), None, false);
        state.utterance_counter = 10;

        state.reset_utterance();

        assert_eq!(state.utterance_counter, 10);
    }

    #[test]
    fn should_start_with_running_exit_reason() {
        let state = SttState::new(make_carry_over(), None, false);

        assert!(matches!(state.exit_reason, ExitReason::Running));
    }
}
