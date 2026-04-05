//! Handle final transcripts from Gladia.

use std::time::Instant;
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite;
use tracing::{info, error, debug};

use crate::core::config::BYTES_PER_SEC;
use crate::stt::config::{ADAPTIVE_SAMPLE_COUNT, MAX_VALID_WPM, PASSTHROUGH_PADDING_SECS};
use crate::tts::StyleParams;
use crate::core::types::{Lang, ServerMsg};

use super::state::{ExitReason, SttState, SttContext};

// ── Context structs ─────────────────────────────────────────────────────────

/// Everything needed to emit a final utterance to clients.
struct FinalUtterance {
    transcript: String,
    utterance_id: u64,
    style_params: StyleParams,
    utterance_start: Instant,
    host_audio: Vec<u8>,
}

/// Input for spawning the full-utterance translation pipeline.
pub(crate) struct PipelineInput {
    pub text: String,
    pub utterance_id: u64,
    pub source_lang: Lang,
    pub style_params: StyleParams,
    pub utterance_start: Instant,
    pub host_audio: Vec<u8>,
    pub active: Vec<Lang>,
    pub tier: u8,
}

// ── Public API ──────────────────────────────────────────────────────────────

pub(super) async fn handle_final_transcript(
    state: &mut SttState,
    transcript: &str,
    ctx: &SttContext,
) {
    let start = state.utterance_start.take().unwrap_or_else(Instant::now);
    let host_audio = drain_host_audio(ctx);
    let sp = analyze_prosody_and_style(transcript, &host_audio);

    let utterance = FinalUtterance {
        transcript: transcript.to_string(),
        utterance_id: 0, // filled per path
        style_params: sp,
        utterance_start: start,
        host_audio,
    };

    if state.chunk_pipeline_tx.is_some() {
        finalize_chunked_utterance(state, &utterance, ctx);
    } else {
        finalize_legacy_utterance(state, &utterance, ctx);
    }

    state.reset_utterance();
    check_adaptive_endpointing(state, &utterance.host_audio, ctx).await;
}

// ── Audio drain ─────────────────────────────────────────────────────────────

fn drain_host_audio(ctx: &SttContext) -> Vec<u8> {
    let mut acc = ctx.audio_acc.lock().unwrap();
    acc.drain(..).flatten().collect()
}

// ── Prosody analysis ────────────────────────────────────────────────────────

fn analyze_prosody_and_style(transcript: &str, host_audio: &[u8]) -> StyleParams {
    let prosody = compute_prosody(host_audio, transcript);
    let emotion = crate::stt::classify_emotion(&prosody);
    log_emotion(&prosody, emotion);
    style_from_emotion(emotion)
}

fn compute_prosody(host_audio: &[u8], transcript: &str) -> crate::stt::Prosody {
    let mut prosody = crate::stt::extract_prosody(host_audio, crate::core::config::SAMPLE_RATE);
    let word_count = transcript.split_whitespace().count();
    crate::stt::compute_speaking_rate(&mut prosody, word_count);
    prosody
}

fn log_emotion(prosody: &crate::stt::Prosody, emotion: &str) {
    debug!(
        "[EMOTION] {} (energy={:.4} pitch_std={:.1} rate={}wpm)",
        emotion, prosody.energy_rms, prosody.pitch_std, prosody.speaking_rate_wpm
    );
}

fn style_from_emotion(emotion: &str) -> StyleParams {
    let vs = crate::tts::VoiceStyle::from_emotion(emotion);
    StyleParams { speed: vs.speed, emotion: emotion.to_string() }
}

// ── Chunked utterance finalization ──────────────────────────────────────────

fn finalize_chunked_utterance(
    state: &mut SttState,
    utterance: &FinalUtterance,
    ctx: &SttContext,
) {
    let uid = state.utterance_counter;
    info!("[FINAL #{}] {} (chunked, {} prior chunks)", uid, utterance.transcript, state.chunk_index);

    flush_final_chunk(state, utterance);
    state.chunk_pipeline_tx = None;
    send_final_to_host(ctx, &utterance.transcript, uid);
    queue_source_passthrough(ctx, utterance);
}

fn flush_final_chunk(state: &mut SttState, utterance: &FinalUtterance) {
    let tx = match &state.chunk_pipeline_tx {
        Some(tx) => tx,
        None => return,
    };

    let uid = state.utterance_counter;
    let flush_context = state.progressive.context().map(|s| s.to_string());
    if let Some(boundary) = state.progressive.flush(&utterance.transcript)
        && let Err(e) = tx.try_send(crate::core::types::ChunkEvent {
            text: boundary.chunk_text,
            chunk_index: state.chunk_index,
            context: flush_context,
            is_utterance_final: true,
            utterance_id: uid,
            utterance_start: utterance.utterance_start,
            host_audio: utterance.host_audio.clone(),
        }) {
            error!("[CHUNK] #{} final chunk dropped: {}", uid, e);
        }
}

// ── Legacy (non-chunked) utterance finalization ─────────────────────────────

fn finalize_legacy_utterance(
    state: &mut SttState,
    utterance: &FinalUtterance,
    ctx: &SttContext,
) {
    state.utterance_counter += 1;
    let uid = state.utterance_counter;
    info!("[FINAL #{}] {}", uid, utterance.transcript);

    let utt = FinalUtterance {
        transcript: utterance.transcript.clone(),
        utterance_id: uid,
        style_params: utterance.style_params.clone(),
        utterance_start: utterance.utterance_start,
        host_audio: utterance.host_audio.clone(),
    };

    emit_final(&utt, ctx);
}

// ── emit_final ──────────────────────────────────────────────────────────────

fn emit_final(utterance: &FinalUtterance, ctx: &SttContext) {
    let session = match ctx.sessions.get(&ctx.session_id) {
        Some(s) => s,
        None => return,
    };

    send_final_msg(&session, &utterance.transcript, utterance.utterance_id);
    let (active, tier) = read_pipeline_params(&session, utterance);
    drop(session);

    if active.is_empty() { return; }

    let input = PipelineInput {
        text: utterance.transcript.clone(),
        utterance_id: utterance.utterance_id,
        source_lang: ctx.source_lang.clone(),
        style_params: utterance.style_params.clone(),
        utterance_start: utterance.utterance_start,
        host_audio: utterance.host_audio.clone(),
        active,
        tier,
    };

    spawn_pipeline(input, ctx);
}

fn send_final_msg(
    session: &dashmap::mapref::one::Ref<'_, String, crate::core::types::Session>,
    transcript: &str,
    uid: u64,
) {
    session.send_to_host(crate::pipeline::to_ws(&ServerMsg::Final {
        transcript: transcript.to_string(),
        utterance_id: uid,
    }));
}

fn read_pipeline_params(
    session: &dashmap::mapref::one::Ref<'_, String, crate::core::types::Session>,
    utterance: &FinalUtterance,
) -> (Vec<Lang>, u8) {
    let active = session.active_langs();
    let tier = session.tier;
    let utterance_dur = Instant::now().duration_since(utterance.utterance_start);

    info!(
        "[PIPELINE] #{} active langs: {:?} tier={} utterance_dur={}ms host_audio={}B ({:.1}s)",
        utterance.utterance_id, active, tier, utterance_dur.as_millis(),
        utterance.host_audio.len(), utterance.host_audio.len() as f64 / BYTES_PER_SEC,
    );

    (active, tier)
}

fn spawn_pipeline(input: PipelineInput, ctx: &SttContext) {
    let utterance_end = Instant::now();
    let sessions_clone = ctx.sessions.clone();
    let sid = ctx.session_id.clone();

    tokio::spawn(async move {
        let req = crate::pipeline::full::PipelineRequest {
            transcript: input.text,
            utterance_id: input.utterance_id,
            source_lang: input.source_lang,
            target_langs: input.active,
            sessions: sessions_clone,
            session_id: sid,
            style_params: input.style_params,
            tier: input.tier,
            utterance_start: input.utterance_start,
            utterance_end,
            host_audio: input.host_audio,
        };
        crate::pipeline::full::run_pipeline(req).await;
    });
}

// ── Host messaging ──────────────────────────────────────────────────────────

fn send_final_to_host(ctx: &SttContext, transcript: &str, uid: u64) {
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        send_final_msg(&session, transcript, uid);
    }
}

// ── Source passthrough ──────────────────────────────────────────────────────

fn queue_source_passthrough(ctx: &SttContext, utterance: &FinalUtterance) {
    let session = match ctx.sessions.get(&ctx.session_id) {
        Some(s) => s,
        None => return,
    };
    if !session.active_langs().contains(&ctx.source_lang) { return; }
    let mgr = match session.rtmp_manager.clone() {
        Some(m) => m,
        None => return,
    };

    let lang_str = ctx.source_lang.to_string();
    let start = utterance.utterance_start;
    let pcm = truncate_passthrough_audio(&utterance.host_audio);
    tokio::spawn(async move {
        let locked = mgr.lock().await;
        locked.queue_audio(&lang_str, pcm, start);
    });
}

fn truncate_passthrough_audio(host_audio: &[u8]) -> Vec<u8> {
    let mut pcm = host_audio.to_vec();
    let max_bytes = ((host_audio.len() as f64 / BYTES_PER_SEC + PASSTHROUGH_PADDING_SECS) * BYTES_PER_SEC) as usize;
    if pcm.len() > max_bytes {
        crate::streaming::truncate_with_fadeout(&mut pcm, max_bytes);
    }
    pcm
}

// ── Adaptive endpointing ───────────────────────────────────────────────────

async fn check_adaptive_endpointing(
    state: &mut SttState,
    host_audio: &[u8],
    ctx: &SttContext,
) {
    let prosody = crate::stt::extract_prosody(host_audio, crate::core::config::SAMPLE_RATE);
    if should_skip_adaptive(state, &prosody) { return; }

    state.wpm_samples.push(prosody.speaking_rate_wpm);
    if let Some(result) = compute_adaptive_params(state) {
        apply_adaptive_result(state, result, ctx).await;
    }
}

fn should_skip_adaptive(state: &SttState, prosody: &crate::stt::Prosody) -> bool {
    state.adapted
        || prosody.speaking_rate_wpm == 0
        || prosody.speaking_rate_wpm > MAX_VALID_WPM
}

/// Result of adaptive speech-rate analysis.
struct AdaptiveResult {
    label: String,
    endpointing: f64,
    max_duration: f64,
}

fn compute_adaptive_params(state: &SttState) -> Option<AdaptiveResult> {
    if state.wpm_samples.len() < ADAPTIVE_SAMPLE_COUNT { return None; }

    let avg_wpm = state.wpm_samples.iter().sum::<u32>() as f32 / state.wpm_samples.len() as f32;
    let (label, new_endp, new_max_dur) = crate::stt::classify_speaking_speed(avg_wpm);
    Some(AdaptiveResult { label: label.to_string(), endpointing: new_endp, max_duration: new_max_dur })
}

async fn apply_adaptive_result(
    state: &mut SttState,
    result: AdaptiveResult,
    ctx: &SttContext,
) {
    state.adapted = true;

    if result.label == "normal" {
        info!("[ADAPTIVE] Avg classified: {}, keeping defaults", result.label);
        return;
    }

    log_adaptive_reconnect(&result);
    trigger_adaptive_reconnect(state, &result, ctx).await;
}

fn log_adaptive_reconnect(result: &AdaptiveResult) {
    info!(
        "[ADAPTIVE] classified: {}, reconnecting (endpointing={:.2}s, max_dur={:.0}s)",
        result.label, result.endpointing, result.max_duration
    );
}

async fn trigger_adaptive_reconnect(
    state: &mut SttState,
    result: &AdaptiveResult,
    ctx: &SttContext,
) {
    state.exit_reason = ExitReason::AdaptiveReconnect {
        endpointing: result.endpointing,
        max_duration: result.max_duration,
    };

    let mut s = ctx.sink.lock().await;
    let _ = s.send(tungstenite::Message::Text(
        r#"{"type":"stop_recording"}"#.to_string().into()
    )).await;
}
