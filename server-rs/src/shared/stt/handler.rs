//! Process a single Soniox message and return a control-flow signal.

use std::time::Instant;
use tracing::{error, info};

use super::state::{ExitReason, SttState, SttContext, MessageAction};
use super::interim_handler::send_interim_to_host;
use super::final_handler::handle_translation_endpoint;
use crate::shared::stt::types::{SonioxResponse, SonioxToken};

pub(super) async fn process_soniox_message(
    text: &str,
    state: &mut SttState,
    ctx: &SttContext,
) -> MessageAction {
    if !ctx.sessions.contains_key(&ctx.session_id) {
        return MessageAction::Break;
    }

    let resp: SonioxResponse = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(_) => return MessageAction::Continue,
    };

    if resp.is_error() {
        log_soniox_error(&resp);
        state.exit_reason = ExitReason::Disconnected;
        return MessageAction::Break;
    }

    if resp.is_finished() {
        return MessageAction::Break;
    }

    process_tokens(&resp.tokens, state, ctx).await
}

fn log_soniox_error(resp: &SonioxResponse) {
    error!(
        "[STT] Soniox error {}: {}",
        resp.error_code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string()),
        resp.error_message.as_deref().unwrap_or("no message"),
    );
}

async fn process_tokens(
    tokens: &[SonioxToken],
    state: &mut SttState,
    ctx: &SttContext,
) -> MessageAction {
    for token in tokens {
        if is_semantic_endpoint(token) {
            handle_endpoint(state, ctx).await;
        } else if token.is_original() {
            handle_original_token(token, state, ctx);
        } else if token.is_translation() {
            handle_translation_token(token, state);
        }
    }
    MessageAction::Continue
}

fn is_semantic_endpoint(token: &SonioxToken) -> bool {
    token.text == "<end>" && token.is_final
}

fn handle_original_token(token: &SonioxToken, state: &mut SttState, ctx: &SttContext) {
    mark_utterance_start(state);
    if token.is_final {
        state.transcript_acc.push_str(&token.text);
    } else {
        send_interim_to_host(ctx, &token.text);
    }
}

fn handle_translation_token(token: &SonioxToken, state: &mut SttState) {
    if token.is_final {
        state.translation_acc.push_str(&token.text);
    }
}

fn mark_utterance_start(state: &mut SttState) {
    if state.utterance_start.is_none() {
        state.utterance_start = Some(Instant::now());
    }
}

async fn handle_endpoint(state: &mut SttState, ctx: &SttContext) {
    state.utterance_counter += 1;
    let uid = state.utterance_counter;

    if state.target_lang.is_some() {
        handle_translation_connection_endpoint(state, uid, ctx).await;
    } else {
        handle_source_connection_endpoint(state, uid, ctx);
    }

    state.reset_utterance();
}

async fn handle_translation_connection_endpoint(
    state: &mut SttState,
    uid: u64,
    ctx: &SttContext,
) {
    let translated = state.translation_acc.clone();
    if translated.is_empty() {
        return;
    }

    info!("[STT] #{} endpoint: translation='{}' (lang={:?})",
        uid, &translated[..translated.len().min(60)], state.target_lang);

    handle_translation_endpoint(state, &translated, ctx).await;
}

fn handle_source_connection_endpoint(state: &mut SttState, uid: u64, ctx: &SttContext) {
    let transcript = state.transcript_acc.clone();
    if transcript.is_empty() {
        return;
    }

    info!("[STT] #{} endpoint: transcript='{}'", uid, &transcript[..transcript.len().min(60)]);
    send_final_to_host(ctx, &transcript, uid);
    queue_source_passthrough(ctx, state);
}

fn send_final_to_host(ctx: &SttContext, transcript: &str, uid: u64) {
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(
            crate::features::broadcast::data::pipeline_helpers::to_ws(
                &crate::core::types::ServerMsg::Final {
                    transcript: transcript.to_string(),
                    utterance_id: uid,
                },
            ),
        );
    }
}

fn queue_source_passthrough(ctx: &SttContext, state: &SttState) {
    let session = match ctx.sessions.get(&ctx.session_id) {
        Some(s) => s,
        None => return,
    };
    if !session.active_langs().contains(&ctx.source_lang) {
        return;
    }
    let erased = match session.rtmp_manager.clone() {
        Some(m) => m,
        None => return,
    };
    let mgr = match crate::features::broadcast::data::streaming::downcast_rtmp_manager(&erased) {
        Some(m) => m,
        None => return,
    };

    let host_audio = drain_host_audio(ctx);
    let lang_str = ctx.source_lang.to_string();
    let start = state.utterance_start.unwrap_or_else(Instant::now);

    tokio::spawn(async move {
        let locked = mgr.lock().await;
        locked.queue_audio(&lang_str, host_audio, start);
    });
}

fn drain_host_audio(ctx: &SttContext) -> Vec<u8> {
    let mut acc = ctx.audio_acc.lock().unwrap();
    acc.drain(..).flatten().collect()
}
