//! TTS orchestrator: coordinates deadline, voice selection, and execution.

use std::time::Instant;
use tracing::{info, error, debug};

use crate::constants::{BYTES_PER_SEC, DEFAULT_BROADCAST_DELAY_MS};
use crate::pipeline::budget::{compute_tts_deadline, compute_max_pcm_bytes};
use crate::types::{Lang, Sessions, ServerMsg};
use super::config::{StyleParams, DEFAULT_VOICE};
use super::voice_settings::VoiceStyle;

/// All parameters needed for a single TTS invocation.
pub struct TtsRequest<'a> {
    pub text: &'a str,
    pub utterance_id: u64,
    pub lang: &'a Lang,
    pub voice_clone_id: Option<&'a str>,
    pub style_params: &'a StyleParams,
    pub utterance_start: Instant,
    pub utterance_end: Instant,
    pub tts_model: &'a str,
}

/// ElevenLabs TTS -- WebSocket streaming with REST fallback.
pub async fn do_tts(
    client: &reqwest::Client,
    req: &TtsRequest<'_>,
    sessions: &Sessions,
    session_id: &str,
) {
    let tts_start = Instant::now();
    let lang_str = req.lang.to_string();
    let ctx = prepare_context(req, sessions, session_id);

    log_tts_start(req, &ctx);

    let streaming = allocate_rtmp_slot(sessions, session_id, &lang_str, req.utterance_start).await;
    log_streaming_slot(req.utterance_id, &lang_str, ctx.max_bytes, &streaming);
    notify_host(sessions, session_id, ServerMsg::TtsStart { lang: lang_str.clone(), utterance_id: req.utterance_id });

    let tts_result = execute_tts_with_fallback(client, req, &ctx, &lang_str, &streaming).await;

    finalize(streaming, tts_start, &tts_result, &lang_str, req.utterance_id, &ctx.tts_deadline);
    notify_host(sessions, session_id, ServerMsg::TtsEnd { lang: lang_str, utterance_id: req.utterance_id, tts_ms: tts_start.elapsed().as_millis() as u64 });
}

// ── Internal types ───

struct TtsContext {
    voice_id: String,
    voice_settings: serde_json::Value,
    max_bytes: usize,
    tts_deadline: std::time::Duration,
}

// ── Orchestration helpers ───

fn prepare_context(req: &TtsRequest<'_>, sessions: &Sessions, session_id: &str) -> TtsContext {
    let broadcast_delay_ms = read_broadcast_delay(sessions, session_id);
    TtsContext {
        voice_id: select_voice(req.voice_clone_id),
        voice_settings: build_voice_settings(req.style_params),
        max_bytes: compute_max_pcm_bytes(req.utterance_start, req.utterance_end, 2.0),
        tts_deadline: compute_tts_deadline(broadcast_delay_ms),
    }
}

fn read_broadcast_delay(sessions: &Sessions, session_id: &str) -> u64 {
    sessions.get(session_id)
        .map(|s| s.broadcast_delay_ms)
        .unwrap_or(DEFAULT_BROADCAST_DELAY_MS)
}

async fn execute_tts_with_fallback(
    client: &reqwest::Client,
    req: &TtsRequest<'_>,
    ctx: &TtsContext,
    lang_str: &str,
    streaming: &Option<crate::ffmpeg::StreamingPcm>,
) -> Result<Result<usize, String>, tokio::time::error::Elapsed> {
    tokio::time::timeout(ctx.tts_deadline, async {
        let ws_result = super::ws::do_tts_ws(
            req.text, &ctx.voice_id, lang_str, &ctx.voice_settings,
            ctx.max_bytes, streaming.as_ref(), req.tts_model,
        ).await;
        match ws_result {
            Ok(total_bytes) => Ok(total_bytes),
            Err(ws_err) => {
                tracing::warn!("[TTS] #{} {} WS failed: {}, falling back to REST", req.utterance_id, lang_str, ws_err);
                super::rest::do_tts_rest(
                    client, req.text, &ctx.voice_id, lang_str, &ctx.voice_settings,
                    ctx.max_bytes, streaming.as_ref(), req.tts_model,
                ).await
            }
        }
    }).await
}

fn finalize(
    streaming: Option<crate::ffmpeg::StreamingPcm>,
    tts_start: Instant,
    tts_result: &Result<Result<usize, String>, tokio::time::error::Elapsed>,
    lang_str: &str,
    utterance_id: u64,
    deadline: &std::time::Duration,
) {
    if let Some(ref s) = streaming { s.finish(); }
    let tts_ms = tts_start.elapsed().as_millis() as u64;
    log_tts_result(tts_result, lang_str, utterance_id, tts_ms, deadline);
}

// ── Voice selection ───

fn select_voice(voice_clone_id: Option<&str>) -> String {
    voice_clone_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| DEFAULT_VOICE.clone())
}

fn build_voice_settings(style_params: &StyleParams) -> serde_json::Value {
    let vs = VoiceStyle::from_emotion(&style_params.emotion);
    vs.to_voice_settings(style_params.speed)
}

// ── RTMP slot ───

async fn allocate_rtmp_slot(
    sessions: &Sessions,
    session_id: &str,
    lang: &str,
    utterance_start: Instant,
) -> Option<crate::ffmpeg::StreamingPcm> {
    let rtmp_mgr = sessions.get(session_id)?.rtmp_manager.clone()?;
    let mgr = rtmp_mgr.lock().await;
    Some(mgr.queue_streaming_audio(lang, utterance_start))
}

fn log_streaming_slot(utterance_id: u64, lang: &str, max_bytes: usize, streaming: &Option<crate::ffmpeg::StreamingPcm>) {
    if streaming.is_some() {
        debug!("[TTS] #{} {} queued streaming slot (max={}B={:.1}s)", utterance_id, lang, max_bytes, max_bytes as f64 / BYTES_PER_SEC);
    }
}

// ── Host notification ───

fn notify_host(sessions: &Sessions, session_id: &str, msg: ServerMsg) {
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(crate::pipeline::to_ws(&msg));
    }
}

// ── Logging ───

fn log_tts_start(req: &TtsRequest<'_>, ctx: &TtsContext) {
    info!(
        "[TTS] elevenlabs WS voice={}{} lang={} emotion={} speed={:.2} text='{}' [deadline={}ms]",
        &ctx.voice_id[..8.min(ctx.voice_id.len())],
        if req.voice_clone_id.is_some() { " (cloned)" } else { "" },
        req.lang, req.style_params.emotion, req.style_params.speed, req.text,
        ctx.tts_deadline.as_millis()
    );
}

fn log_tts_result(
    result: &Result<Result<usize, String>, tokio::time::error::Elapsed>,
    lang: &str,
    utterance_id: u64,
    tts_ms: u64,
    deadline: &std::time::Duration,
) {
    match result {
        Ok(Ok(total_bytes)) => info!("[TTS] {}KB in {}ms for {} (streaming PCM)", total_bytes / 1024, tts_ms, lang),
        Ok(Err(e)) => error!("[TTS] Failed for {}: {} ({}ms)", lang, e, tts_ms),
        Err(_) => error!("[TTS] TIMEOUT: utterance {} for {} exceeded {}ms", utterance_id, lang, deadline.as_millis()),
    }
}
