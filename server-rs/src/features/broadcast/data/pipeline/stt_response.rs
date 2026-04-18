use crate::features::broadcast::domain::{Lang, LiveSessions, ServerMsg};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite;

use super::soniox::{SONIOX_END_TOKEN, SonioxMode, SonioxResponse};
use super::stt_transport::SonioxStream;
use super::to_ws;
use super::tts::broadcast_translated_tts;

pub(super) fn spawn_response_processor(
    live_session_id: &str,
    live_sessions: &LiveSessions,
    mode: SonioxMode,
    tag: String,
    utterance_counter: u64,
    mut stt_stream: SonioxStream,
) -> tokio::task::JoinHandle<(u64, bool)> {
    let live_session_id = live_session_id.to_string();
    let live_sessions = live_sessions.clone();
    tokio::spawn(async move {
        let mut final_text = String::new();
        let mut utterance_counter = utterance_counter;

        while let Some(message) = read_soniox_message(&tag, &mut stt_stream).await {
            let Some(response) = parse_soniox_response(&tag, &message) else {
                continue;
            };
            if response.error_code.is_some() {
                return (utterance_counter, true);
            }
            if !live_sessions.contains_key(&live_session_id) {
                return (utterance_counter, false);
            }

            let (interim_tail, endpoint_hit) = accumulate_tokens(&mode, &response, &mut final_text);
            emit_interim_if_needed(
                &mode,
                &live_sessions,
                &live_session_id,
                &final_text,
                &interim_tail,
            );

            if endpoint_hit {
                utterance_counter = finalize_utterance_if_needed(
                    &mode,
                    utterance_counter,
                    &live_sessions,
                    &live_session_id,
                    &mut final_text,
                )
                .await;
            }
        }

        (utterance_counter, true)
    })
}

async fn read_soniox_message(tag: &str, stt_stream: &mut SonioxStream) -> Option<String> {
    let message = stt_stream.next().await?;
    let message = match message {
        Ok(message) => message,
        Err(error) => {
            eprintln!("[STT {}] WebSocket read error: {}", tag, error);
            return None;
        }
    };

    match message {
        tungstenite::Message::Text(text) => Some(text.to_string()),
        tungstenite::Message::Close(_) => {
            eprintln!("[STT {}] Soniox closed the connection", tag);
            None
        }
        _ => Some(String::new()),
    }
}

fn parse_soniox_response(tag: &str, text: &str) -> Option<SonioxResponse> {
    if text.is_empty() {
        return None;
    }

    match serde_json::from_str::<SonioxResponse>(text) {
        Ok(response) => {
            if let Some(code) = &response.error_code {
                eprintln!(
                    "[STT {}] Soniox error {}: {}",
                    tag,
                    code,
                    response.error_message.clone().unwrap_or_default()
                );
            }
            Some(response)
        }
        Err(error) => {
            eprintln!("[STT {}] parse error: {} — raw: {}", tag, error, text);
            None
        }
    }
}

fn accumulate_tokens(
    mode: &SonioxMode,
    response: &SonioxResponse,
    final_text: &mut String,
) -> (String, bool) {
    let mut interim_tail = String::new();
    let mut endpoint_hit = false;

    for token in &response.tokens {
        if !mode.accepts(token) {
            continue;
        }
        if token.text == SONIOX_END_TOKEN {
            endpoint_hit = true;
            continue;
        }
        if token.is_final {
            final_text.push_str(&token.text);
        } else {
            interim_tail.push_str(&token.text);
        }
    }

    (interim_tail, endpoint_hit)
}

fn emit_interim_if_needed(
    mode: &SonioxMode,
    live_sessions: &LiveSessions,
    live_session_id: &str,
    final_text: &str,
    interim_tail: &str,
) {
    if !matches!(mode, SonioxMode::Source { .. }) {
        return;
    }

    let interim = format!("{}{}", final_text, interim_tail);
    if interim.is_empty() {
        return;
    }

    if let Some(live_session) = live_sessions.get(live_session_id) {
        live_session.send_to_host(to_ws(&ServerMsg::Interim {
            transcript: interim,
        }));
    }
}

async fn finalize_utterance_if_needed(
    mode: &SonioxMode,
    utterance_counter: u64,
    live_sessions: &LiveSessions,
    live_session_id: &str,
    final_text: &mut String,
) -> u64 {
    if final_text.trim().is_empty() {
        final_text.clear();
        return utterance_counter;
    }

    let next_utterance_id = utterance_counter + 1;
    let committed = std::mem::take(final_text);
    match mode {
        SonioxMode::Source { .. } => emit_final_source(
            &committed,
            next_utterance_id,
            live_sessions,
            live_session_id,
        ),
        SonioxMode::Translate { target_lang, .. } => {
            emit_translation(
                &committed,
                next_utterance_id,
                target_lang,
                live_sessions,
                live_session_id,
            )
            .await;
        }
    }

    next_utterance_id
}

fn emit_final_source(
    committed: &str,
    utterance_id: u64,
    live_sessions: &LiveSessions,
    live_session_id: &str,
) {
    if let Some(live_session) = live_sessions.get(live_session_id) {
        live_session.send_to_host(to_ws(&ServerMsg::Final {
            transcript: committed.to_string(),
            utterance_id,
        }));
    }
}

async fn emit_translation(
    committed: &str,
    utterance_id: u64,
    target_lang: &Lang,
    live_sessions: &LiveSessions,
    live_session_id: &str,
) {
    let selected_voice_id = live_sessions
        .get(live_session_id)
        .and_then(|session| session.selected_voice_id.clone());
    let rtmp_manager = live_sessions
        .get(live_session_id)
        .and_then(|session| session.rtmp_manager.clone());

    if let Some(live_session) = live_sessions.get(live_session_id) {
        live_session.send_to_host(to_ws(&ServerMsg::Translation {
            text: committed.to_string(),
            utterance_id,
            target_lang: target_lang.to_string(),
            translate_ms: 0,
        }));
    }

    if let Some(manager) = rtmp_manager {
        let lang = target_lang.to_string();
        let text = committed.to_string();
        tokio::spawn(async move {
            manager.lock().await.push_caption(&lang, text);
        });
    }

    broadcast_translated_tts(
        committed,
        utterance_id,
        target_lang,
        live_sessions,
        live_session_id,
        selected_voice_id.as_deref(),
    )
    .await;
}
