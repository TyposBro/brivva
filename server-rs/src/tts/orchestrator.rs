//! TTS orchestrator: coordinates deadline, voice selection, and execution.

use std::time::Instant;
use tracing::{info, error, debug};

use crate::constants::{BYTES_PER_SEC, DEFAULT_BROADCAST_DELAY_MS};
use crate::pipeline::budget::{compute_tts_deadline, compute_max_pcm_bytes};
use crate::types::{Lang, Sessions, ServerMsg};
use super::config::{StyleParams, DEFAULT_VOICE};
use super::voice_settings::VoiceStyle;

fn select_voice(voice_clone_id: Option<&str>) -> String {
    voice_clone_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| DEFAULT_VOICE.clone())
}

fn build_voice_settings(style_params: &StyleParams) -> serde_json::Value {
    let vs = VoiceStyle::from_emotion(&style_params.emotion);
    vs.to_voice_settings(style_params.speed)
}

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

fn notify_host(sessions: &Sessions, session_id: &str, msg: ServerMsg) {
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(crate::pipeline::to_ws(&msg));
    }
}

/// ElevenLabs TTS — WebSocket streaming with REST fallback.
pub async fn do_tts(
    client: &reqwest::Client,
    text: &str,
    utterance_id: u64,
    lang: &Lang,
    sessions: &Sessions,
    session_id: &str,
    voice_clone_id: Option<&str>,
    style_params: &StyleParams,
    utterance_start: Instant,
    utterance_end: Instant,
    tts_model: &str,
) {
    let tts_start = Instant::now();
    let lang_str = lang.to_string();

    let broadcast_delay_ms = sessions.get(session_id)
        .map(|s| s.broadcast_delay_ms)
        .unwrap_or(DEFAULT_BROADCAST_DELAY_MS);
    let tts_deadline = compute_tts_deadline(broadcast_delay_ms);
    let voice_id = select_voice(voice_clone_id);
    let voice_settings = build_voice_settings(style_params);
    let max_bytes = compute_max_pcm_bytes(utterance_start, utterance_end, 2.0);

    info!(
        "[TTS] elevenlabs WS voice={}{} lang={} emotion={} speed={:.2} text='{}' [deadline={}ms]",
        &voice_id[..8.min(voice_id.len())],
        if voice_clone_id.is_some() { " (cloned)" } else { "" },
        lang, style_params.emotion, style_params.speed, text,
        tts_deadline.as_millis()
    );

    let streaming = allocate_rtmp_slot(sessions, session_id, &lang_str, utterance_start).await;
    if streaming.is_some() {
        debug!("[TTS] #{} {} queued streaming slot (max={}B={:.1}s)", utterance_id, lang_str, max_bytes, max_bytes as f64 / BYTES_PER_SEC);
    }

    notify_host(sessions, session_id, ServerMsg::TtsStart { lang: lang_str.clone(), utterance_id });

    let tts_result = tokio::time::timeout(tts_deadline, async {
        match super::ws::do_tts_ws(text, &voice_id, &lang_str, &voice_settings, max_bytes, streaming.as_ref(), tts_model).await {
            Ok(total_bytes) => Ok(total_bytes),
            Err(ws_err) => {
                tracing::warn!("[TTS] #{} {} WS failed: {}, falling back to REST", utterance_id, lang_str, ws_err);
                super::rest::do_tts_rest(client, text, &voice_id, &lang_str, &voice_settings, max_bytes, streaming.as_ref(), tts_model).await
            }
        }
    }).await;

    if let Some(ref s) = streaming { s.finish(); }

    let tts_ms = tts_start.elapsed().as_millis() as u64;
    log_tts_result(&tts_result, &lang_str, utterance_id, tts_ms, &tts_deadline);
    notify_host(sessions, session_id, ServerMsg::TtsEnd { lang: lang_str, utterance_id, tts_ms });
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
