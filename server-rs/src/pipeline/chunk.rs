use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{info, error, debug};

use crate::constants::{CHUNK_PIPELINE_CAPACITY, STREAMING_BUDGET_PADDING_SECS};
use crate::tts::{StyleParams, DEFAULT_VOICE};
use crate::types::{Lang, Sessions, ServerMsg};

use super::to_ws;

// ---------------------------------------------------------------------------
// Public API (the "headline")
// ---------------------------------------------------------------------------

/// Spawn a chunked pipeline that processes sub-utterance chunks via an mpsc channel.
/// One StreamingPcm per (utterance, language) — multiple TTS chunks feed the same buffer.
pub(super) fn spawn_chunked_pipeline(
    sessions: &Sessions,
    session_id: &str,
    utterance_id: u64,
    source_lang: &Lang,
    style_params: StyleParams,
) -> Option<mpsc::Sender<crate::types::ChunkEvent>> {
    let ctx = extract_session_context(sessions, session_id)?;
    let (chunk_tx, chunk_rx) = mpsc::channel::<crate::types::ChunkEvent>(CHUNK_PIPELINE_CAPACITY);

    let sessions = sessions.clone();
    let session_id = session_id.to_string();
    let source_lang = source_lang.clone();

    tokio::spawn(async move {
        run_pipeline_loop(
            chunk_rx, &sessions, &session_id, utterance_id,
            &source_lang, &ctx.active, ctx.tier, ctx.voice_clone_id,
            ctx.tts_model, ctx.broadcast_delay_ms, style_params,
        ).await;
    });

    Some(chunk_tx)
}

// ---------------------------------------------------------------------------
// Session context extraction
// ---------------------------------------------------------------------------

struct SessionContext {
    active: Vec<Lang>,
    tier: u8,
    voice_clone_id: Option<String>,
    tts_model: String,
    broadcast_delay_ms: u64,
}

fn extract_session_context(sessions: &Sessions, session_id: &str) -> Option<SessionContext> {
    let session = sessions.get(session_id)?;
    let active = session.active_langs();
    let ctx = SessionContext {
        active: active.clone(),
        tier: session.tier,
        voice_clone_id: session.voice_clone_id.clone(),
        tts_model: session.tts_model.clone(),
        broadcast_delay_ms: session.broadcast_delay_ms,
    };
    drop(session); // release DashMap ref

    if ctx.active.is_empty() { return None; }
    Some(ctx)
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn run_pipeline_loop(
    mut chunk_rx: mpsc::Receiver<crate::types::ChunkEvent>,
    sessions: &Sessions,
    session_id: &str,
    utterance_id: u64,
    source_lang: &Lang,
    active: &[Lang],
    tier: u8,
    voice_clone_id: Option<String>,
    tts_model: String,
    broadcast_delay_ms: u64,
    style_params: StyleParams,
) {
    let mut lang_streaming: std::collections::HashMap<
        String, crate::ffmpeg::StreamingPcm
    > = std::collections::HashMap::new();

    let pipeline_start = Instant::now();
    let mut chunk_count: u16 = 0;

    while let Some(chunk) = chunk_rx.recv().await {
        chunk_count += 1;
        process_chunk(
            &chunk, sessions, session_id, utterance_id, source_lang,
            active, tier, &voice_clone_id, &tts_model, broadcast_delay_ms,
            &style_params, &mut lang_streaming,
        ).await;
    }

    complete_pipeline(&lang_streaming, sessions, session_id, utterance_id, &pipeline_start);
    info!(
        "[CHUNK] #{} pipeline complete: {} chunks, {}ms total",
        utterance_id, chunk_count, pipeline_start.elapsed().as_millis()
    );
}

// ---------------------------------------------------------------------------
// Per-chunk processing
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn process_chunk(
    chunk: &crate::types::ChunkEvent,
    sessions: &Sessions,
    session_id: &str,
    utterance_id: u64,
    source_lang: &Lang,
    active: &[Lang],
    tier: u8,
    voice_clone_id: &Option<String>,
    tts_model: &str,
    broadcast_delay_ms: u64,
    style_params: &StyleParams,
    lang_streaming: &mut std::collections::HashMap<String, crate::ffmpeg::StreamingPcm>,
) {
    let chunk_idx = chunk.chunk_index;
    let is_final = chunk.is_utterance_final;

    log_chunk_received(utterance_id, chunk_idx, &chunk.text, is_final, &chunk.context);

    let mut handles = Vec::new();

    for lang in active {
        if lang == source_lang {
            continue;
        }

        let lang_str = lang.to_string();

        if chunk_idx == 0 {
            create_streaming_slot(
                sessions, session_id, utterance_id,
                &lang_str, chunk.utterance_start, lang_streaming,
            ).await;
            notify_tts_start(sessions, session_id, &lang_str, utterance_id);
        }

        let handle = spawn_translate_and_synthesize(
            chunk, sessions, session_id, utterance_id, source_lang,
            lang, tier, voice_clone_id, tts_model, broadcast_delay_ms,
            style_params, lang_streaming.get(&lang_str).cloned(),
        );
        handles.push(handle);
    }

    for h in handles {
        let _ = h.await;
    }
}

fn log_chunk_received(
    utterance_id: u64,
    chunk_idx: u16,
    text: &str,
    is_final: bool,
    context: &Option<String>,
) {
    debug!(
        "[CHUNK] #{}.{} text='{}' final={} context={}",
        utterance_id, chunk_idx,
        &text[..text.len().min(60)],
        is_final,
        context.as_ref().map(|c| c.len()).unwrap_or(0)
    );
}

// ---------------------------------------------------------------------------
// Streaming slot creation + notifications
// ---------------------------------------------------------------------------

async fn create_streaming_slot(
    sessions: &Sessions,
    session_id: &str,
    utterance_id: u64,
    lang_str: &str,
    utterance_start: Instant,
    lang_streaming: &mut std::collections::HashMap<String, crate::ffmpeg::StreamingPcm>,
) {
    let rtmp_mgr = sessions.get(session_id).and_then(|s| s.rtmp_manager.clone());
    if let Some(manager) = rtmp_mgr {
        let mgr = manager.lock().await;
        let streaming = mgr.queue_streaming_audio(lang_str, utterance_start);
        lang_streaming.insert(lang_str.to_string(), streaming);
        debug!("[CHUNK] #{}.0 {} created StreamingPcm slot", utterance_id, lang_str);
    }
}

fn notify_tts_start(sessions: &Sessions, session_id: &str, lang_str: &str, utterance_id: u64) {
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(to_ws(&ServerMsg::TtsStart {
            lang: lang_str.to_string(), utterance_id,
        }));
    }
}

// ---------------------------------------------------------------------------
// Per-language translate + TTS
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn spawn_translate_and_synthesize(
    chunk: &crate::types::ChunkEvent,
    sessions: &Sessions,
    session_id: &str,
    utterance_id: u64,
    source_lang: &Lang,
    target_lang: &Lang,
    tier: u8,
    voice_clone_id: &Option<String>,
    tts_model: &str,
    broadcast_delay_ms: u64,
    style_params: &StyleParams,
    streaming: Option<crate::ffmpeg::StreamingPcm>,
) -> tokio::task::JoinHandle<()> {
    let text = chunk.text.clone();
    let ctx = chunk.context.clone();
    let chunk_idx = chunk.chunk_index;
    let source = source_lang.clone();
    let target = target_lang.clone();
    let sessions_c = sessions.clone();
    let session_id_c = session_id.to_string();
    let voice_clone = voice_clone_id.clone();
    let sp = style_params.clone();
    let tts_model_c = tts_model.to_string();

    tokio::spawn(async move {
        translate_and_synthesize_chunk(
            &text, ctx.as_deref(), utterance_id, chunk_idx,
            &source, &target, &sessions_c, &session_id_c,
            tier, voice_clone.as_deref(), &sp, &tts_model_c,
            broadcast_delay_ms, streaming.as_ref(),
        ).await;
    })
}

#[allow(clippy::too_many_arguments)]
async fn translate_and_synthesize_chunk(
    text: &str,
    context: Option<&str>,
    utterance_id: u64,
    chunk_idx: u16,
    source: &Lang,
    target: &Lang,
    sessions: &Sessions,
    session_id: &str,
    tier: u8,
    voice_clone_id: Option<&str>,
    style_params: &StyleParams,
    tts_model: &str,
    broadcast_delay_ms: u64,
    streaming: Option<&crate::ffmpeg::StreamingPcm>,
) {
    let (translated_text, translate_ms) = match translate_chunk(
        text, context, utterance_id, chunk_idx, source, target,
    ).await {
        Some(result) => result,
        None => return,
    };

    send_chunk_translation(sessions, session_id, target, &translated_text, utterance_id, chunk_idx, translate_ms);

    if tier >= 2 {
        if let Some(s) = streaming {
            synthesize_chunk(
                &translated_text, utterance_id, chunk_idx, target,
                voice_clone_id, style_params, tts_model, broadcast_delay_ms, s,
            ).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Translation
// ---------------------------------------------------------------------------

async fn translate_chunk(
    text: &str,
    context: Option<&str>,
    utterance_id: u64,
    chunk_idx: u16,
    source: &Lang,
    target: &Lang,
) -> Option<(String, u64)> {
    match crate::translation::translate(text, context, source, target).await {
        Ok((translated, ms)) => {
            debug!(
                "[TRANSLATE] chunk #{}.{} {} = '{}' ({}ms)",
                utterance_id, chunk_idx, target, translated, ms
            );
            Some((translated, ms))
        }
        Err(e) => {
            error!("[TRANSLATE] chunk #{}.{} {}: {}", utterance_id, chunk_idx, target, e);
            None
        }
    }
}

fn send_chunk_translation(
    sessions: &Sessions,
    session_id: &str,
    target: &Lang,
    translated_text: &str,
    utterance_id: u64,
    chunk_idx: u16,
    translate_ms: u64,
) {
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(to_ws(&ServerMsg::ChunkTranslation {
            lang: target.to_string(),
            text: translated_text.to_string(),
            utterance_id,
            chunk_index: chunk_idx,
            translate_ms,
        }));
    }
}

// ---------------------------------------------------------------------------
// TTS synthesis
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn synthesize_chunk(
    translated_text: &str,
    utterance_id: u64,
    chunk_idx: u16,
    target: &Lang,
    voice_clone_id: Option<&str>,
    style_params: &StyleParams,
    tts_model: &str,
    broadcast_delay_ms: u64,
    streaming: &crate::ffmpeg::StreamingPcm,
) {
    let voice_id = voice_clone_id.unwrap_or(&*DEFAULT_VOICE);
    let lang_str = target.to_string();
    let voice_settings = crate::tts::VoiceStyle::from_emotion(&style_params.emotion)
        .to_voice_settings(style_params.speed);
    let max_bytes = crate::pipeline::budget::compute_streaming_max_bytes(
        broadcast_delay_ms, STREAMING_BUDGET_PADDING_SECS,
    );
    let tts_deadline = crate::pipeline::budget::compute_tts_deadline(broadcast_delay_ms);

    match tokio::time::timeout(tts_deadline, crate::tts::do_tts_ws(
        translated_text, voice_id, &lang_str,
        &voice_settings, max_bytes, Some(streaming), tts_model,
    )).await {
        Ok(Ok(bytes)) => {
            debug!("[TTS] chunk #{}.{} {} = {}KB PCM", utterance_id, chunk_idx, target, bytes / 1024);
        }
        Ok(Err(e)) => {
            error!("[TTS] chunk #{}.{} {} error: {}", utterance_id, chunk_idx, target, e);
        }
        Err(_) => {
            error!("[TTS] chunk #{}.{} {} TIMEOUT", utterance_id, chunk_idx, target);
        }
    }
}

// ---------------------------------------------------------------------------
// Pipeline completion
// ---------------------------------------------------------------------------

fn complete_pipeline(
    lang_streaming: &std::collections::HashMap<String, crate::ffmpeg::StreamingPcm>,
    sessions: &Sessions,
    session_id: &str,
    utterance_id: u64,
    pipeline_start: &Instant,
) {
    for (lang_str, streaming) in lang_streaming {
        streaming.finish();
        notify_tts_end(sessions, session_id, lang_str, utterance_id, pipeline_start);
    }
}

fn notify_tts_end(
    sessions: &Sessions,
    session_id: &str,
    lang_str: &str,
    utterance_id: u64,
    pipeline_start: &Instant,
) {
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(to_ws(&ServerMsg::TtsEnd {
            lang: lang_str.to_string(),
            utterance_id,
            tts_ms: pipeline_start.elapsed().as_millis() as u64,
        }));
    }
}
