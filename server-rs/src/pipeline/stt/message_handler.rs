//! Process a single Gladia message and return a control-flow signal.

use std::sync::Arc;
use tokio_tungstenite::tungstenite;
use tracing::{error, debug};

use crate::types::{Lang, Sessions};

use super::state::{ExitReason, SttState, MessageAction, WsStream};
use super::final_handler::handle_final_transcript;
use super::interim_handler::handle_interim_transcript;

pub(super) async fn process_gladia_message(
    gm: crate::stt::GladiaMessage,
    state: &mut SttState,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    acc_rx: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    sink: &Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
) -> MessageAction {
    if !sessions.contains_key(session_id) {
        return MessageAction::Break;
    }

    if let Some(ref err) = gm.error {
        error!("[STT] Gladia error {}: {}", err.status_code, err.message);
        state.exit_reason = ExitReason::Disconnected;
        return MessageAction::Break;
    }

    match gm.msg_type.as_str() {
        "transcript" => handle_transcript(gm, state, sessions, session_id, source_lang, acc_rx, sink).await,
        "speech_start" => { debug!("[STT] VAD: speech started"); MessageAction::Continue }
        "speech_end" => { debug!("[STT] VAD: speech ended"); MessageAction::Continue }
        _ => MessageAction::Continue,
    }
}

async fn handle_transcript(
    gm: crate::stt::GladiaMessage,
    state: &mut SttState,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    acc_rx: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    sink: &Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
) -> MessageAction {
    let transcript = match gm.transcript() {
        Some(t) => t,
        None => return MessageAction::Continue,
    };

    if gm.is_final() {
        handle_final_transcript(state, &transcript, sessions, session_id, source_lang, acc_rx, sink).await;
        if matches!(state.exit_reason, ExitReason::AdaptiveReconnect { .. }) { return MessageAction::Break; }
    } else {
        handle_interim_transcript(state, &transcript, sessions, session_id, source_lang, acc_rx).await;
    }

    MessageAction::Continue
}
