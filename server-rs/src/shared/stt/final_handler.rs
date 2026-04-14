//! Handle semantic endpoints from Soniox — spawn TTS for translated text.

use std::time::Instant;
use tracing::{info, error, debug};

use crate::core::types::{ServerMsg, StyleParams};

use super::state::{SttState, SttContext};

// ── Public API ──────────────────────────────────────────────────────────────

pub(super) async fn handle_translation_endpoint(
    state: &mut SttState,
    translated_text: &str,
    ctx: &SttContext,
) {
    let utterance_start = state.utterance_start.take().unwrap_or_else(Instant::now);
    let host_audio = drain_host_audio(ctx);
    let uid = state.utterance_counter;
    let target_lang = match &state.target_lang {
        Some(l) => l.clone(),
        None => return,
    };

    info!(
        "[PIPELINE] #{} {} translate done ({}ms since speech), spawning TTS ({}B host audio)",
        uid, target_lang, utterance_start.elapsed().as_millis(), host_audio.len(),
    );

    let sp = analyze_prosody_and_style(translated_text, &host_audio);
    send_translation_to_host(ctx, &target_lang, translated_text, uid);
    update_stream_subtitles(ctx, &target_lang, &state.transcript_acc, translated_text);
    spawn_tts_for_translation(ctx, &target_lang, translated_text, uid, utterance_start, &sp);
}

// ── Audio drain ─────────────────────────────────────────────────────────────

fn drain_host_audio(ctx: &SttContext) -> Vec<u8> {
    let mut acc = ctx.audio_acc.lock().unwrap();
    acc.drain(..).flatten().collect()
}

// ── Prosody analysis ────────────────────────────────────────────────────────

fn analyze_prosody_and_style(text: &str, host_audio: &[u8]) -> StyleParams {
    let prosody = compute_prosody(host_audio, text);
    let emotion = crate::shared::stt::classify_emotion(&prosody);
    log_emotion(&prosody, emotion);
    style_from_emotion(emotion)
}

fn compute_prosody(host_audio: &[u8], text: &str) -> crate::shared::stt::Prosody {
    // Cap to last 3 seconds to avoid blocking TTS spawn on long utterances
    // (autocorrelation is O(n^2) per frame, so 16s of audio → ~4.5s of CPU)
    const MAX_PROSODY_SECS: usize = 3;
    let max_bytes = MAX_PROSODY_SECS * crate::core::config::BYTES_PER_SEC as usize;
    let audio = if host_audio.len() > max_bytes {
        &host_audio[host_audio.len() - max_bytes..]
    } else {
        host_audio
    };
    let mut prosody = crate::shared::stt::extract_prosody(
        audio, crate::core::config::SAMPLE_RATE,
    );
    let word_count = text.split_whitespace().count();
    crate::shared::stt::compute_speaking_rate(&mut prosody, word_count);
    prosody
}

fn log_emotion(prosody: &crate::shared::stt::Prosody, emotion: &str) {
    debug!(
        "[EMOTION] {} (energy={:.4} pitch_std={:.1} rate={}wpm)",
        emotion, prosody.energy_rms, prosody.pitch_std, prosody.speaking_rate_wpm
    );
}

fn style_from_emotion(emotion: &str) -> StyleParams {
    let vs = crate::shared::tts::VoiceStyle::from_emotion(emotion);
    StyleParams { speed: vs.speed, emotion: emotion.to_string() }
}

// ── Host messaging ──────────────────────────────────────────────────────────

fn send_translation_to_host(ctx: &SttContext, lang: &str, text: &str, uid: u64) {
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(
            crate::features::broadcast::data::pipeline_helpers::to_ws(
                &ServerMsg::Translation {
                    lang: lang.to_string(),
                    text: text.to_string(),
                    utterance_id: uid,
                    translate_ms: 0,
                },
            ),
        );
    }
}

// ── Subtitle overlay ───────────────────────────────────────────────────────

/// Write transcript + translation to subtitle files that FFmpeg reads via drawtext reload=1.
/// Files are named by session_id + lang so each RTMP stream shows its own subtitles.
fn update_stream_subtitles(ctx: &SttContext, lang: &str, transcript: &str, translation: &str) {
    let sid = &ctx.session_id;
    let transcript_file = format!("/tmp/brivva_sub_{}_{}_transcript.txt", sid, lang);
    let translation_file = format!("/tmp/brivva_sub_{}_{}_translation.txt", sid, lang);
    let _ = std::fs::write(&transcript_file, transcript);
    let _ = std::fs::write(&translation_file, translation);
}

// ── TTS spawn ───────────────────────────────────────────────────────────────

fn spawn_tts_for_translation(
    ctx: &SttContext,
    target_lang: &str,
    translated_text: &str,
    uid: u64,
    utterance_start: Instant,
    style_params: &StyleParams,
) {
    let session = match ctx.sessions.get(&ctx.session_id) {
        Some(s) => s,
        None => {
            tracing::warn!("[TTS] #{} {} skipped: session {} gone", uid, target_lang, ctx.session_id);
            return;
        }
    };
    if session.tier < 2 {
        tracing::debug!("[TTS] #{} {} skipped: tier={} < 2", uid, target_lang, session.tier);
        return;
    }
    let voice_clone_id = session.voice_clone_id.clone();
    let use_default_voice = session.use_default_voice_langs.contains(target_lang);
    let tts_voice_gender = session.tts_voice_gender.clone();
    let tts_model = session.tts_model.clone();
    let tts_provider = session.tts_provider.clone();
    let broadcast_delay_ms = session.broadcast_delay_ms;
    let erased = session.rtmp_manager.clone();
    let pipeline_counters = session.pipeline_counters.clone();
    let latency_tracker = session.latency_tracker.clone();
    let tts_circuit_breaker = session.tts_circuit_breaker.clone();
    if erased.is_none() {
        tracing::warn!("[TTS] #{} {} no RTMP manager — TTS audio will not reach stream", uid, target_lang);
    }
    drop(session);

    let tts_req = TtsSpawnRequest {
        sessions: ctx.sessions.clone(),
        session_id: ctx.session_id.clone(),
        tts_api_key: ctx.tts_api_key.clone(),
        default_voice: ctx.default_voice.clone(),
        http_client: ctx.http_client.clone(),
        target_lang: target_lang.to_string(),
        translated_text: translated_text.to_string(),
        uid,
        utterance_start,
        style_params: style_params.clone(),
        voice_clone_id,
        use_default_voice,
        tts_voice_gender,
        tts_model,
        tts_provider,
        broadcast_delay_ms,
        erased_rtmp: erased,
        pipeline_counters,
        latency_tracker,
        tts_circuit_breaker,
    };

    tokio::spawn(run_tts_synthesis(tts_req));
}

struct TtsSpawnRequest {
    sessions: crate::core::types::Sessions,
    session_id: String,
    tts_api_key: String,
    default_voice: String,
    http_client: reqwest::Client,
    target_lang: String,
    translated_text: String,
    uid: u64,
    utterance_start: Instant,
    style_params: StyleParams,
    voice_clone_id: Option<String>,
    use_default_voice: bool,
    tts_voice_gender: String,
    tts_model: String,
    tts_provider: String,
    broadcast_delay_ms: u64,
    erased_rtmp: Option<crate::core::types::ErasedRtmpManager>,
    pipeline_counters: std::sync::Arc<crate::core::pipeline_counters::PipelineCounters>,
    latency_tracker: std::sync::Arc<crate::core::latency_tracker::LatencyTracker>,
    tts_circuit_breaker: std::sync::Arc<crate::core::circuit_breaker::CircuitBreaker>,
}

fn resolve_voice_id(req: &TtsSpawnRequest) -> String {
    if !req.use_default_voice {
        return req.voice_clone_id.as_deref().unwrap_or(&req.default_voice).to_string();
    }
    let is_male = req.tts_voice_gender == "male";
    if req.tts_provider == "dashscope" {
        if is_male { crate::core::config::DASHSCOPE_DEFAULT_VOICE_MALE.to_string() }
        else       { crate::core::config::DASHSCOPE_DEFAULT_VOICE_FEMALE.to_string() }
    } else {
        if is_male { crate::core::config::DEFAULT_VOICE_ID_MALE.to_string() }
        else       { crate::core::config::DEFAULT_VOICE_ID_FEMALE.to_string() }
    }
}

fn resolve_tts_model(req: &TtsSpawnRequest) -> String {
    if req.tts_provider == "dashscope" {
        // Flash model supports preset voices (Cherry/Ethan); VC model requires enrolled clone.
        if req.use_default_voice {
            crate::core::config::DASHSCOPE_TTS_MODEL_FLASH.to_string()
        } else {
            crate::core::config::DASHSCOPE_TTS_MODEL_VC.to_string()
        }
    } else {
        req.tts_model.clone()
    }
}

async fn run_tts_synthesis(req: TtsSpawnRequest) {
    if check_tts_circuit_breaker(&req) {
        return;
    }

    let streaming = create_streaming_slot(&req).await;

    notify_tts_start(&req);
    let tts_start = Instant::now();

    let voice_id_str = resolve_voice_id(&req);
    let tts_model_str = resolve_tts_model(&req);
    let voice_settings = crate::shared::tts::VoiceStyle::from_emotion(&req.style_params.emotion)
        .to_voice_settings(req.style_params.speed);
    let max_bytes = crate::features::broadcast::domain::pipeline_budget::compute_streaming_max_bytes(
        req.broadcast_delay_ms,
        crate::core::config::STREAMING_BUDGET_PADDING_SECS,
    );
    let tts_deadline = crate::features::broadcast::domain::pipeline_budget::compute_tts_deadline(
        req.broadcast_delay_ms,
    );

    let synth_req = crate::shared::tts::SynthesisRequest {
        text: &req.translated_text,
        voice_id: &voice_id_str,
        lang: &req.target_lang,
        voice_settings: &voice_settings,
        max_bytes,
        streaming: streaming.as_ref(),
        model_id: &tts_model_str,
        api_key: &req.tts_api_key,
    };

    execute_tts_with_fallback(&synth_req, &req, streaming.as_ref(), tts_deadline).await;

    finish_streaming(streaming.as_ref());
    record_e2e_latency(&req);
    notify_tts_end(&req, &tts_start);
}

/// Skip TTS if the circuit breaker is open. Returns true if skipped.
fn check_tts_circuit_breaker(req: &TtsSpawnRequest) -> bool {
    if req.tts_circuit_breaker.is_open() {
        tracing::warn!(
            "[TTS] #{} {} circuit breaker OPEN, skipping",
            req.uid, req.target_lang
        );
        return true;
    }
    false
}

/// Record end-to-end latency: utterance_start -> TTS complete.
fn record_e2e_latency(req: &TtsSpawnRequest) {
    let latency_ms = req.utterance_start.elapsed().as_millis() as u64;
    req.latency_tracker.record(latency_ms);
}

async fn execute_tts_with_fallback(
    synth_req: &crate::shared::tts::SynthesisRequest<'_>,
    req: &TtsSpawnRequest,
    streaming: Option<&crate::features::broadcast::data::streaming::StreamingPcm>,
    tts_deadline: std::time::Duration,
) {
    if req.tts_provider == "dashscope" {
        execute_tts_dashscope(synth_req, req, streaming, tts_deadline).await;
    } else {
        execute_tts_elevenlabs(synth_req, req, streaming, tts_deadline).await;
    }
}

async fn execute_tts_elevenlabs(
    synth_req: &crate::shared::tts::SynthesisRequest<'_>,
    req: &TtsSpawnRequest,
    streaming: Option<&crate::features::broadcast::data::streaming::StreamingPcm>,
    tts_deadline: std::time::Duration,
) {
    match tokio::time::timeout(tts_deadline, crate::shared::tts::do_tts_ws(synth_req)).await {
        Ok(Ok(bytes)) => {
            req.tts_circuit_breaker.record_success();
            info!("[TTS] #{} {} = {}KB PCM", req.uid, req.target_lang, bytes / 1024);
        }
        Ok(Err(ws_err)) => {
            tracing::warn!("[TTS] #{} {} WS failed: {}, falling back to REST",
                req.uid, req.target_lang, ws_err);
            try_rest_fallback(synth_req, req, streaming).await;
        }
        Err(_) => {
            req.pipeline_counters.increment_tts_timeouts();
            req.tts_circuit_breaker.record_failure();
            error!("[TTS] #{} {} TIMEOUT", req.uid, req.target_lang);
            if let Some(s) = streaming { s.finish(); }
        }
    }
}

async fn execute_tts_dashscope(
    synth_req: &crate::shared::tts::SynthesisRequest<'_>,
    req: &TtsSpawnRequest,
    streaming: Option<&crate::features::broadcast::data::streaming::StreamingPcm>,
    tts_deadline: std::time::Duration,
) {
    match tokio::time::timeout(tts_deadline, crate::shared::tts::do_tts_dashscope(synth_req)).await {
        Ok(Ok(bytes)) => {
            req.tts_circuit_breaker.record_success();
            info!("[TTS] #{} {} dashscope = {}KB PCM", req.uid, req.target_lang, bytes / 1024);
        }
        Ok(Err(e)) => {
            req.pipeline_counters.increment_tts_failures();
            req.tts_circuit_breaker.record_failure();
            error!("[TTS] #{} {} dashscope failed: {}", req.uid, req.target_lang, e);
            if let Some(s) = streaming { s.finish(); }
        }
        Err(_) => {
            req.pipeline_counters.increment_tts_timeouts();
            req.tts_circuit_breaker.record_failure();
            error!("[TTS] #{} {} TIMEOUT (dashscope)", req.uid, req.target_lang);
            if let Some(s) = streaming { s.finish(); }
        }
    }
}

async fn try_rest_fallback(
    synth_req: &crate::shared::tts::SynthesisRequest<'_>,
    req: &TtsSpawnRequest,
    streaming: Option<&crate::features::broadcast::data::streaming::StreamingPcm>,
) {
    match crate::shared::tts::do_tts_rest(&req.http_client, synth_req).await {
        Ok(bytes) => {
            req.tts_circuit_breaker.record_success();
            info!("[TTS] #{} {} REST fallback = {}KB PCM", req.uid, req.target_lang, bytes / 1024);
        }
        Err(rest_err) => {
            req.pipeline_counters.increment_tts_failures();
            req.tts_circuit_breaker.record_failure();
            error!("[TTS] #{} {} REST fallback also failed: {}",
                req.uid, req.target_lang, rest_err);
            if let Some(s) = streaming { s.finish(); }
        }
    }
}

async fn create_streaming_slot(
    req: &TtsSpawnRequest,
) -> Option<crate::features::broadcast::data::streaming::StreamingPcm> {
    let erased = match req.erased_rtmp.as_ref() {
        Some(e) => e,
        None => {
            tracing::warn!("[TTS] #{} {} no RTMP manager, streaming slot skipped", req.uid, req.target_lang);
            return None;
        }
    };
    let mgr = match crate::features::broadcast::data::streaming::downcast_rtmp_manager(erased) {
        Some(m) => m,
        None => {
            tracing::error!("[TTS] #{} {} RTMP manager downcast failed", req.uid, req.target_lang);
            return None;
        }
    };
    let mut locked = mgr.lock().await;
    Some(locked.queue_streaming_audio(&req.target_lang, req.utterance_start))
}

fn finish_streaming(streaming: Option<&crate::features::broadcast::data::streaming::StreamingPcm>) {
    if let Some(s) = streaming {
        s.finish();
    }
}

fn notify_tts_start(req: &TtsSpawnRequest) {
    if let Some(session) = req.sessions.get(&req.session_id) {
        session.send_to_host(
            crate::features::broadcast::data::pipeline_helpers::to_ws(
                &ServerMsg::TtsStart {
                    lang: req.target_lang.clone(),
                    utterance_id: req.uid,
                },
            ),
        );
    }
}

fn notify_tts_end(req: &TtsSpawnRequest, tts_start: &Instant) {
    if let Some(session) = req.sessions.get(&req.session_id) {
        session.send_to_host(
            crate::features::broadcast::data::pipeline_helpers::to_ws(
                &ServerMsg::TtsEnd {
                    lang: req.target_lang.clone(),
                    utterance_id: req.uid,
                    tts_ms: tts_start.elapsed().as_millis() as u64,
                },
            ),
        );
    }
}
