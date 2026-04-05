use std::time::{Duration, Instant};
use tracing::{info, debug};

use crate::core::config::BYTES_PER_SEC;
use crate::shared::stt::config::PASSTHROUGH_PADDING_SECS;
use crate::core::types::StyleParams;
use crate::shared::tts::TtsRequest;
use crate::core::types::Lang;
use crate::features::broadcast::domain::Sessions;

// ── Public request struct ───

/// All inputs for a TTS pipeline that receives pre-translated text from Soniox.
pub(crate) struct PipelineRequest {
    pub text: String,
    pub utterance_id: u64,
    pub source_lang: Lang,
    pub target_langs: Vec<Lang>,
    pub sessions: Sessions,
    pub session_id: String,
    pub style_params: StyleParams,
    pub tier: u8,
    pub utterance_start: Instant,
    pub utterance_end: Instant,
    pub host_audio: Vec<u8>,
    pub tts_api_key: String,
    pub default_voice: String,
    pub http_client: reqwest::Client,
}

// ── Internal context ───

/// Shared state for the TTS pipeline. Cloned at `tokio::spawn` boundaries
/// instead of cloning 8+ individual fields.
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
    tts_api_key: String,
    default_voice: String,
    http_client: reqwest::Client,
}

/// Owned per-language task for `tokio::spawn` boundaries.
struct LangTask {
    ctx: PipelineCtx,
    text: String,
    target: Lang,
}

// ── TTS Pipeline ───

pub(crate) async fn run_pipeline(req: PipelineRequest) {
    let pipeline_start = Instant::now();
    let ctx = build_ctx(&req);

    log_pipeline_start(&ctx, &req);

    let handles = spawn_all_lang_tasks(&ctx, &req);
    await_all(handles).await;

    log_pipeline_complete(&ctx, pipeline_start);
}

// ── Context Construction ───

fn build_ctx(req: &PipelineRequest) -> PipelineCtx {
    let (voice_clone_id, tts_model) = read_voice_config(&req.sessions, &req.session_id);

    PipelineCtx {
        sessions: req.sessions.clone(),
        session_id: req.session_id.clone(),
        utterance_id: req.utterance_id,
        source_lang: req.source_lang.clone(),
        style_params: req.style_params.clone(),
        tier: req.tier,
        voice_clone_id,
        tts_model,
        utterance_start: req.utterance_start,
        utterance_end: req.utterance_end,
        tts_api_key: req.tts_api_key.clone(),
        default_voice: req.default_voice.clone(),
        http_client: req.http_client.clone(),
    }
}

fn read_voice_config(sessions: &Sessions, session_id: &str) -> (Option<String>, String) {
    match sessions.get(session_id) {
        Some(s) => (s.voice_clone_id.clone(), s.tts_model.clone()),
        None => (None, crate::core::config::DEFAULT_TTS_MODEL.to_string()),
    }
}

// ── Spawning ───

fn spawn_all_lang_tasks(
    ctx: &PipelineCtx,
    req: &PipelineRequest,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles = Vec::new();

    for lang in &req.target_langs {
        if lang == &ctx.source_lang {
            handles.extend(spawn_source_passthrough(ctx, lang, &req.host_audio));
        } else {
            let task = LangTask {
                ctx: ctx.clone(),
                text: req.text.clone(),
                target: lang.clone(),
            };
            handles.push(spawn_tts(task));
        }
    }

    handles
}

fn spawn_source_passthrough(
    ctx: &PipelineCtx,
    lang: &Lang,
    host_audio: &[u8],
) -> Option<tokio::task::JoinHandle<()>> {
    let erased = ctx.sessions.get(&ctx.session_id).and_then(|s| s.rtmp_manager.clone())?;
    let mgr = super::streaming::downcast_rtmp_manager(&erased)?;
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

fn spawn_tts(task: LangTask) -> tokio::task::JoinHandle<()> {
    let client = task.ctx.http_client.clone();
    tokio::spawn(async move {
        run_tts(&task, &client).await;
    })
}

// ── TTS Task ───

async fn run_tts(task: &LangTask, _client: &reqwest::Client) {
    let ctx = &task.ctx;
    if ctx.tier < 2 {
        log_tts_skipped(ctx, &task.target);
        return;
    }

    log_tts_start(task);
    let req = build_tts_request(ctx, &task.text, &task.target);
    let tts_env = crate::shared::tts::TtsEnv {
        client: _client,
        sessions: &ctx.sessions,
        session_id: &ctx.session_id,
    };
    crate::shared::tts::do_tts(&tts_env, &req).await;
    log_tts_complete(task);
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
        tts_api_key: &ctx.tts_api_key,
        default_voice: &ctx.default_voice,
    }
}

// ── Helpers ───

fn truncate_if_too_long(pcm: &mut Vec<u8>, utterance_start: Instant, utterance_end: Instant) {
    let utterance_dur = utterance_end.duration_since(utterance_start);
    let max_dur = utterance_dur + Duration::from_secs_f64(PASSTHROUGH_PADDING_SECS);
    let max_bytes = (max_dur.as_secs_f64() * BYTES_PER_SEC) as usize;
    if pcm.len() > max_bytes {
        super::streaming::truncate_with_fadeout(pcm, max_bytes);
    }
}

async fn await_all(handles: Vec<tokio::task::JoinHandle<()>>) {
    for handle in handles {
        let _ = handle.await;
    }
}

// ── Logging ───

fn log_pipeline_start(ctx: &PipelineCtx, req: &PipelineRequest) {
    info!(
        "[PIPELINE] #{} starting TTS: '{}' -> {:?} (voice_clone={}) delay_since_utterance_start={}ms",
        ctx.utterance_id, &req.text[..req.text.len().min(60)],
        req.target_langs.iter().map(|l| l.to_string()).collect::<Vec<_>>(),
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

fn log_tts_start(task: &LangTask) {
    debug!(
        "[PIPELINE] #{} {} starting TTS (total pipeline elapsed {}ms)",
        task.ctx.utterance_id, task.target, task.ctx.utterance_start.elapsed().as_millis()
    );
}

fn log_tts_complete(task: &LangTask) {
    debug!(
        "[PIPELINE] #{} {} TTS complete (total {}ms since utterance start)",
        task.ctx.utterance_id, task.target, task.ctx.utterance_start.elapsed().as_millis()
    );
}

fn log_tts_skipped(ctx: &PipelineCtx, target: &Lang) {
    debug!("[PIPELINE] #{} {} tier={}, skipping TTS", ctx.utterance_id, target, ctx.tier);
}
