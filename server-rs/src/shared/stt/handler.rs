//! Process a single Gladia message and return a control-flow signal.

use tracing::{error, debug};

use super::state::{ExitReason, SttState, SttContext, MessageAction};
use super::final_handler::handle_final_transcript;
use super::interim_handler::handle_interim_transcript;

pub(super) async fn process_gladia_message(
    gm: crate::shared::stt::GladiaMessage,
    state: &mut SttState,
    ctx: &SttContext,
) -> MessageAction {
    if !ctx.sessions.contains_key(&ctx.session_id) {
        return MessageAction::Break;
    }

    if let Some(ref err) = gm.error {
        error!("[STT] Gladia error {}: {}", err.status_code, err.message);
        state.exit_reason = ExitReason::Disconnected;
        return MessageAction::Break;
    }

    match gm.msg_type.as_str() {
        "transcript" => handle_transcript(gm, state, ctx).await,
        "speech_start" => { debug!("[STT] VAD: speech started"); MessageAction::Continue }
        "speech_end" => { debug!("[STT] VAD: speech ended"); MessageAction::Continue }
        _ => MessageAction::Continue,
    }
}

async fn handle_transcript(
    gm: crate::shared::stt::GladiaMessage,
    state: &mut SttState,
    ctx: &SttContext,
) -> MessageAction {
    let transcript = match gm.transcript() {
        Some(t) => t,
        None => return MessageAction::Continue,
    };

    if gm.is_final() {
        handle_final_transcript(state, &transcript, ctx).await;
        if matches!(state.exit_reason, ExitReason::AdaptiveReconnect { .. }) { return MessageAction::Break; }
    } else {
        handle_interim_transcript(state, &transcript, ctx).await;
    }

    MessageAction::Continue
}
