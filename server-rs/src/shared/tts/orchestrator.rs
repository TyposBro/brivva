//! TTS orchestrator: coordinates deadline, voice selection, and execution.

use std::time::Instant;
use tracing::{info, error, debug};

use crate::core::config::{BYTES_PER_SEC, DEFAULT_BROADCAST_DELAY_MS};
use crate::core::pipeline_budget::{compute_tts_deadline, compute_max_pcm_bytes};
use crate::core::types::{Lang, ServerMsg, Sessions, StyleParams};
use super::voice_settings::VoiceStyle;
use super::SynthesisRequest;

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
    pub tts_api_key: &'a str,
    pub tts_provider: &'a str,
    pub default_voice: &'a str,
}

/// Session-scoped environment for TTS execution.
pub struct TtsEnv<'a> {
    pub client: &'a reqwest::Client,
    pub sessions: &'a Sessions,
    pub session_id: &'a str,
}

/// TTS entry point -- routes to ElevenLabs or DashScope based on provider.
pub async fn do_tts(
    env: &TtsEnv<'_>,
    req: &TtsRequest<'_>,
) {
    let tts_start = Instant::now();
    let lang_str = req.lang.to_string();
    let ctx = prepare_context(req, env.sessions, env.session_id);

    log_tts_start(req, &ctx);

    let streaming = allocate_rtmp_slot(env.sessions, env.session_id, &lang_str, req.utterance_start).await;
    log_streaming_slot(req.utterance_id, &lang_str, ctx.max_bytes, &streaming);
    notify_host(env.sessions, env.session_id, ServerMsg::TtsStart { lang: lang_str.clone(), utterance_id: req.utterance_id });

    let synth_req = build_synthesis_request(req, &ctx, &lang_str, &streaming);
    let tts_result = execute_tts_with_fallback(env.client, &synth_req, &ctx, req.tts_provider).await;

    let outcome = TtsOutcome {
        streaming,
        tts_start,
        tts_result: &tts_result,
        lang_str: &lang_str,
        utterance_id: req.utterance_id,
        deadline: &ctx.tts_deadline,
    };
    finalize(&outcome);
    notify_host(env.sessions, env.session_id, ServerMsg::TtsEnd { lang: lang_str, utterance_id: req.utterance_id, tts_ms: tts_start.elapsed().as_millis() as u64 });
}

// ── Synthesizer implementation ───

/// ElevenLabs synthesizer: WebSocket streaming with REST fallback.
pub struct ElevenLabsSynthesizer {
    client: reqwest::Client,
}

impl ElevenLabsSynthesizer {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl super::Synthesizer for ElevenLabsSynthesizer {
    async fn synthesize(
        &self,
        req: &SynthesisRequest<'_>,
    ) -> Result<usize, String> {
        let ws_result = super::ws::do_tts_ws(req).await;
        match ws_result {
            Ok(bytes) => Ok(bytes),
            Err(ws_err) => {
                tracing::warn!("[TTS] WS failed: {}, falling back to REST", ws_err);
                fallback_to_rest(&self.client, req).await
            }
        }
    }
}

async fn fallback_to_rest(
    client: &reqwest::Client,
    req: &SynthesisRequest<'_>,
) -> Result<usize, String> {
    super::rest::do_tts_rest(client, req).await
}

// ── Internal types ───

struct TtsContext {
    voice_id: String,
    voice_settings: serde_json::Value,
    max_bytes: usize,
    tts_deadline: std::time::Duration,
}

struct TtsOutcome<'a> {
    streaming: Option<crate::features::broadcast::data::streaming::StreamingPcm>,
    tts_start: Instant,
    tts_result: &'a Result<Result<usize, String>, tokio::time::error::Elapsed>,
    lang_str: &'a str,
    utterance_id: u64,
    deadline: &'a std::time::Duration,
}

// ── Orchestration helpers ───

fn prepare_context(req: &TtsRequest<'_>, sessions: &Sessions, session_id: &str) -> TtsContext {
    let broadcast_delay_ms = read_broadcast_delay(sessions, session_id);
    TtsContext {
        voice_id: select_voice(req.voice_clone_id, req.default_voice),
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

fn build_synthesis_request<'a>(
    req: &'a TtsRequest<'_>,
    ctx: &'a TtsContext,
    lang_str: &'a str,
    streaming: &'a Option<crate::features::broadcast::data::streaming::StreamingPcm>,
) -> SynthesisRequest<'a> {
    SynthesisRequest {
        text: req.text,
        voice_id: &ctx.voice_id,
        lang: lang_str,
        voice_settings: &ctx.voice_settings,
        max_bytes: ctx.max_bytes,
        streaming: streaming.as_ref(),
        model_id: req.tts_model,
        api_key: req.tts_api_key,
    }
}

async fn execute_tts_with_fallback(
    client: &reqwest::Client,
    synth_req: &SynthesisRequest<'_>,
    ctx: &TtsContext,
    provider: &str,
) -> Result<Result<usize, String>, tokio::time::error::Elapsed> {
    if provider == "dashscope" {
        tokio::time::timeout(ctx.tts_deadline, async {
            super::dashscope_ws::do_tts_dashscope(synth_req).await
        }).await
    } else {
        let synth = ElevenLabsSynthesizer::new(client.clone());
        run_with_deadline(&synth, synth_req, ctx).await
    }
}

async fn run_with_deadline(
    synth: &impl super::Synthesizer,
    synth_req: &SynthesisRequest<'_>,
    ctx: &TtsContext,
) -> Result<Result<usize, String>, tokio::time::error::Elapsed> {
    tokio::time::timeout(ctx.tts_deadline, async {
        synth.synthesize(synth_req).await
    }).await
}

fn finalize(outcome: &TtsOutcome<'_>) {
    if let Some(ref s) = outcome.streaming { s.finish(); }
    let tts_ms = outcome.tts_start.elapsed().as_millis() as u64;
    log_tts_result(outcome, tts_ms);
}

// ── Voice selection ───

fn select_voice(voice_clone_id: Option<&str>, default_voice: &str) -> String {
    voice_clone_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| default_voice.to_string())
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
) -> Option<crate::features::broadcast::data::streaming::StreamingPcm> {
    let erased = sessions.get(session_id)?.rtmp_manager.clone()?;
    let rtmp_mgr = crate::features::broadcast::data::streaming::downcast_rtmp_manager(&erased)?;
    let mut mgr = rtmp_mgr.lock().await;
    Some(mgr.queue_streaming_audio(lang))
}

fn log_streaming_slot(utterance_id: u64, lang: &str, max_bytes: usize, streaming: &Option<crate::features::broadcast::data::streaming::StreamingPcm>) {
    if streaming.is_some() {
        debug!("[TTS] #{} {} queued streaming slot (max={}B={:.1}s)", utterance_id, lang, max_bytes, max_bytes as f64 / BYTES_PER_SEC);
    }
}

// ── Host notification ───

fn notify_host(sessions: &Sessions, session_id: &str, msg: ServerMsg) {
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(crate::features::broadcast::data::pipeline_helpers::to_ws(&msg));
    }
}

// ── Logging ───

fn log_tts_start(req: &TtsRequest<'_>, ctx: &TtsContext) {
    info!(
        "[TTS] {} WS voice={}{} lang={} emotion={} speed={:.2} text='{}' [deadline={}ms]",
        req.tts_provider,
        &ctx.voice_id[..8.min(ctx.voice_id.len())],
        if req.voice_clone_id.is_some() { " (cloned)" } else { "" },
        req.lang, req.style_params.emotion, req.style_params.speed, req.text,
        ctx.tts_deadline.as_millis()
    );
}

fn log_tts_result(outcome: &TtsOutcome<'_>, tts_ms: u64) {
    match outcome.tts_result {
        Ok(Ok(total_bytes)) => info!("[TTS] {}KB in {}ms for {} (streaming PCM)", total_bytes / 1024, tts_ms, outcome.lang_str),
        Ok(Err(e)) => error!("[TTS] Failed for {}: {} ({}ms)", outcome.lang_str, e, tts_ms),
        Err(_) => error!("[TTS] TIMEOUT: utterance {} for {} exceeded {}ms", outcome.utterance_id, outcome.lang_str, outcome.deadline.as_millis()),
    }
}
