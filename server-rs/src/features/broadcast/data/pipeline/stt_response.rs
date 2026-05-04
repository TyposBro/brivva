use crate::features::broadcast::domain::{
    AvailableWindowMethod, Lang, LiveSessionHandle, ProviderHealthEvent, ServerMsg,
    SourceTimingMethod, SourceUtteranceTiming, TtsRequest,
};
use futures_util::StreamExt;
use std::{sync::Arc, time::Instant};
use tokio::{
    sync::{Mutex, mpsc::error::TrySendError},
    time::{Duration, sleep},
};
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
        let mut timing = UtteranceTimingAccumulator::default();
        let pending_tts = Arc::new(Mutex::new(None));
        let lookahead_tts = tts_lookahead_budget_enabled();
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
                emit_provider_health(&handle, ProviderHealthEvent::new(
                    "soniox",
                    "reconnecting",
                    true,
                    false,
                    "upstream_error",
                    format!(
                        "Soniox returned an error for {tag}; translation is reconnecting and this period is not billable."
                    ),
                ).target_lang(mode.target_lang_string()).error_code(response.error_code.clone()));
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

            let (interim_tail, endpoint_hit) =
                accumulate_tokens(&mode, &response, &mut final_text, &mut timing);
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
                    source_start_ms: timing.source_start_ms,
                    source_timing: timing.finish(),
                    pending_tts: pending_tts.clone(),
                    lookahead_tts,
                })
                .await;
            }
        }

        flush_pending_tts(pending_tts, AvailableWindowMethod::HoldTimeoutFallback).await;
        (utterance_counter, true)
    })
}

fn tts_lookahead_budget_enabled() -> bool {
    std::env::var("BRIVVA_TTS_LOOKAHEAD_BUDGET")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
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

fn emit_provider_health(handle: &LiveSessionHandle, event: ProviderHealthEvent) {
    if let Some(live_session) = handle.sessions.get(&handle.id) {
        live_session.emit_provider_health(event);
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
    // Hard ceiling. 100 chars ≈ 5-10 seconds of synth at Flash v2.5 rates,
    // which fits comfortably under the 30s TTS deadline.
    const FLUSH_MAX_CHARS: usize = 100;
    if char_count < FLUSH_MIN_CHARS {
        return None;
    }
    const SENTENCE_ENDERS: &[char] = &['.', '!', '?', '。', '！', '？'];
    if final_text
        .chars()
        .filter(|c| SENTENCE_ENDERS.contains(c))
        .count()
        >= 3
    {
        return Some("sentence_count");
    }
    if char_count >= FLUSH_MAX_CHARS {
        return Some("length");
    }
    const SOFT_SENTENCE_ENDERS: &[char] =
        &['.', '!', '?', '。', '！', '？', ',', '，', ';', '；', '、'];
    if final_text
        .chars()
        .rev()
        .take(3)
        .any(|c| SOFT_SENTENCE_ENDERS.contains(&c))
    {
        return Some("punctuation");
    }
    None
}

#[derive(Default)]
struct UtteranceTimingAccumulator {
    source_start_ms: Option<u64>,
    source_end_ms: Option<u64>,
    response_started_at: Option<Instant>,
}

impl UtteranceTimingAccumulator {
    fn observe_useful_token(&mut self) {
        self.response_started_at.get_or_insert_with(Instant::now);
    }

    fn observe_source_timing(&mut self, start_ms: u64, end_ms: u64) {
        self.source_start_ms = Some(
            self.source_start_ms
                .map_or(start_ms, |current| current.min(start_ms)),
        );
        self.source_end_ms = Some(
            self.source_end_ms
                .map_or(end_ms, |current| current.max(end_ms)),
        );
    }

    fn finish(&mut self) -> Option<SourceUtteranceTiming> {
        let timing = if let (Some(start), Some(end)) = (self.source_start_ms, self.source_end_ms) {
            Some(SourceUtteranceTiming::same_as_speech(
                end.saturating_sub(start).clamp(500, 15_000),
                SourceTimingMethod::SonioxTokenTimestamps,
            ))
        } else {
            self.response_started_at.map(|started| {
                SourceUtteranceTiming::same_as_speech(
                    (started.elapsed().as_millis() as u64).clamp(500, 15_000),
                    SourceTimingMethod::ResponseWallClock,
                )
            })
        };
        *self = Self::default();
        timing
    }
}

fn accumulate_tokens(
    mode: &SonioxMode,
    response: &SonioxResponse,
    final_text: &mut String,
    timing: &mut UtteranceTimingAccumulator,
) -> (String, bool) {
    let mut interim_tail = String::new();
    let mut endpoint_hit = false;

    for token in &response.tokens {
        let accepted = mode.accepts(token);
        if accepted || token.timing_ms().is_some() {
            timing.observe_useful_token();
        }
        if matches!(
            token.translation_status.as_deref(),
            Some("none" | "original") | None
        ) && token.is_final
            && let Some((start_ms, end_ms)) = token.timing_ms()
        {
            timing.observe_source_timing(start_ms, end_ms);
        }
        // AUDIT: normal-flow filter — mode selects source vs translation
        // tokens per the SonioxMode. Not a silent-bug branch.
        if !accepted {
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

type PendingTtsSlot = Arc<Mutex<Option<PendingTtsDispatch>>>;

struct PendingTtsDispatch {
    committed: String,
    utterance_id: u64,
    target_lang: Lang,
    handle: LiveSessionHandle,
    selected_voice_id: Option<String>,
    selected_voice_enrollment_lang: Option<Lang>,
    voice_preset: crate::features::broadcast::domain::VoicePreset,
    source_timing: Option<SourceUtteranceTiming>,
    source_start_ms: Option<u64>,
}

struct FinalizeArgs<'a> {
    mode: &'a SonioxMode,
    utterance_counter: u64,
    handle: &'a LiveSessionHandle,
    final_text: &'a mut String,
    source_start_ms: Option<u64>,
    source_timing: Option<SourceUtteranceTiming>,
    pending_tts: PendingTtsSlot,
    lookahead_tts: bool,
}

async fn finalize_utterance_if_needed(args: FinalizeArgs<'_>) -> u64 {
    let FinalizeArgs {
        mode,
        utterance_counter,
        handle,
        final_text,
        source_start_ms,
        source_timing,
        pending_tts,
        lookahead_tts,
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
                source_start_ms,
                source_timing,
                pending_tts,
                lookahead_tts,
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
    source_start_ms: Option<u64>,
    source_timing: Option<SourceUtteranceTiming>,
    pending_tts: PendingTtsSlot,
    lookahead_tts: bool,
}

async fn emit_translation(args: EmitTranslationArgs<'_>) {
    let EmitTranslationArgs {
        committed,
        utterance_id,
        target_lang,
        handle,
        source_start_ms,
        source_timing,
        pending_tts,
        lookahead_tts,
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
        if let Some(manager) = live_session.rtmp_manager.clone() {
            let lang = target_lang.to_string();
            let subtitle = committed.to_string();
            tokio::spawn(async move {
                manager.lock().await.push_subtitle(&lang, &subtitle);
            });
        }
    }

    let dispatch = PendingTtsDispatch {
        committed: committed.to_string(),
        utterance_id,
        target_lang: target_lang.clone(),
        handle: handle.clone(),
        selected_voice_id,
        selected_voice_enrollment_lang,
        voice_preset,
        source_timing,
        source_start_ms,
    };
    if lookahead_tts {
        schedule_tts_with_lookahead(pending_tts, dispatch).await;
    } else {
        dispatch_pending_tts(dispatch);
    }
}

struct DispatchTtsArgs<'a> {
    committed: &'a str,
    utterance_id: u64,
    target_lang: &'a Lang,
    handle: &'a LiveSessionHandle,
    selected_voice_id: Option<String>,
    selected_voice_enrollment_lang: Option<Lang>,
    voice_preset: crate::features::broadcast::domain::VoicePreset,
    source_timing: Option<SourceUtteranceTiming>,
}

fn dispatch_pending_tts(dispatch: PendingTtsDispatch) {
    dispatch_tts_to_worker(DispatchTtsArgs {
        committed: &dispatch.committed,
        utterance_id: dispatch.utterance_id,
        target_lang: &dispatch.target_lang,
        handle: &dispatch.handle,
        selected_voice_id: dispatch.selected_voice_id,
        selected_voice_enrollment_lang: dispatch.selected_voice_enrollment_lang,
        voice_preset: dispatch.voice_preset,
        source_timing: dispatch.source_timing,
    });
}

async fn flush_pending_tts(pending_tts: PendingTtsSlot, method: AvailableWindowMethod) {
    let pending = pending_tts.lock().await.take();
    if let Some(mut dispatch) = pending {
        if matches!(method, AvailableWindowMethod::HoldTimeoutFallback) {
            dispatch.source_timing = dispatch
                .source_timing
                .map(SourceUtteranceTiming::hold_timeout_fallback);
        }
        dispatch_pending_tts(dispatch);
    }
}

async fn schedule_tts_with_lookahead(pending_tts: PendingTtsSlot, current: PendingTtsDispatch) {
    let previous = {
        let mut guard = pending_tts.lock().await;
        guard.take()
    };
    if let Some(mut previous) = previous {
        if let (Some(prev_start), Some(current_start)) =
            (previous.source_start_ms, current.source_start_ms)
            && current_start > prev_start
        {
            previous.source_timing = previous
                .source_timing
                .map(|timing| timing.with_available_window(current_start - prev_start));
        }
        dispatch_pending_tts(previous);
    }

    let utterance_id = current.utterance_id;
    {
        let mut guard = pending_tts.lock().await;
        *guard = Some(current);
    }

    let pending_for_timeout = pending_tts.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(500)).await;
        let should_flush = pending_for_timeout
            .lock()
            .await
            .as_ref()
            .map(|pending| pending.utterance_id == utterance_id)
            .unwrap_or(false);
        if should_flush {
            flush_pending_tts(
                pending_for_timeout,
                AvailableWindowMethod::HoldTimeoutFallback,
            )
            .await;
        }
    });
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
        source_timing: args.source_timing,
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
            start_ms: None,
            end_ms: None,
            is_final,
            translation_status: translation_status.map(str::to_string),
        }
    }

    fn timed_token(
        text: &str,
        is_final: bool,
        translation_status: Option<&str>,
        start_ms: u64,
        end_ms: u64,
    ) -> SonioxToken {
        SonioxToken {
            text: text.to_string(),
            start_ms: Some(start_ms),
            end_ms: Some(end_ms),
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
        let mut timing = UtteranceTimingAccumulator::default();
        let (interim, endpoint) = accumulate_tokens(&mode, &response, &mut final_text, &mut timing);

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
        let mut timing = UtteranceTimingAccumulator::default();
        let (_, endpoint) = accumulate_tokens(&mode, &response, &mut final_text, &mut timing);

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
        let mut timing = UtteranceTimingAccumulator::default();
        let (interim, _) = accumulate_tokens(&mode, &response, &mut final_text, &mut timing);

        assert_eq!(final_text, "konnichiwa");
        assert_eq!(interim, " yo");
    }

    #[test]
    fn accumulate_translate_mode_captures_source_token_timing() {
        let mode = SonioxMode::Translate {
            source_lang: Lang::En,
            target_lang: Lang::Ja,
        };
        let response = SonioxResponse {
            tokens: vec![
                timed_token("hello ", true, Some("original"), 1_000, 1_400),
                timed_token("world", true, Some("original"), 1_450, 2_200),
                token("こんにちは", true, Some("translation")),
            ],
            error_code: None,
            error_message: None,
        };
        let mut final_text = String::new();
        let mut timing = UtteranceTimingAccumulator::default();

        accumulate_tokens(&mode, &response, &mut final_text, &mut timing);
        let source_timing = timing.finish().expect("source timing");

        assert_eq!(final_text, "こんにちは");
        assert_eq!(source_timing.source_speech_duration_ms, 1_200);
        assert_eq!(source_timing.tts_budget_ms(), 1_200);
        assert_eq!(
            source_timing.timing_method,
            SourceTimingMethod::SonioxTokenTimestamps
        );
    }

    #[test]
    fn accumulate_translate_mode_falls_back_to_response_wall_clock_without_timestamps() {
        let mode = SonioxMode::Translate {
            source_lang: Lang::En,
            target_lang: Lang::Ja,
        };
        let response = SonioxResponse {
            tokens: vec![token("こんにちは", true, Some("translation"))],
            error_code: None,
            error_message: None,
        };
        let mut final_text = String::new();
        let mut timing = UtteranceTimingAccumulator::default();

        accumulate_tokens(&mode, &response, &mut final_text, &mut timing);
        let source_timing = timing.finish().expect("fallback timing");

        assert_eq!(final_text, "こんにちは");
        assert_eq!(source_timing.source_speech_duration_ms, 500);
        assert_eq!(source_timing.tts_budget_ms(), 500);
        assert_eq!(
            source_timing.timing_method,
            SourceTimingMethod::ResponseWallClock
        );
    }

    #[test]
    fn timing_accumulator_resets_after_finish() {
        let mut timing = UtteranceTimingAccumulator::default();
        timing.observe_source_timing(1_000, 2_000);
        assert!(timing.finish().is_some());
        assert!(timing.finish().is_none());
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
        let mut timing = UtteranceTimingAccumulator::default();
        accumulate_tokens(&mode, &response, &mut final_text, &mut timing);

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
            source_start_ms: None,
            source_timing: None,
            pending_tts: Arc::new(Mutex::new(None)),
            lookahead_tts: false,
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
            source_start_ms: None,
            source_timing: None,
            pending_tts: Arc::new(Mutex::new(None)),
            lookahead_tts: false,
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
    async fn emit_translation_sends_translation_message() {
        let (sessions, mut rx) = session_with_host_tx("room");
        let handle = LiveSessionHandle::new("room".into(), sessions);

        super::emit_translation(super::EmitTranslationArgs {
            committed: "konnichiwa",
            utterance_id: 2,
            target_lang: &Lang::Ja,
            handle: &handle,
            source_start_ms: None,
            source_timing: None,
            pending_tts: Arc::new(Mutex::new(None)),
            lookahead_tts: false,
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
        // 100 chars with no punctuation — force-flush on length alone so
        // long pauseless monologues don't accumulate into one giant chunk
        // that then blows the TTS deadline.
        assert_eq!(flush_reason(false, &"a".repeat(100)), Some("length"));
        assert_eq!(flush_reason(false, &"b".repeat(500)), Some("length"));
    }

    #[test]
    fn flush_reason_returns_sentence_count_after_three_sentences() {
        let text = "One good deal. Two left now. Three sold fast.";
        assert!(text.chars().count() >= 30);
        assert_eq!(flush_reason(false, text), Some("sentence_count"));
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
            source_timing: None,
        });

        let req = tts_rx.recv().await.expect("worker should receive");
        assert_eq!(req.utterance_id, 42);
        assert_eq!(req.text, "konnichiwa");
        assert_eq!(req.target_lang, Lang::Ja);
    }

    fn pending_dispatch(
        utterance_id: u64,
        source_start_ms: Option<u64>,
        source_timing: Option<SourceUtteranceTiming>,
        handle: LiveSessionHandle,
    ) -> PendingTtsDispatch {
        use crate::features::broadcast::domain::VoicePreset;
        PendingTtsDispatch {
            committed: format!("utt-{utterance_id}"),
            utterance_id,
            target_lang: Lang::Ja,
            handle,
            selected_voice_id: None,
            selected_voice_enrollment_lang: None,
            voice_preset: VoicePreset::Female,
            source_timing,
            source_start_ms,
        }
    }

    #[tokio::test]
    async fn lookahead_scheduler_uses_next_utterance_start_as_available_window() {
        let (sessions, _host_rx) = session_with_host_tx("room");
        let (tts_tx, mut tts_rx) = mpsc::channel::<TtsRequest>(4);
        {
            let mut session = sessions.get_mut("room").unwrap();
            session.tts_workers.insert(Lang::Ja, tts_tx);
        }
        let handle = LiveSessionHandle::new("room".into(), sessions);
        let pending = Arc::new(Mutex::new(None));

        schedule_tts_with_lookahead(
            pending.clone(),
            pending_dispatch(
                1,
                Some(1_000),
                Some(SourceUtteranceTiming::same_as_speech(
                    500,
                    SourceTimingMethod::SonioxTokenTimestamps,
                )),
                handle.clone(),
            ),
        )
        .await;
        assert!(tts_rx.try_recv().is_err(), "first utterance should be held");

        schedule_tts_with_lookahead(
            pending,
            pending_dispatch(
                2,
                Some(3_000),
                Some(SourceUtteranceTiming::same_as_speech(
                    500,
                    SourceTimingMethod::SonioxTokenTimestamps,
                )),
                handle,
            ),
        )
        .await;

        let req = tts_rx.recv().await.expect("previous utterance dispatched");
        let timing = req.source_timing.expect("timing");
        assert_eq!(req.utterance_id, 1);
        assert_eq!(timing.source_speech_duration_ms, 500);
        assert_eq!(timing.available_window_ms, Some(2_000));
        assert_eq!(
            timing.available_window_method,
            AvailableWindowMethod::NextUtteranceStart
        );
        assert_eq!(timing.tts_budget_ms(), 2_000);
    }

    #[tokio::test]
    async fn lookahead_timeout_dispatches_pending_once() {
        let (sessions, _host_rx) = session_with_host_tx("room");
        let (tts_tx, mut tts_rx) = mpsc::channel::<TtsRequest>(4);
        {
            let mut session = sessions.get_mut("room").unwrap();
            session.tts_workers.insert(Lang::Ja, tts_tx);
        }
        let handle = LiveSessionHandle::new("room".into(), sessions);
        let pending = Arc::new(Mutex::new(None));

        schedule_tts_with_lookahead(
            pending,
            pending_dispatch(
                1,
                Some(1_000),
                Some(SourceUtteranceTiming::same_as_speech(
                    500,
                    SourceTimingMethod::SonioxTokenTimestamps,
                )),
                handle,
            ),
        )
        .await;

        let req = tokio::time::timeout(Duration::from_secs(1), tts_rx.recv())
            .await
            .expect("timeout task should dispatch")
            .expect("request");
        let timing = req.source_timing.expect("timing");
        assert_eq!(req.utterance_id, 1);
        assert_eq!(timing.available_window_ms, Some(1_000));
        assert_eq!(
            timing.available_window_method,
            AvailableWindowMethod::HoldTimeoutFallback
        );
        tokio::time::sleep(Duration::from_millis(550)).await;
        assert!(tts_rx.try_recv().is_err(), "pending should dispatch once");
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
            source_timing: None,
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
            source_timing: None,
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
            source_timing: None,
        });
    }
}
