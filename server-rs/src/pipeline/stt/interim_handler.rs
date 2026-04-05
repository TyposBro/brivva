//! Handle interim (partial) transcripts from Gladia.

use std::sync::Arc;
use std::time::Instant;
use tracing::{info, error};

use crate::tts::StyleParams;
use crate::types::{Lang, Sessions, ServerMsg};

use super::state::SttState;

pub(super) async fn handle_interim_transcript(
    state: &mut SttState,
    transcript: &str,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    acc_rx: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
) {
    mark_utterance_start(state);
    send_interim_to_host(sessions, session_id, transcript);
    detect_and_emit_chunk(state, transcript, sessions, session_id, source_lang, acc_rx);
    state.chunk_detector.check(transcript);
}

fn mark_utterance_start(state: &mut SttState) {
    if state.utterance_start.is_none() {
        state.utterance_start = Some(Instant::now());
    }
}

fn send_interim_to_host(sessions: &Sessions, session_id: &str, transcript: &str) {
    info!("[INTERIM] {}", transcript);
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(crate::pipeline::to_ws(&ServerMsg::Interim {
            transcript: transcript.to_string(),
        }));
    }
}

fn detect_and_emit_chunk(
    state: &mut SttState,
    transcript: &str,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    acc_rx: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
) {
    let pre_check_context = state.progressive.context().map(|s| s.to_string());
    let boundary = match state.progressive.check(transcript) {
        Some(b) => b,
        None => return,
    };

    spawn_pipeline_if_needed(state, sessions, session_id, source_lang);

    let tx = match &state.chunk_pipeline_tx {
        Some(tx) => tx,
        None => return,
    };

    let chunk_audio = extract_chunk_audio(acc_rx, transcript, boundary.split_pos, &mut state.last_audio_byte_sent);
    let start = state.utterance_start.unwrap();

    if let Err(e) = tx.try_send(crate::types::ChunkEvent {
        text: boundary.chunk_text,
        chunk_index: state.chunk_index,
        context: pre_check_context,
        is_utterance_final: false,
        utterance_id: state.utterance_counter,
        utterance_start: start,
        host_audio: chunk_audio,
    }) {
        error!("[CHUNK] #{}.{} dropped (channel full): {}", state.utterance_counter, state.chunk_index, e);
    }
    state.chunk_index += 1;
}

fn spawn_pipeline_if_needed(
    state: &mut SttState,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
) {
    if state.chunk_pipeline_tx.is_some() { return; }
    state.utterance_counter += 1;
    let sp = StyleParams::default();
    state.chunk_pipeline_tx = crate::pipeline::chunk::spawn_chunked_pipeline(
        sessions, session_id, state.utterance_counter, source_lang, sp,
    );
}

fn extract_chunk_audio(
    acc_rx: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    transcript: &str,
    split_pos: usize,
    last_sent: &mut usize,
) -> Vec<u8> {
    let acc = acc_rx.lock().unwrap();
    let total: Vec<u8> = acc.iter().flatten().cloned().collect();
    let ratio = if transcript.is_empty() { 0.0 } else { split_pos as f64 / transcript.len() as f64 };
    let end_pos = ((total.len() as f64 * ratio) as usize) & !1;
    let start_pos = (*last_sent).min(end_pos);
    *last_sent = end_pos;
    total[start_pos..end_pos.min(total.len())].to_vec()
}
