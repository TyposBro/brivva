//! Handle interim (partial) transcripts from Gladia.

use std::time::Instant;
use tracing::{info, error};

use crate::core::types::StyleParams;
use crate::features::broadcast::domain::ServerMsg;

use super::state::{SttState, SttContext};

pub(super) async fn handle_interim_transcript(
    state: &mut SttState,
    transcript: &str,
    ctx: &SttContext,
) {
    mark_utterance_start(state);
    send_interim_to_host(ctx, transcript);
    detect_and_emit_chunk(state, transcript, ctx);
    state.chunk_detector.check(transcript);
}

fn mark_utterance_start(state: &mut SttState) {
    if state.utterance_start.is_none() {
        state.utterance_start = Some(Instant::now());
    }
}

fn send_interim_to_host(ctx: &SttContext, transcript: &str) {
    info!("[INTERIM] {}", transcript);
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(crate::pipeline::to_ws(&ServerMsg::Interim {
            transcript: transcript.to_string(),
        }));
    }
}

/// A detected chunk boundary with its surrounding context.
struct DetectedChunk {
    boundary: crate::stt::ChunkBoundary,
    context: Option<String>,
    transcript_len: usize,
}

fn detect_and_emit_chunk(
    state: &mut SttState,
    transcript: &str,
    ctx: &SttContext,
) {
    let context = state.progressive.context().map(|s| s.to_string());
    let boundary = match state.progressive.check(transcript) {
        Some(b) => b,
        None => return,
    };

    let detected = DetectedChunk { boundary, context, transcript_len: transcript.len() };
    spawn_pipeline_if_needed(state, ctx);
    let event = build_chunk_event(state, detected, ctx);
    try_send_chunk(state, event);
}

fn build_chunk_event(
    state: &mut SttState,
    detected: DetectedChunk,
    ctx: &SttContext,
) -> crate::core::types::ChunkEvent {
    let audio = extract_chunk_audio(state, &detected, ctx);
    crate::core::types::ChunkEvent {
        text: detected.boundary.chunk_text,
        chunk_index: state.chunk_index,
        context: detected.context,
        is_utterance_final: false,
        utterance_id: state.utterance_counter,
        utterance_start: state.utterance_start.unwrap(),
        host_audio: audio,
    }
}

fn try_send_chunk(state: &mut SttState, event: crate::core::types::ChunkEvent) {
    let tx = match &state.chunk_pipeline_tx {
        Some(tx) => tx,
        None => return,
    };
    if let Err(e) = tx.try_send(event) {
        error!("[CHUNK] #{}.{} dropped (channel full): {}", state.utterance_counter, state.chunk_index, e);
    }
    state.chunk_index += 1;
}

fn spawn_pipeline_if_needed(state: &mut SttState, ctx: &SttContext) {
    if state.chunk_pipeline_tx.is_some() { return; }
    state.utterance_counter += 1;
    let req = crate::pipeline::chunk::ChunkPipelineRequest {
        sessions: ctx.sessions.clone(),
        session_id: ctx.session_id.clone(),
        utterance_id: state.utterance_counter,
        source_lang: ctx.source_lang.clone(),
        style_params: StyleParams::default(),
    };
    state.chunk_pipeline_tx = crate::pipeline::chunk::spawn_chunked_pipeline(req);
}

fn extract_chunk_audio(
    state: &mut SttState,
    detected: &DetectedChunk,
    ctx: &SttContext,
) -> Vec<u8> {
    let acc = ctx.audio_acc.lock().unwrap();
    let total: Vec<u8> = acc.iter().flatten().cloned().collect();
    let ratio = if detected.transcript_len == 0 { 0.0 } else { detected.boundary.split_pos as f64 / detected.transcript_len as f64 };
    let end_pos = ((total.len() as f64 * ratio) as usize) & !1;
    let start_pos = state.last_audio_byte_sent.min(end_pos);
    state.last_audio_byte_sent = end_pos;
    total[start_pos..end_pos.min(total.len())].to_vec()
}
