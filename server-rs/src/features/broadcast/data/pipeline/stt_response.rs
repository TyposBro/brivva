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
    let (selected_voice_id, selected_voice_enrollment_lang, voice_preset) = handle
        .sessions
        .get(&handle.id)
        .map(|session| {
            (
                session.selected_voice_id.clone(),
                session.selected_voice_enrollment_lang.clone(),
                session.voice_preset,
            )
        })
        .unwrap_or((
            None,
            None,
            crate::features::broadcast::domain::VoicePreset::Female,
        ));
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
        selected_voice_enrollment_lang,
        voice_preset,
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

    use crate::features::broadcast::domain::{LiveSession, LiveSessions, PipelineConfig};
    use axum::extract::ws::Message;
    use dashmap::DashMap;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn session_with_host_tx(id: &str) -> (LiveSessions, mpsc::UnboundedReceiver<Message>) {
        let sessions: LiveSessions = Arc::new(DashMap::new());
        let (tx, rx) = mpsc::unbounded_channel();
        let mut session = LiveSession::new(
            id.into(),
            Lang::En,
            None,
            Arc::new(PipelineConfig::default()),
        );
        session.host_tx = Some(tx);
        sessions.insert(id.into(), session);
        (sessions, rx)
    }

    #[test]
    fn emit_interim_in_source_mode_forwards_joined_text_to_host() {
        let (sessions, mut rx) = session_with_host_tx("room");
        let handle = LiveSessionHandle::new("room".into(), sessions);
        emit_interim_if_needed(InterimArgs {
            mode: &SonioxMode::Source { lang: Lang::En },
            handle: &handle,
            final_text: "hello ",
            interim_tail: "world",
        });

        match rx.try_recv().expect("message forwarded") {
            Message::Text(t) => {
                assert!(t.as_str().contains("\"type\":\"interim\""));
                assert!(t.as_str().contains("hello world"));
            }
            other => panic!("expected text message, got {other:?}"),
        }
    }

    #[test]
    fn emit_interim_skips_send_when_mode_is_translate() {
        let (sessions, mut rx) = session_with_host_tx("room");
        let handle = LiveSessionHandle::new("room".into(), sessions);
        emit_interim_if_needed(InterimArgs {
            mode: &SonioxMode::Translate {
                source_lang: Lang::En,
                target_lang: Lang::Ja,
            },
            handle: &handle,
            final_text: "x",
            interim_tail: "y",
        });
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn emit_interim_skips_send_when_buffer_is_empty() {
        let (sessions, mut rx) = session_with_host_tx("room");
        let handle = LiveSessionHandle::new("room".into(), sessions);
        emit_interim_if_needed(InterimArgs {
            mode: &SonioxMode::Source { lang: Lang::En },
            handle: &handle,
            final_text: "",
            interim_tail: "",
        });
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn emit_final_source_forwards_full_message_to_host() {
        let (sessions, mut rx) = session_with_host_tx("room");
        let handle = LiveSessionHandle::new("room".into(), sessions);
        emit_final_source("commit", 7, &handle);
        match rx.try_recv().expect("message forwarded") {
            Message::Text(t) => {
                assert!(t.as_str().contains("\"type\":\"final\""));
                assert!(t.as_str().contains("\"utteranceId\":7"));
                assert!(t.as_str().contains("\"transcript\":\"commit\""));
            }
            other => panic!("expected text message, got {other:?}"),
        }
    }

    #[test]
    fn emit_final_source_is_noop_when_live_session_missing() {
        let sessions: LiveSessions = Arc::new(DashMap::new());
        let handle = LiveSessionHandle::new("missing".into(), sessions);
        emit_final_source("commit", 1, &handle);
    }

    #[tokio::test]
    async fn finalize_utterance_drops_empty_text_without_bumping_counter() {
        let (sessions, _rx) = session_with_host_tx("room");
        let handle = LiveSessionHandle::new("room".into(), sessions);
        let mut text = "   ".to_string();
        let new_counter = finalize_utterance_if_needed(FinalizeArgs {
            mode: &SonioxMode::Source { lang: Lang::En },
            utterance_counter: 5,
            handle: &handle,
            final_text: &mut text,
        })
        .await;
        assert_eq!(new_counter, 5);
        assert!(text.is_empty());
    }

    #[tokio::test]
    async fn finalize_utterance_commits_non_empty_source_text_and_bumps_counter() {
        let (sessions, mut rx) = session_with_host_tx("room");
        let handle = LiveSessionHandle::new("room".into(), sessions);
        let mut text = "hello".to_string();
        let new_counter = finalize_utterance_if_needed(FinalizeArgs {
            mode: &SonioxMode::Source { lang: Lang::En },
            utterance_counter: 3,
            handle: &handle,
            final_text: &mut text,
        })
        .await;
        assert_eq!(new_counter, 4);
        assert!(text.is_empty());
        match rx.try_recv().expect("forwarded") {
            Message::Text(t) => assert!(t.as_str().contains("\"utteranceId\":4")),
            other => panic!("expected text message, got {other:?}"),
        }
    }

    async fn spawn_ws_soniox_fixture(
        messages: Vec<String>,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            use futures_util::SinkExt;
            for m in messages {
                if ws
                    .send(tokio_tungstenite::tungstenite::Message::Text(m.into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            let _ = ws.close(None).await;
        });
        (addr, handle)
    }

    #[tokio::test]
    async fn spawn_response_processor_emits_final_transcript_on_end_token_in_source_mode() {
        let fixture = vec![
            serde_json::json!({
                "tokens": [
                    {"text": "hello ", "is_final": true},
                    {"text": "world", "is_final": true},
                    {"text": super::super::soniox::SONIOX_END_TOKEN, "is_final": true},
                ]
            })
            .to_string(),
        ];
        let (addr, server) = spawn_ws_soniox_fixture(fixture).await;

        let url = format!("ws://{}", addr);
        let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        use futures_util::StreamExt;
        let (_sink, stream) = ws.split();

        let (sessions, mut rx) = session_with_host_tx("room");
        let handle = LiveSessionHandle::new("room".into(), sessions);

        let processor = spawn_response_processor(ProcessorArgs {
            handle,
            mode: SonioxMode::Source { lang: Lang::En },
            tag: "tag".into(),
            utterance_counter: 0,
            stt_stream: stream,
        });

        let (counter, disconnected) =
            tokio::time::timeout(std::time::Duration::from_secs(2), processor)
                .await
                .expect("processor completes")
                .expect("task panic");
        assert_eq!(counter, 1, "end-token should bump utterance counter by 1");
        assert!(disconnected);

        let mut saw_final = false;
        while let Ok(msg) = rx.try_recv() {
            if let Message::Text(t) = msg
                && t.as_str().contains("\"type\":\"final\"")
                && t.as_str().contains("\"transcript\":\"hello world\"")
            {
                saw_final = true;
            }
        }
        assert!(
            saw_final,
            "expected final host message with joined transcript"
        );

        server.abort();
    }

    #[tokio::test]
    async fn emit_translation_sends_translation_message_and_triggers_push_caption_when_manager_present()
     {
        use crate::features::broadcast::data::ffmpeg::RtmpManager;

        let (sessions, mut rx) = session_with_host_tx("room");
        // Attach an empty RtmpManager so the push_caption branch is exercised.
        {
            let mut session = sessions.get_mut("room").unwrap();
            session.rtmp_manager = Some(Arc::new(tokio::sync::Mutex::new(RtmpManager::new())));
        }
        let handle = LiveSessionHandle::new("room".into(), sessions);

        super::emit_translation(super::EmitTranslationArgs {
            committed: "konnichiwa",
            utterance_id: 2,
            target_lang: &Lang::Ja,
            handle: &handle,
        })
        .await;

        let mut saw_translation = false;
        while let Ok(Message::Text(t)) = rx.try_recv() {
            if t.as_str().contains("\"type\":\"translation\"") {
                saw_translation = true;
            }
        }
        assert!(saw_translation);
    }

    #[tokio::test]
    async fn spawn_response_processor_exits_when_soniox_reports_error_code() {
        let fixture = vec![
            serde_json::json!({
                "error_code": "auth_failed",
                "error_message": "bad key"
            })
            .to_string(),
        ];
        let (addr, server) = spawn_ws_soniox_fixture(fixture).await;
        let url = format!("ws://{}", addr);
        let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        use futures_util::StreamExt;
        let (_sink, stream) = ws.split();

        let (sessions, _rx) = session_with_host_tx("room");
        let handle = LiveSessionHandle::new("room".into(), sessions);
        let processor = spawn_response_processor(ProcessorArgs {
            handle,
            mode: SonioxMode::Source { lang: Lang::En },
            tag: "tag".into(),
            utterance_counter: 0,
            stt_stream: stream,
        });
        let (_counter, disconnected) =
            tokio::time::timeout(std::time::Duration::from_secs(2), processor)
                .await
                .expect("processor completes")
                .expect("task panic");
        // Current contract: error_code → return (counter, true) — treat as
        // disconnect so outer loop may attempt reconnect.
        assert!(disconnected);
        server.abort();
    }
}
