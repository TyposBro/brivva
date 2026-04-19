use crate::features::broadcast::domain::{Lang, LiveSessionHandle, ServerMsg};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite;

use super::soniox::{SONIOX_END_TOKEN, SonioxMode, SonioxResponse};
use super::stt_transport::SonioxStream;
use super::to_ws;
use super::tts::{TtsRequest, broadcast_translated_tts};

pub(super) struct ProcessorArgs {
    pub handle: LiveSessionHandle,
    pub mode: SonioxMode,
    pub tag: String,
    pub utterance_counter: u64,
    pub stt_stream: SonioxStream,
}

pub(super) fn spawn_response_processor(
    args: ProcessorArgs,
) -> tokio::task::JoinHandle<(u64, bool)> {
    let ProcessorArgs {
        handle,
        mode,
        tag,
        utterance_counter,
        mut stt_stream,
    } = args;
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
            if !handle.sessions.contains_key(&handle.id) {
                return (utterance_counter, false);
            }

            let (interim_tail, endpoint_hit) = accumulate_tokens(&mode, &response, &mut final_text);
            emit_interim_if_needed(InterimArgs {
                mode: &mode,
                handle: &handle,
                final_text: &final_text,
                interim_tail: &interim_tail,
            });

            if endpoint_hit {
                utterance_counter = finalize_utterance_if_needed(FinalizeArgs {
                    mode: &mode,
                    utterance_counter,
                    handle: &handle,
                    final_text: &mut final_text,
                })
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

struct InterimArgs<'a> {
    mode: &'a SonioxMode,
    handle: &'a LiveSessionHandle,
    final_text: &'a str,
    interim_tail: &'a str,
}

fn emit_interim_if_needed(args: InterimArgs<'_>) {
    if !matches!(args.mode, SonioxMode::Source { .. }) {
        return;
    }
    let interim = format!("{}{}", args.final_text, args.interim_tail);
    if interim.is_empty() {
        return;
    }
    if let Some(live_session) = args.handle.sessions.get(&args.handle.id) {
        live_session.send_to_host(to_ws(&ServerMsg::Interim {
            transcript: interim,
        }));
    }
}

struct FinalizeArgs<'a> {
    mode: &'a SonioxMode,
    utterance_counter: u64,
    handle: &'a LiveSessionHandle,
    final_text: &'a mut String,
}

async fn finalize_utterance_if_needed(args: FinalizeArgs<'_>) -> u64 {
    let FinalizeArgs {
        mode,
        utterance_counter,
        handle,
        final_text,
    } = args;
    if final_text.trim().is_empty() {
        final_text.clear();
        return utterance_counter;
    }

    let next_utterance_id = utterance_counter + 1;
    let committed = std::mem::take(final_text);
    match mode {
        SonioxMode::Source { .. } => emit_final_source(&committed, next_utterance_id, handle),
        SonioxMode::Translate { target_lang, .. } => {
            emit_translation(EmitTranslationArgs {
                committed: &committed,
                utterance_id: next_utterance_id,
                target_lang,
                handle,
            })
            .await;
        }
    }

    next_utterance_id
}

fn emit_final_source(committed: &str, utterance_id: u64, handle: &LiveSessionHandle) {
    if let Some(live_session) = handle.sessions.get(&handle.id) {
        live_session.send_to_host(to_ws(&ServerMsg::Final {
            transcript: committed.to_string(),
            utterance_id,
        }));
    }
}

struct EmitTranslationArgs<'a> {
    committed: &'a str,
    utterance_id: u64,
    target_lang: &'a Lang,
    handle: &'a LiveSessionHandle,
}

async fn emit_translation(args: EmitTranslationArgs<'_>) {
    let EmitTranslationArgs {
        committed,
        utterance_id,
        target_lang,
        handle,
    } = args;
    let selected_voice_id = handle
        .sessions
        .get(&handle.id)
        .and_then(|session| session.selected_voice_id.clone());
    let rtmp_manager = handle
        .sessions
        .get(&handle.id)
        .and_then(|session| session.rtmp_manager.clone());

    if let Some(live_session) = handle.sessions.get(&handle.id) {
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

    broadcast_translated_tts(TtsRequest {
        text: committed.to_string(),
        utterance_id,
        target_lang: target_lang.clone(),
        handle: handle.clone(),
        selected_voice_id,
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::broadcast::data::pipeline::soniox::SonioxToken;
    use crate::features::broadcast::domain::Lang;

    fn token(text: &str, is_final: bool, translation_status: Option<&str>) -> SonioxToken {
        SonioxToken {
            text: text.to_string(),
            is_final,
            translation_status: translation_status.map(str::to_string),
        }
    }

    #[test]
    fn parse_soniox_response_returns_none_on_empty_string() {
        assert!(parse_soniox_response("tag", "").is_none());
    }

    #[test]
    fn parse_soniox_response_returns_none_on_invalid_json() {
        assert!(parse_soniox_response("tag", "{not json").is_none());
    }

    #[test]
    fn parse_soniox_response_handles_missing_tokens_array() {
        let resp = parse_soniox_response("tag", "{}").expect("empty object is valid");
        assert!(resp.tokens.is_empty());
        assert!(resp.error_code.is_none());
    }

    #[test]
    fn parse_soniox_response_captures_error_fields() {
        let json = r#"{"error_code":"auth_failed","error_message":"bad key"}"#;
        let resp = parse_soniox_response("tag", json).expect("valid json");
        assert_eq!(resp.error_code.as_deref(), Some("auth_failed"));
        assert_eq!(resp.error_message.as_deref(), Some("bad key"));
    }

    #[test]
    fn accumulate_source_mode_joins_final_tokens_and_returns_interim_tail() {
        let mode = SonioxMode::Source { lang: Lang::En };
        let response = SonioxResponse {
            tokens: vec![
                token("hello ", true, None),
                token("world", true, None),
                token(" so", false, None),
            ],
            error_code: None,
            error_message: None,
        };
        let mut final_text = String::new();
        let (interim, endpoint) = accumulate_tokens(&mode, &response, &mut final_text);

        assert_eq!(final_text, "hello world");
        assert_eq!(interim, " so");
        assert!(!endpoint);
    }

    #[test]
    fn accumulate_flags_endpoint_on_end_token_and_skips_its_text() {
        let mode = SonioxMode::Source { lang: Lang::En };
        let response = SonioxResponse {
            tokens: vec![
                token("done ", true, None),
                token(SONIOX_END_TOKEN, true, None),
            ],
            error_code: None,
            error_message: None,
        };
        let mut final_text = String::new();
        let (_, endpoint) = accumulate_tokens(&mode, &response, &mut final_text);

        assert_eq!(final_text, "done ");
        assert!(endpoint);
    }

    #[test]
    fn accumulate_translate_mode_drops_tokens_without_translation_status() {
        let mode = SonioxMode::Translate {
            source_lang: Lang::En,
            target_lang: Lang::Ja,
        };
        let response = SonioxResponse {
            tokens: vec![
                token("original ", true, Some("original")),
                token("konnichiwa", true, Some("translation")),
                token(" yo", false, Some("translation")),
            ],
            error_code: None,
            error_message: None,
        };
        let mut final_text = String::new();
        let (interim, _) = accumulate_tokens(&mode, &response, &mut final_text);

        assert_eq!(final_text, "konnichiwa");
        assert_eq!(interim, " yo");
    }

    #[test]
    fn accumulate_preserves_prior_final_text_across_calls() {
        let mode = SonioxMode::Source { lang: Lang::En };
        let mut final_text = "prefix ".to_string();
        let response = SonioxResponse {
            tokens: vec![token("added", true, None)],
            error_code: None,
            error_message: None,
        };
        accumulate_tokens(&mode, &response, &mut final_text);

        assert_eq!(final_text, "prefix added");
    }
}
