use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{info, error, debug};

use crate::core::config::{CHUNK_PIPELINE_CAPACITY, STREAMING_BUDGET_PADDING_SECS};
use crate::core::types::StyleParams;
use crate::core::types::Lang; use crate::features::broadcast::domain::{Sessions, ServerMsg};

use super::pipeline_helpers::to_ws;

// ---------------------------------------------------------------------------
// Context structs
// ---------------------------------------------------------------------------

/// Everything a pipeline function needs -- passed by reference instead of 10+
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
    translate_api_key: String,
    tts_api_key: String,
    default_voice: String,
    http_client: reqwest::Client,
}

/// Input for spawning a chunked pipeline.
pub(crate) struct ChunkPipelineRequest {
    pub sessions: Sessions,
    pub session_id: String,
    pub utterance_id: u64,
    pub source_lang: Lang,
    pub style_params: StyleParams,
    pub translate_api_key: String,
    pub tts_api_key: String,
    pub default_voice: String,
    pub http_client: reqwest::Client,
}

type StreamingMap = std::collections::HashMap<String, super::streaming::StreamingPcm>;

/// Owned bundle for `tokio::spawn` boundaries where references cannot cross.
struct ChunkTask {
    ctx: PipelineContext,
    text: String,
    context: Option<String>,
    chunk_idx: u16,
    target: Lang,
    streaming: Option<super::streaming::StreamingPcm>,
}

/// Result of translating a chunk.
struct TranslationResult {
    text: String,
    translate_ms: u64,
}

// ---------------------------------------------------------------------------
// Public API (the "headline")
// ---------------------------------------------------------------------------

/// Spawn a chunked pipeline that processes sub-utterance chunks via an mpsc channel.
/// One StreamingPcm per (utterance, language) -- multiple TTS chunks feed the same buffer.
pub(crate) fn spawn_chunked_pipeline(
    req: ChunkPipelineRequest,
) -> Option<mpsc::Sender<crate::core::types::ChunkEvent>> {
    let ctx = build_pipeline_context(&req)?;
    let (chunk_tx, chunk_rx) = mpsc::channel::<crate::core::types::ChunkEvent>(CHUNK_PIPELINE_CAPACITY);

    tokio::spawn(async move {
        run_pipeline_loop(chunk_rx, &ctx).await;
    });

    Some(chunk_tx)
}

// ---------------------------------------------------------------------------
// Context construction
// ---------------------------------------------------------------------------

fn build_pipeline_context(req: &ChunkPipelineRequest) -> Option<PipelineContext> {
    let session = req.sessions.get(&req.session_id)?;
    let active = session.active_langs();

    if active.is_empty() {
        return None;
    }

    let ctx = PipelineContext {
        sessions: req.sessions.clone(),
        session_id: req.session_id.clone(),
        utterance_id: req.utterance_id,
        source_lang: req.source_lang.clone(),
        active: active.clone(),
        tier: session.tier,
        voice_clone_id: session.voice_clone_id.clone(),
        tts_model: session.tts_model.clone(),
        broadcast_delay_ms: session.broadcast_delay_ms,
        style_params: req.style_params.clone(),
        translate_api_key: req.translate_api_key.clone(),
        tts_api_key: req.tts_api_key.clone(),
        default_voice: req.default_voice.clone(),
        http_client: req.http_client.clone(),
    };
    drop(session); // release DashMap ref

    Some(ctx)
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

async fn run_pipeline_loop(
    mut chunk_rx: mpsc::Receiver<crate::core::types::ChunkEvent>,
    ctx: &PipelineContext,
) {
    let mut lang_streaming: std::collections::HashMap<
        String, super::streaming::StreamingPcm
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
    chunk: &crate::core::types::ChunkEvent,
    ctx: &PipelineContext,
    lang_streaming: &mut StreamingMap,
) {
    log_chunk_received(ctx.utterance_id, chunk);

    let mut handles = Vec::new();

    for lang in &ctx.active {
        if lang == &ctx.source_lang {
            continue;
        }

        let lang_str = lang.to_string();

        if chunk.chunk_index == 0 {
            if let Some(streaming) = create_streaming_pcm(ctx, &lang_str, chunk.utterance_start).await {
                lang_streaming.insert(lang_str.clone(), streaming);
            }
            notify_tts_start(ctx, &lang_str);
        }

        let task = ChunkTask {
            ctx: ctx.clone(),
            text: chunk.text.clone(),
            context: chunk.context.clone(),
            chunk_idx: chunk.chunk_index,
            target: lang.clone(),
            streaming: lang_streaming.get(&lang_str).cloned(),
        };

        handles.push(spawn_translate_and_synthesize(task));
    }

    for h in handles {
        let _ = h.await;
    }
}

fn log_chunk_received(utterance_id: u64, chunk: &crate::core::types::ChunkEvent) {
    debug!(
        "[CHUNK] #{}.{} text='{}' final={} context={}",
        utterance_id, chunk.chunk_index,
        &chunk.text[..chunk.text.len().min(60)],
        chunk.is_utterance_final,
        chunk.context.as_ref().map(|c| c.len()).unwrap_or(0)
    );
}

// ---------------------------------------------------------------------------
// Streaming slot creation + notifications
// ---------------------------------------------------------------------------

async fn create_streaming_pcm(
    ctx: &PipelineContext,
    lang_str: &str,
    utterance_start: Instant,
) -> Option<super::streaming::StreamingPcm> {
    let erased = ctx.sessions.get(&ctx.session_id).and_then(|s| s.rtmp_manager.clone())?;
    let rtmp_mgr = super::streaming::downcast_rtmp_manager(&erased)?;
    let mgr = rtmp_mgr.lock().await;
    let streaming = mgr.queue_streaming_audio(lang_str, utterance_start);
    debug!("[CHUNK] #{}.0 {} created StreamingPcm slot", ctx.utterance_id, lang_str);
    Some(streaming)
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
    let result = match translate_chunk(task).await {
        Some(r) => r,
        None => return,
    };

    send_chunk_translation(task, &result);

    if task.ctx.tier >= 2 {
        if let Some(ref s) = task.streaming {
            synthesize_chunk(task, &result.text, s).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Translation
// ---------------------------------------------------------------------------

async fn translate_chunk(task: &ChunkTask) -> Option<TranslationResult> {
    let ctx = &task.ctx;
    match crate::shared::translation::translate(
        &task.text, task.context.as_deref(), &ctx.source_lang, &task.target,
        &ctx.translate_api_key, &ctx.http_client,
    ).await {
        Ok((translated, ms)) => {
            debug!(
                "[TRANSLATE] chunk #{}.{} {} = '{}' ({}ms)",
                ctx.utterance_id, task.chunk_idx, task.target, translated, ms
            );
            Some(TranslationResult { text: translated, translate_ms: ms })
        }
        Err(e) => {
            error!("[TRANSLATE] chunk #{}.{} {}: {}", ctx.utterance_id, task.chunk_idx, task.target, e);
            None
        }
    }
}

fn send_chunk_translation(task: &ChunkTask, result: &TranslationResult) {
    let ctx = &task.ctx;
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(to_ws(&ServerMsg::ChunkTranslation {
            lang: task.target.to_string(),
            text: result.text.clone(),
            utterance_id: ctx.utterance_id,
            chunk_index: task.chunk_idx,
            translate_ms: result.translate_ms,
        }));
    }
}

// ---------------------------------------------------------------------------
// TTS synthesis
// ---------------------------------------------------------------------------

async fn synthesize_chunk(
    task: &ChunkTask,
    translated_text: &str,
    streaming: &super::streaming::StreamingPcm,
) {
    let ctx = &task.ctx;
    let voice_id = ctx.voice_clone_id.as_deref().unwrap_or(&ctx.default_voice);
    let lang_str = task.target.to_string();
    let voice_settings = crate::shared::tts::VoiceStyle::from_emotion(&ctx.style_params.emotion)
        .to_voice_settings(ctx.style_params.speed);
    let max_bytes = crate::features::broadcast::domain::pipeline_budget::compute_streaming_max_bytes(
        ctx.broadcast_delay_ms, STREAMING_BUDGET_PADDING_SECS,
    );
    let tts_deadline = crate::features::broadcast::domain::pipeline_budget::compute_tts_deadline(ctx.broadcast_delay_ms);

    let synth_req = crate::shared::tts::SynthesisRequest {
        text: translated_text,
        voice_id,
        lang: &lang_str,
        voice_settings: &voice_settings,
        max_bytes,
        streaming: Some(streaming),
        model_id: &ctx.tts_model,
        api_key: &ctx.tts_api_key,
    };

    match tokio::time::timeout(tts_deadline, crate::shared::tts::do_tts_ws(&synth_req)).await {
        Ok(Ok(bytes)) => {
            debug!("[TTS] chunk #{}.{} {} = {}KB PCM", ctx.utterance_id, task.chunk_idx, task.target, bytes / 1024);
        }
        Ok(Err(e)) => {
            error!("[TTS] chunk #{}.{} {} error: {}", ctx.utterance_id, task.chunk_idx, task.target, e);
        }
        Err(_) => {
            error!("[TTS] chunk #{}.{} {} TIMEOUT", ctx.utterance_id, task.chunk_idx, task.target);
        }
    }
}

// ---------------------------------------------------------------------------
// Pipeline completion
// ---------------------------------------------------------------------------

fn complete_pipeline(
    lang_streaming: &StreamingMap,
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
