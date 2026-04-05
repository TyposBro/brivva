use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{info, error, debug};

use crate::constants::{CHUNK_PIPELINE_CAPACITY, STREAMING_BUDGET_PADDING_SECS};
use crate::tts::{StyleParams, DEFAULT_VOICE};
use crate::types::{Lang, Sessions, ServerMsg};

use super::to_ws;

// ---------------------------------------------------------------------------
// Context structs (eliminate all `too_many_arguments`)
// ---------------------------------------------------------------------------

/// Everything a pipeline function needs — passed by reference instead of 10+
/// separate parameters.
#[derive(Clone)]
struct PipelineContext {
    sessions: Sessions,
    session_id: String,
    utterance_id: u64,
    source_lang: Lang,
    active: Vec<Lang>,
    tier: u8,
    voice_clone_id: Option<String>,
    tts_model: String,
    broadcast_delay_ms: u64,
    style_params: StyleParams,
}

/// Owned bundle for `tokio::spawn` boundaries where references cannot cross.
struct ChunkTask {
    ctx: PipelineContext,
    text: String,
    context: Option<String>,
    chunk_idx: u16,
    target: Lang,
    streaming: Option<crate::ffmpeg::StreamingPcm>,
}

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
    let ctx = build_pipeline_context(sessions, session_id, utterance_id, source_lang, style_params)?;
    let (chunk_tx, chunk_rx) = mpsc::channel::<crate::types::ChunkEvent>(CHUNK_PIPELINE_CAPACITY);

    tokio::spawn(async move {
        run_pipeline_loop(chunk_rx, &ctx).await;
    });

    Some(chunk_tx)
}

// ---------------------------------------------------------------------------
// Context construction
// ---------------------------------------------------------------------------

fn build_pipeline_context(
    sessions: &Sessions,
    session_id: &str,
    utterance_id: u64,
    source_lang: &Lang,
    style_params: StyleParams,
) -> Option<PipelineContext> {
    let session = sessions.get(session_id)?;
    let active = session.active_langs();

    if active.is_empty() {
        return None;
    }

    let ctx = PipelineContext {
        sessions: sessions.clone(),
        session_id: session_id.to_string(),
        utterance_id,
        source_lang: source_lang.clone(),
        active: active.clone(),
        tier: session.tier,
        voice_clone_id: session.voice_clone_id.clone(),
        tts_model: session.tts_model.clone(),
        broadcast_delay_ms: session.broadcast_delay_ms,
        style_params,
    };
    drop(session); // release DashMap ref

    Some(ctx)
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

async fn run_pipeline_loop(
    mut chunk_rx: mpsc::Receiver<crate::types::ChunkEvent>,
    ctx: &PipelineContext,
) {
    let mut lang_streaming: std::collections::HashMap<
        String, crate::ffmpeg::StreamingPcm
    > = std::collections::HashMap::new();

    let pipeline_start = Instant::now();
    let mut chunk_count: u16 = 0;

    while let Some(chunk) = chunk_rx.recv().await {
        chunk_count += 1;
        process_chunk(&chunk, ctx, &mut lang_streaming).await;
    }

    complete_pipeline(&lang_streaming, ctx, &pipeline_start);
    info!(
        "[CHUNK] #{} pipeline complete: {} chunks, {}ms total",
        ctx.utterance_id, chunk_count, pipeline_start.elapsed().as_millis()
    );
}

// ---------------------------------------------------------------------------
// Per-chunk processing
// ---------------------------------------------------------------------------

async fn process_chunk(
    chunk: &crate::types::ChunkEvent,
    ctx: &PipelineContext,
    lang_streaming: &mut std::collections::HashMap<String, crate::ffmpeg::StreamingPcm>,
) {
    let chunk_idx = chunk.chunk_index;
    let is_final = chunk.is_utterance_final;

    log_chunk_received(ctx.utterance_id, chunk_idx, &chunk.text, is_final, &chunk.context);

    let mut handles = Vec::new();

    for lang in &ctx.active {
        if lang == &ctx.source_lang {
            continue;
        }

        let lang_str = lang.to_string();

        if chunk_idx == 0 {
            create_streaming_slot(ctx, &lang_str, chunk.utterance_start, lang_streaming).await;
            notify_tts_start(ctx, &lang_str);
        }

        let task = ChunkTask {
            ctx: ctx.clone(),
            text: chunk.text.clone(),
            context: chunk.context.clone(),
            chunk_idx,
            target: lang.clone(),
            streaming: lang_streaming.get(&lang_str).cloned(),
        };

        handles.push(spawn_translate_and_synthesize(task));
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
    ctx: &PipelineContext,
    lang_str: &str,
    utterance_start: Instant,
    lang_streaming: &mut std::collections::HashMap<String, crate::ffmpeg::StreamingPcm>,
) {
    let rtmp_mgr = ctx.sessions.get(&ctx.session_id).and_then(|s| s.rtmp_manager.clone());
    if let Some(manager) = rtmp_mgr {
        let mgr = manager.lock().await;
        let streaming = mgr.queue_streaming_audio(lang_str, utterance_start);
        lang_streaming.insert(lang_str.to_string(), streaming);
        debug!("[CHUNK] #{}.0 {} created StreamingPcm slot", ctx.utterance_id, lang_str);
    }
}

fn notify_tts_start(ctx: &PipelineContext, lang_str: &str) {
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(to_ws(&ServerMsg::TtsStart {
            lang: lang_str.to_string(), utterance_id: ctx.utterance_id,
        }));
    }
}

// ---------------------------------------------------------------------------
// Per-language translate + TTS
// ---------------------------------------------------------------------------

fn spawn_translate_and_synthesize(task: ChunkTask) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        translate_and_synthesize_chunk(&task).await;
    })
}

async fn translate_and_synthesize_chunk(task: &ChunkTask) {
    let ctx = &task.ctx;

    let (translated_text, translate_ms) = match translate_chunk(
        &task.text, task.context.as_deref(), ctx.utterance_id,
        task.chunk_idx, &ctx.source_lang, &task.target,
    ).await {
        Some(result) => result,
        None => return,
    };

    send_chunk_translation(ctx, &task.target, &translated_text, task.chunk_idx, translate_ms);

    if ctx.tier >= 2 {
        if let Some(ref s) = task.streaming {
            synthesize_chunk(ctx, &translated_text, task.chunk_idx, &task.target, s).await;
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
    ctx: &PipelineContext,
    target: &Lang,
    translated_text: &str,
    chunk_idx: u16,
    translate_ms: u64,
) {
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(to_ws(&ServerMsg::ChunkTranslation {
            lang: target.to_string(),
            text: translated_text.to_string(),
            utterance_id: ctx.utterance_id,
            chunk_index: chunk_idx,
            translate_ms,
        }));
    }
}

// ---------------------------------------------------------------------------
// TTS synthesis
// ---------------------------------------------------------------------------

async fn synthesize_chunk(
    ctx: &PipelineContext,
    translated_text: &str,
    chunk_idx: u16,
    target: &Lang,
    streaming: &crate::ffmpeg::StreamingPcm,
) {
    let voice_id = ctx.voice_clone_id.as_deref().unwrap_or(&*DEFAULT_VOICE);
    let lang_str = target.to_string();
    let voice_settings = crate::tts::VoiceStyle::from_emotion(&ctx.style_params.emotion)
        .to_voice_settings(ctx.style_params.speed);
    let max_bytes = crate::pipeline::budget::compute_streaming_max_bytes(
        ctx.broadcast_delay_ms, STREAMING_BUDGET_PADDING_SECS,
    );
    let tts_deadline = crate::pipeline::budget::compute_tts_deadline(ctx.broadcast_delay_ms);

    match tokio::time::timeout(tts_deadline, crate::tts::do_tts_ws(
        translated_text, voice_id, &lang_str,
        &voice_settings, max_bytes, Some(streaming), &ctx.tts_model,
    )).await {
        Ok(Ok(bytes)) => {
            debug!("[TTS] chunk #{}.{} {} = {}KB PCM", ctx.utterance_id, chunk_idx, target, bytes / 1024);
        }
        Ok(Err(e)) => {
            error!("[TTS] chunk #{}.{} {} error: {}", ctx.utterance_id, chunk_idx, target, e);
        }
        Err(_) => {
            error!("[TTS] chunk #{}.{} {} TIMEOUT", ctx.utterance_id, chunk_idx, target);
        }
    }
}

// ---------------------------------------------------------------------------
// Pipeline completion
// ---------------------------------------------------------------------------

fn complete_pipeline(
    lang_streaming: &std::collections::HashMap<String, crate::ffmpeg::StreamingPcm>,
    ctx: &PipelineContext,
    pipeline_start: &Instant,
) {
    for (lang_str, streaming) in lang_streaming {
        streaming.finish();
        notify_tts_end(ctx, lang_str, pipeline_start);
    }
}

fn notify_tts_end(ctx: &PipelineContext, lang_str: &str, pipeline_start: &Instant) {
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(to_ws(&ServerMsg::TtsEnd {
            lang: lang_str.to_string(),
            utterance_id: ctx.utterance_id,
            tts_ms: pipeline_start.elapsed().as_millis() as u64,
        }));
    }
}
