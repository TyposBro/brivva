use crate::features::broadcast::domain::{Lang, LiveSessionHandle, ServerMsg, TtsRequest};
use futures_util::StreamExt;
use tokio::sync::mpsc::error::TrySendError;
use tokio_tungstenite::tungstenite;

use super::soniox::{SONIOX_END_TOKEN, SonioxMode, SonioxResponse};
use super::stt_transport::SonioxStream;
use super::to_ws;

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
                tracing::debug!(
                    session_id = %handle.id,
                    tag = %tag,
                    "stt response unparseable — skipping frame (parse_soniox_response already logged detail)"
                );
                continue;
            };
            if response.error_code.is_some() {
                tracing::warn!(
                    session_id = %handle.id,
                    tag = %tag,
                    "stt soniox returned error_code — exiting response processor as disconnected"
                );
                return (utterance_counter, true);
            }
            if !handle.sessions.contains_key(&handle.id) {
                tracing::info!(
                    session_id = %handle.id,
                    tag = %tag,
                    "stt session removed mid-response — exiting response processor"
                );
                return (utterance_counter, false);
            }

            let (interim_tail, endpoint_hit) = accumulate_tokens(&mode, &response, &mut final_text);
            emit_interim_if_needed(InterimArgs {
                mode: &mode,
                handle: &handle,
                final_text: &final_text,
                interim_tail: &interim_tail,
            });

            let flush_reason = flush_reason(endpoint_hit, &final_text);
            if let Some(reason) = flush_reason {
                // §0.5.4: operators need to tell "Soniox sent <end>" apart
                // from "our punctuation/length heuristic fired" when
                // diagnosing choppy playback or scrambled sentence breaks.
                tracing::debug!(
                    session_id = %handle.id,
                    tag = %tag,
                    reason = reason,
                    char_count = final_text.chars().count(),
                    "flushing utterance"
                );
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
            tracing::warn!(tag = %tag, error = %error, "stt websocket read error");
            return None;
        }
    };

    match message {
        tungstenite::Message::Text(text) => Some(text.to_string()),
        tungstenite::Message::Close(_) => {
            tracing::info!(tag = %tag, "stt soniox closed the connection");
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
                tracing::warn!(
                    tag = %tag,
                    error_code = %code,
                    error_message = response.error_message.as_deref().unwrap_or(""),
                    "stt soniox returned error_code in response"
                );
            }
            Some(response)
        }
        Err(error) => {
            tracing::warn!(
                tag = %tag,
                error = %error,
                raw_len = text.len(),
                "stt response parse error — dropping frame"
            );
            None
        }
    }
}

/// Decide whether to flush the in-flight utterance now, and why. The old
/// behaviour flushed only when Soniox emitted `<end>` — for fast-talking
/// hosts reading a script with no pauses, that meant one giant utterance
/// that then hit the TTS deadline and got dropped wholesale. We now also
/// flush on sentence-ending punctuation or a hard length ceiling so the
/// translation ships in chewable chunks.
///
/// Order matters: endpoint wins, then length, then punctuation. Reporting
/// the reason is §0.5.4 observability — absent a log line, we can't tell
/// which heuristic is firing in prod.
fn flush_reason(endpoint_hit: bool, final_text: &str) -> Option<&'static str> {
    if endpoint_hit {
        return Some("endpoint");
    }
    let char_count = final_text.chars().count();
    // Minimum chunk size so "Hi." doesn't trigger a flush on its own — short
    // utterances cost as much as long ones to synthesize and piling up tiny
    // chunks shreds ElevenLabs concurrency limits.
    const FLUSH_MIN_CHARS: usize = 30;
    // Hard ceiling. 120 chars ≈ 6-12 seconds of synth at Flash v2.5 rates,
    // which fits comfortably under the 30s TTS deadline.
    const FLUSH_MAX_CHARS: usize = 120;
    if char_count < FLUSH_MIN_CHARS {
        return None;
    }
    if char_count >= FLUSH_MAX_CHARS {
        return Some("length");
    }
    const SENTENCE_ENDERS: &[char] = &['.', '!', '?', '。', '！', '？', ',', '，', ';', '；', '、'];
    if final_text
        .chars()
        .rev()
        .take(3)
        .any(|c| SENTENCE_ENDERS.contains(&c))
    {
        return Some("punctuation");
    }
    None
}

fn accumulate_tokens(
    mode: &SonioxMode,
    response: &SonioxResponse,
    final_text: &mut String,
) -> (String, bool) {
    let mut interim_tail = String::new();
    let mut endpoint_hit = false;

    for token in &response.tokens {
        // AUDIT: normal-flow filter — mode selects source vs translation
        // tokens per the SonioxMode. Not a silent-bug branch.
        if !mode.accepts(token) {
            continue;
        }
        // AUDIT: end-of-utterance sentinel — marks boundary, not a drop.
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
    if let Some(live_session) = handle.sessions.get(&handle.id) {
        live_session.send_to_host(to_ws(&ServerMsg::Translation {
            text: committed.to_string(),
            utterance_id,
            target_lang: target_lang.to_string(),
            translate_ms: 0,
        }));
    }

    dispatch_tts_to_worker(DispatchTtsArgs {
        committed,
        utterance_id,
        target_lang,
        handle,
        selected_voice_id,
        selected_voice_enrollment_lang,
        voice_preset,
    });
}

struct DispatchTtsArgs<'a> {
    committed: &'a str,
    utterance_id: u64,
    target_lang: &'a Lang,
    handle: &'a LiveSessionHandle,
    selected_voice_id: Option<String>,
    selected_voice_enrollment_lang: Option<Lang>,
    voice_preset: crate::features::broadcast::domain::VoicePreset,
}

/// Non-blocking hand-off to the per-lang TTS worker. Before April 2026 this
/// code awaited `broadcast_translated_tts` inline inside the Soniox response
/// loop, which meant a single slow ElevenLabs call stalled every subsequent
/// STT frame — the session looked like Soniox stopped sending transcripts.
/// Now the STT loop dispatches in O(µs) and the worker does the heavy lift.
fn dispatch_tts_to_worker(args: DispatchTtsArgs<'_>) {
    let sender = args
        .handle
        .sessions
        .get(&args.handle.id)
        .and_then(|session| session.tts_workers.get(args.target_lang).cloned());
    let Some(sender) = sender else {
        // §0.5.4: no worker registered — either the session is torn down or
        // ElevenLabs credentials were missing at spawn time. Either way the
        // utterance will not be synthesized; log it so operators don't
        // chase ghost Soniox failures.
        tracing::warn!(
            target_lang = %args.target_lang,
            utterance_id = args.utterance_id,
            "tts dispatch dropped: no worker registered for target_lang"
        );
        return;
    };
    let req = TtsRequest {
        text: args.committed.to_string(),
        utterance_id: args.utterance_id,
        target_lang: args.target_lang.clone(),
        handle: args.handle.clone(),
        selected_voice_id: args.selected_voice_id,
        selected_voice_enrollment_lang: args.selected_voice_enrollment_lang,
        voice_preset: args.voice_preset,
    };
    match sender.try_send(req) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            // §0.5.4: worker backlog saturated — ElevenLabs is likely slow
            // or down. Dropping the new request keeps the STT loop moving
            // forward; an older in-flight utterance will still reach the
            // viewer, which is better than freezing the stream.
            tracing::warn!(
                target_lang = %args.target_lang,
                utterance_id = args.utterance_id,
                "tts dispatch dropped: worker queue full"
            );
        }
        Err(TrySendError::Closed(_)) => {
            // §0.5.4: receiver end gone. Normally this only happens during
            // teardown; if it fires mid-session the worker task crashed.
            tracing::warn!(
                target_lang = %args.target_lang,
                utterance_id = args.utterance_id,
                "tts dispatch dropped: worker channel closed"
            );
        }
    }
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
    async fn emit_translation_sends_translation_message_when_manager_present() {
        use crate::features::broadcast::data::ffmpeg::RtmpManager;

        let (sessions, mut rx) = session_with_host_tx("room");
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

    // ── flush_reason ──────────────────────────────────────────

    #[test]
    fn flush_reason_returns_endpoint_when_soniox_end_token_fired() {
        assert_eq!(flush_reason(true, ""), Some("endpoint"));
        // Endpoint wins even if other heuristics would also fire.
        assert_eq!(flush_reason(true, &"a".repeat(500)), Some("endpoint"));
    }

    #[test]
    fn flush_reason_returns_none_for_text_shorter_than_min_chars() {
        assert_eq!(flush_reason(false, ""), None);
        assert_eq!(flush_reason(false, "Hi."), None);
        assert_eq!(flush_reason(false, &"a".repeat(29)), None);
    }

    #[test]
    fn flush_reason_returns_length_when_text_exceeds_hard_ceiling() {
        // 120 chars with no punctuation — force-flush on length alone so
        // long pauseless monologues don't accumulate into one giant chunk
        // that then blows the TTS deadline.
        assert_eq!(flush_reason(false, &"a".repeat(120)), Some("length"));
        assert_eq!(flush_reason(false, &"b".repeat(500)), Some("length"));
    }

    #[test]
    fn flush_reason_returns_punctuation_when_final_char_is_sentence_end() {
        let text = "This is a long enough sentence.";
        assert!(text.chars().count() >= 30);
        assert_eq!(flush_reason(false, text), Some("punctuation"));
    }

    #[test]
    fn flush_reason_handles_cjk_sentence_enders() {
        // Build each string by padding a 30-char filler with the ender so
        // the min-length gate passes regardless of how many chars the
        // human-readable stem happens to have.
        let ko_base: String = "가".repeat(35);
        let ko = format!("{}。", ko_base);
        assert_eq!(flush_reason(false, &ko), Some("punctuation"));
        let zh_base: String = "中".repeat(35);
        let zh = format!("{}？", zh_base);
        assert_eq!(flush_reason(false, &zh), Some("punctuation"));
        let ja_base: String = "あ".repeat(35);
        let ja = format!("{}、", ja_base);
        assert_eq!(flush_reason(false, &ja), Some("punctuation"));
    }

    #[test]
    fn flush_reason_returns_none_when_over_min_but_no_punctuation_and_under_ceiling() {
        // 30 chars, no punctuation — keep accumulating until we see one.
        let text: String = "x".repeat(40);
        assert_eq!(flush_reason(false, &text), None);
    }

    // ── dispatch_tts_to_worker ────────────────────────────────

    #[tokio::test]
    async fn dispatch_tts_to_worker_forwards_request_when_worker_registered() {
        use crate::features::broadcast::domain::VoicePreset;

        let (sessions, _host_rx) = session_with_host_tx("room");
        let (tts_tx, mut tts_rx) = mpsc::channel::<TtsRequest>(4);
        {
            let mut session = sessions.get_mut("room").unwrap();
            session.tts_workers.insert(Lang::Ja, tts_tx);
        }
        let handle = LiveSessionHandle::new("room".into(), sessions);

        dispatch_tts_to_worker(DispatchTtsArgs {
            committed: "konnichiwa",
            utterance_id: 42,
            target_lang: &Lang::Ja,
            handle: &handle,
            selected_voice_id: None,
            selected_voice_enrollment_lang: None,
            voice_preset: VoicePreset::Female,
        });

        let req = tts_rx.recv().await.expect("worker should receive");
        assert_eq!(req.utterance_id, 42);
        assert_eq!(req.text, "konnichiwa");
        assert_eq!(req.target_lang, Lang::Ja);
    }

    #[tokio::test]
    async fn dispatch_tts_to_worker_drops_silently_when_no_worker_for_target_lang() {
        use crate::features::broadcast::domain::VoicePreset;

        // Session exists but no worker registered for Ja — the dispatch
        // should just log + return. Previously the STT loop awaited TTS
        // inline and would hang here; now it must not panic or block.
        let (sessions, _host_rx) = session_with_host_tx("room");
        let handle = LiveSessionHandle::new("room".into(), sessions);

        dispatch_tts_to_worker(DispatchTtsArgs {
            committed: "hello",
            utterance_id: 1,
            target_lang: &Lang::Ja,
            handle: &handle,
            selected_voice_id: None,
            selected_voice_enrollment_lang: None,
            voice_preset: VoicePreset::Female,
        });
        // Nothing observable beyond no-panic; the §0.5.4 warn is fired by
        // `tracing` into a test-mode sink.
    }

    #[tokio::test]
    async fn dispatch_tts_to_worker_drops_request_when_worker_queue_is_full() {
        use crate::features::broadcast::domain::VoicePreset;

        let (sessions, _host_rx) = session_with_host_tx("room");
        // Single-slot channel that we will not drain, so the second send
        // exercises the TrySendError::Full branch.
        let (tts_tx, _tts_rx) = mpsc::channel::<TtsRequest>(1);
        {
            let mut session = sessions.get_mut("room").unwrap();
            session.tts_workers.insert(Lang::Ja, tts_tx);
        }
        let handle = LiveSessionHandle::new("room".into(), sessions);

        let args_template = || DispatchTtsArgs {
            committed: "t",
            utterance_id: 1,
            target_lang: &Lang::Ja,
            handle: &handle,
            selected_voice_id: None,
            selected_voice_enrollment_lang: None,
            voice_preset: VoicePreset::Female,
        };
        dispatch_tts_to_worker(args_template()); // fills the single slot
        // Second call must not block, panic, or loop.
        dispatch_tts_to_worker(args_template());
    }

    #[tokio::test]
    async fn dispatch_tts_to_worker_logs_when_channel_is_closed_after_receiver_drop() {
        use crate::features::broadcast::domain::VoicePreset;

        let (sessions, _host_rx) = session_with_host_tx("room");
        let (tts_tx, tts_rx) = mpsc::channel::<TtsRequest>(1);
        drop(tts_rx); // simulate worker already gone
        {
            let mut session = sessions.get_mut("room").unwrap();
            session.tts_workers.insert(Lang::Ja, tts_tx);
        }
        let handle = LiveSessionHandle::new("room".into(), sessions);

        // Must return without panic; this path fires the closed-channel warn.
        dispatch_tts_to_worker(DispatchTtsArgs {
            committed: "x",
            utterance_id: 1,
            target_lang: &Lang::Ja,
            handle: &handle,
            selected_voice_id: None,
            selected_voice_enrollment_lang: None,
            voice_preset: VoicePreset::Female,
        });
    }
}
