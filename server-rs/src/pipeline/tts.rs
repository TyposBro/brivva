use std::time::{Duration, Instant};
use tracing::{info, error, debug};

use crate::constants::BYTES_PER_SEC;
use crate::stt::config::PASSTHROUGH_PADDING_SECS;
use crate::tts::{StyleParams, TtsRequest};
use crate::types::{Lang, Sessions, ServerMsg};

use super::to_ws;

// ── Context ───

/// Shared state for the legacy full-utterance pipeline. Cloned at
/// `tokio::spawn` boundaries instead of cloning 8+ individual fields.
#[derive(Clone)]
struct PipelineCtx {
    sessions: Sessions,
    session_id: String,
    utterance_id: u64,
    source_lang: Lang,
    style_params: StyleParams,
    tier: u8,
    voice_clone_id: Option<String>,
    tts_model: String,
    utterance_start: Instant,
    utterance_end: Instant,
}

// ── Translation Pipeline (legacy full-utterance path) ───

pub(super) async fn run_pipeline(
    transcript: &str,
    utterance_id: u64,
    source_lang: &Lang,
    target_langs: &[Lang],
    sessions: &Sessions,
    session_id: &str,
    style_params: &StyleParams,
    tier: u8,
    utterance_start: Instant,
    utterance_end: Instant,
    host_audio: Vec<u8>,
) {
    let pipeline_start = Instant::now();
    let ctx = build_ctx(sessions, session_id, utterance_id, source_lang, style_params, tier, utterance_start, utterance_end);

    log_pipeline_start(&ctx, transcript, target_langs);

    let handles = spawn_all_lang_tasks(&ctx, transcript, target_langs, &host_audio);
    await_all(handles).await;

    log_pipeline_complete(&ctx, pipeline_start);
}

// ── Context Construction ───

fn build_ctx(
    sessions: &Sessions,
    session_id: &str,
    utterance_id: u64,
    source_lang: &Lang,
    style_params: &StyleParams,
    tier: u8,
    utterance_start: Instant,
    utterance_end: Instant,
) -> PipelineCtx {
    let (voice_clone_id, tts_model) = read_voice_config(sessions, session_id);

    PipelineCtx {
        sessions: sessions.clone(),
        session_id: session_id.to_string(),
        utterance_id,
        source_lang: source_lang.clone(),
        style_params: style_params.clone(),
        tier,
        voice_clone_id,
        tts_model,
        utterance_start,
        utterance_end,
    }
}

fn read_voice_config(sessions: &Sessions, session_id: &str) -> (Option<String>, String) {
    match sessions.get(session_id) {
        Some(s) => (s.voice_clone_id.clone(), s.tts_model.clone()),
        None => (None, crate::constants::DEFAULT_TTS_MODEL.to_string()),
    }
}

// ── Spawning ───

fn spawn_all_lang_tasks(
    ctx: &PipelineCtx,
    transcript: &str,
    target_langs: &[Lang],
    host_audio: &[u8],
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles = Vec::new();

    for lang in target_langs {
        if lang == &ctx.source_lang {
            handles.extend(spawn_source_passthrough(ctx, lang, host_audio));
        } else {
            handles.push(spawn_translate_and_tts(ctx, transcript, lang));
        }
    }

    handles
}

fn spawn_source_passthrough(
    ctx: &PipelineCtx,
    lang: &Lang,
    host_audio: &[u8],
) -> Option<tokio::task::JoinHandle<()>> {
    let mgr = ctx.sessions.get(&ctx.session_id).and_then(|s| s.rtmp_manager.clone())?;
    if host_audio.is_empty() { return None; }

    let mut pcm = host_audio.to_vec();
    let pcm_len = pcm.len();
    let lang_str = lang.to_string();
    let start = ctx.utterance_start;
    let end = ctx.utterance_end;

    Some(tokio::spawn(async move {
        truncate_if_too_long(&mut pcm, start, end);
        let locked = mgr.lock().await;
        locked.queue_audio(&lang_str, pcm, start);
        debug!("[PASSTHROUGH] Queued host audio for {} ({}KB)", lang_str, pcm_len / 1024);
    }))
}

fn spawn_translate_and_tts(
    ctx: &PipelineCtx,
    transcript: &str,
    target_lang: &Lang,
) -> tokio::task::JoinHandle<()> {
    let ctx = ctx.clone();
    let transcript = transcript.to_string();
    let target = target_lang.clone();
    let client = crate::HTTP_CLIENT.clone();

    tokio::spawn(async move {
        run_translate_then_tts(&ctx, &client, &transcript, &target).await;
    })
}

// ── Translate + TTS Task ───

async fn run_translate_then_tts(
    ctx: &PipelineCtx,
    client: &reqwest::Client,
    transcript: &str,
    target: &Lang,
) {
    let step_start = Instant::now();
    log_translate_start(ctx.utterance_id, &ctx.source_lang, target, transcript);

    let (translated, ms) = match translate(transcript, &ctx.source_lang, target, ctx.utterance_id).await {
        Some(result) => result,
        None => return,
    };

    log_translate_result(&ctx.source_lang, target, &translated, ms);
    send_translation_to_host(ctx, target, &translated, ms);
    run_tts_if_eligible(ctx, client, &translated, target, step_start, ms).await;
}

// ── TTS ───

async fn run_tts_if_eligible(
    ctx: &PipelineCtx,
    client: &reqwest::Client,
    translated: &str,
    target: &Lang,
    step_start: Instant,
    translate_ms: u64,
) {
    if ctx.tier < 2 {
        log_tts_skipped(ctx, target);
        return;
    }

    log_tts_start(ctx, target, translate_ms, step_start);
    let req = build_tts_request(ctx, translated, target);
    crate::tts::do_tts(client, &req, &ctx.sessions, &ctx.session_id).await;
    log_tts_complete(ctx, target, step_start);
}

fn build_tts_request<'a>(ctx: &'a PipelineCtx, text: &'a str, lang: &'a Lang) -> TtsRequest<'a> {
    TtsRequest {
        text,
        utterance_id: ctx.utterance_id,
        lang,
        voice_clone_id: ctx.voice_clone_id.as_deref(),
        style_params: &ctx.style_params,
        utterance_start: ctx.utterance_start,
        utterance_end: ctx.utterance_end,
        tts_model: &ctx.tts_model,
    }
}

// ── Helpers ───

fn truncate_if_too_long(pcm: &mut Vec<u8>, utterance_start: Instant, utterance_end: Instant) {
    let utterance_dur = utterance_end.duration_since(utterance_start);
    let max_dur = utterance_dur + Duration::from_secs_f64(PASSTHROUGH_PADDING_SECS);
    let max_bytes = (max_dur.as_secs_f64() * BYTES_PER_SEC) as usize;
    if pcm.len() > max_bytes {
        crate::ffmpeg::truncate_with_fadeout(pcm, max_bytes);
    }
}

async fn translate(transcript: &str, source: &Lang, target: &Lang, utterance_id: u64) -> Option<(String, u64)> {
    match crate::translation::translate(transcript, None, source, target).await {
        Ok((t, ms)) => Some((t, ms)),
        Err(e) => {
            error!("[TRANSLATE] #{} {}: {}", utterance_id, target, e);
            None
        }
    }
}

fn send_translation_to_host(ctx: &PipelineCtx, target: &Lang, translated_text: &str, translate_ms: u64) {
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(to_ws(&ServerMsg::Translation {
            lang: target.to_string(),
            text: translated_text.to_string(),
            utterance_id: ctx.utterance_id,
            translate_ms,
        }));
    }
}

async fn await_all(handles: Vec<tokio::task::JoinHandle<()>>) {
    for handle in handles {
        let _ = handle.await;
    }
}

// ── Logging ───

fn log_pipeline_start(ctx: &PipelineCtx, transcript: &str, target_langs: &[Lang]) {
    info!(
        "[PIPELINE] #{} starting: '{}' -> {:?} (voice_clone={}) delay_since_utterance_start={}ms",
        ctx.utterance_id, &transcript[..transcript.len().min(60)],
        target_langs.iter().map(|l| l.to_string()).collect::<Vec<_>>(),
        ctx.voice_clone_id.as_deref().unwrap_or("none"),
        ctx.utterance_start.elapsed().as_millis()
    );
}

fn log_pipeline_complete(ctx: &PipelineCtx, pipeline_start: Instant) {
    info!(
        "[PIPELINE] #{} all langs done (total {}ms since pipeline start, {}ms since utterance start)",
        ctx.utterance_id, pipeline_start.elapsed().as_millis(), ctx.utterance_start.elapsed().as_millis()
    );
}

fn log_translate_start(utterance_id: u64, source: &Lang, target: &Lang, transcript: &str) {
    debug!(
        "[TRANSLATE] #{} {} -> {}: '{}' ({} chars)",
        utterance_id, source, target, &transcript[..transcript.len().min(80)], transcript.len()
    );
}

fn log_translate_result(source: &Lang, target: &Lang, translated: &str, ms: u64) {
    info!("[TRANSLATE] {} -> {} = '{}' ({}ms)", source, target, translated, ms);
}

fn log_tts_start(ctx: &PipelineCtx, target: &Lang, translate_ms: u64, step_start: Instant) {
    debug!(
        "[PIPELINE] #{} {} starting TTS (translate took {}ms, total pipeline elapsed {}ms)",
        ctx.utterance_id, target, translate_ms, step_start.elapsed().as_millis()
    );
}

fn log_tts_complete(ctx: &PipelineCtx, target: &Lang, step_start: Instant) {
    debug!(
        "[PIPELINE] #{} {} complete (total {}ms since pipeline start)",
        ctx.utterance_id, target, step_start.elapsed().as_millis()
    );
}

fn log_tts_skipped(ctx: &PipelineCtx, target: &Lang) {
    debug!("[PIPELINE] #{} {} tier={}, skipping TTS", ctx.utterance_id, target, ctx.tier);
}
