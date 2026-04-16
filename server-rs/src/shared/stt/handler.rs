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
        log_soniox_error(&resp, ctx);
        state.exit_reason = ExitReason::Disconnected;
        return MessageAction::Break;
    }

    if resp.is_finished() {
        return MessageAction::Break;
    }

    process_tokens(&resp.tokens, state, ctx).await
}

fn log_soniox_error(resp: &SonioxResponse, ctx: &SttContext) {
    let code = resp.error_code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
    let msg = resp.error_message.as_deref().unwrap_or("no message");
    error!("[STT] Soniox error {}: {}", code, msg);
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(
            crate::features::broadcast::data::pipeline_helpers::to_ws(
                &crate::core::types::ServerMsg::PipelineWarning {
                    kind: "stt_error".to_string(),
                    lang: ctx.source_lang.to_string(),
                    detail: format!("Soniox error {}: {}", code, msg),
                    utterance_id: 0,
                },
            ),
        );
    }
}

async fn process_tokens(
    tokens: &[SonioxToken],
    state: &mut SttState,
    ctx: &SttContext,
) -> MessageAction {
    for token in tokens {
        if is_semantic_endpoint(token) {
            handle_endpoint(state, ctx).await;
        } else if token.is_translation() {
            handle_translation_token(token, state);
        } else {
            handle_original_token(token, state, ctx);
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
    }
    if state.is_transcript_provider {
        let interim = format!("{}{}", state.transcript_acc, token.text);
        send_interim_to_host(ctx, interim.trim());
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

    if state.is_transcript_provider {
        send_transcript_final(state, uid, ctx);
        queue_source_passthrough(ctx, state);
    }

    if state.target_lang.is_some() {
        send_translation(state, uid, ctx).await;
    }

    state.reset_utterance();
}

fn send_transcript_final(state: &SttState, uid: u64, ctx: &SttContext) {
    let transcript = &state.transcript_acc;
    if transcript.is_empty() {
        return;
    }
    info!("[STT] #{} endpoint: transcript='{}'", uid, truncate_str(transcript, 80));
    send_final_to_host(ctx, transcript, uid);
}

async fn send_translation(state: &mut SttState, uid: u64, ctx: &SttContext) {
    let translated = state.translation_acc.clone();
    let translated = translated.trim().to_string();
    if translated.is_empty() {
        increment_translation_empty(ctx);
        return;
    }
    info!("[STT] #{} endpoint: translation='{}' (lang={:?})",
        uid, truncate_str(&translated, 80), state.target_lang);
    handle_translation_endpoint(state, &translated, ctx).await;
}

fn increment_translation_empty(ctx: &SttContext) {
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.pipeline_counters.increment_translation_empty();
    }
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

fn queue_source_passthrough(ctx: &SttContext, _state: &SttState) {
    let session = match ctx.sessions.get(&ctx.session_id) {
        Some(s) => s,
        None => {
            tracing::warn!("[PASSTHROUGH] session {} gone, skipping", ctx.session_id);
            return;
        }
    };
    if !session.active_langs().contains(&ctx.source_lang) {
        return;
    }
    let erased = match session.rtmp_manager.clone() {
        Some(m) => m,
        None => {
            tracing::warn!("[PASSTHROUGH] no RTMP manager for {}, audio dropped", ctx.source_lang);
            return;
        }
    };
    let mgr = match crate::features::broadcast::data::streaming::downcast_rtmp_manager(&erased) {
        Some(m) => m,
        None => {
            tracing::error!("[PASSTHROUGH] RTMP manager downcast failed for {}", ctx.source_lang);
            return;
        }
    };

    let host_audio = drain_host_audio(ctx);
    let lang_str = ctx.source_lang.to_string();

    tokio::spawn(async move {
        let mut locked = mgr.lock().await;
        locked.queue_audio(&lang_str, host_audio);
    });
}

fn drain_host_audio(ctx: &SttContext) -> Vec<u8> {
    let mut acc = ctx.audio_acc.lock().unwrap();
    acc.drain(..).flatten().collect()
}

fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
