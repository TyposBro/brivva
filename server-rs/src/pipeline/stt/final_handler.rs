//! Handle final transcripts from Gladia.

use std::sync::Arc;
use std::time::Instant;
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite;
use tracing::{info, error, debug};

use crate::constants::BYTES_PER_SEC;
use crate::stt::config::{ADAPTIVE_SAMPLE_COUNT, MAX_VALID_WPM, PASSTHROUGH_PADDING_SECS};
use crate::tts::StyleParams;
use crate::types::{Lang, Sessions, ServerMsg};

use super::state::{SttState, WsStream};

pub(super) async fn handle_final_transcript(
    state: &mut SttState,
    transcript: &str,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    acc_rx: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    sink: &Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
) {
    let start = state.utterance_start.take().unwrap_or_else(Instant::now);
    let host_audio = drain_host_audio(acc_rx);
    let sp = analyze_prosody_and_style(transcript, &host_audio);

    if state.chunk_pipeline_tx.is_some() {
        finalize_chunked_utterance(state, transcript, &sp, start, &host_audio, sessions, session_id, source_lang);
    } else {
        finalize_legacy_utterance(state, transcript, sp, start, host_audio.clone(), sessions, session_id, source_lang);
    }

    state.reset_utterance();
    check_adaptive_endpointing(state, &host_audio, sink).await;
}

fn drain_host_audio(acc_rx: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>) -> Vec<u8> {
    let mut acc = acc_rx.lock().unwrap();
    acc.drain(..).flatten().collect()
}

fn analyze_prosody_and_style(transcript: &str, host_audio: &[u8]) -> StyleParams {
    let mut prosody = crate::stt::extract_prosody(host_audio, crate::constants::SAMPLE_RATE);
    let word_count = transcript.split_whitespace().count();
    crate::stt::compute_speaking_rate(&mut prosody, word_count);
    let emotion = crate::stt::classify_emotion(&prosody);
    let vs = crate::tts::VoiceStyle::from_emotion(emotion);

    debug!(
        "[EMOTION] {} (energy={:.4} pitch_std={:.1} rate={}wpm)",
        emotion, prosody.energy_rms, prosody.pitch_std, prosody.speaking_rate_wpm
    );

    StyleParams { speed: vs.speed, emotion: emotion.to_string() }
}

fn finalize_chunked_utterance(
    state: &mut SttState,
    transcript: &str,
    _sp: &StyleParams,
    start: Instant,
    host_audio: &[u8],
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
) {
    let uid = state.utterance_counter;
    info!("[FINAL #{}] {} (chunked, {} prior chunks)", uid, transcript, state.chunk_index);

    flush_final_chunk(state, transcript, uid, start, host_audio);
    state.chunk_pipeline_tx = None;
    send_final_to_host(sessions, session_id, transcript, uid);
    queue_source_passthrough(sessions, session_id, source_lang, host_audio, start);
}

fn finalize_legacy_utterance(
    state: &mut SttState,
    transcript: &str,
    sp: StyleParams,
    start: Instant,
    host_audio: Vec<u8>,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
) {
    state.utterance_counter += 1;
    let uid = state.utterance_counter;
    info!("[FINAL #{}] {}", uid, transcript);
    emit_final(sessions, session_id, transcript, uid, source_lang, Some(sp), start, host_audio);
}

fn flush_final_chunk(
    state: &mut SttState,
    transcript: &str,
    uid: u64,
    start: Instant,
    host_audio: &[u8],
) {
    let tx = match &state.chunk_pipeline_tx {
        Some(tx) => tx,
        None => return,
    };

    let flush_context = state.progressive.context().map(|s| s.to_string());
    if let Some(boundary) = state.progressive.flush(transcript)
        && let Err(e) = tx.try_send(crate::types::ChunkEvent {
            text: boundary.chunk_text,
            chunk_index: state.chunk_index,
            context: flush_context,
            is_utterance_final: true,
            utterance_id: uid,
            utterance_start: start,
            host_audio: host_audio.to_vec(),
        }) {
            error!("[CHUNK] #{} final chunk dropped: {}", uid, e);
        }
}

fn send_final_to_host(sessions: &Sessions, session_id: &str, transcript: &str, uid: u64) {
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(crate::pipeline::to_ws(&ServerMsg::Final {
            transcript: transcript.to_string(),
            utterance_id: uid,
        }));
    }
}

fn queue_source_passthrough(
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    host_audio: &[u8],
    start: Instant,
) {
    let session = match sessions.get(session_id) {
        Some(s) => s,
        None => return,
    };
    if !session.active_langs().contains(source_lang) { return; }
    let mgr = match session.rtmp_manager.clone() {
        Some(m) => m,
        None => return,
    };

    let lang_str = source_lang.to_string();
    let mut pcm = host_audio.to_vec();
    let max_bytes = ((host_audio.len() as f64 / BYTES_PER_SEC + PASSTHROUGH_PADDING_SECS) * BYTES_PER_SEC) as usize;
    if pcm.len() > max_bytes {
        crate::ffmpeg::truncate_with_fadeout(&mut pcm, max_bytes);
    }
    tokio::spawn(async move {
        let locked = mgr.lock().await;
        locked.queue_audio(&lang_str, pcm, start);
    });
}

async fn check_adaptive_endpointing(
    state: &mut SttState,
    host_audio: &[u8],
    sink: &Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
) {
    let prosody = crate::stt::extract_prosody(host_audio, crate::constants::SAMPLE_RATE);
    if state.adapted || prosody.speaking_rate_wpm == 0 || prosody.speaking_rate_wpm > MAX_VALID_WPM {
        return;
    }

    state.wpm_samples.push(prosody.speaking_rate_wpm);
    if state.wpm_samples.len() < ADAPTIVE_SAMPLE_COUNT { return; }

    let avg_wpm = state.wpm_samples.iter().sum::<u32>() as f32 / state.wpm_samples.len() as f32;
    let (label, new_endp, new_max_dur) = crate::stt::classify_speaking_speed(avg_wpm);
    state.adapted = true;

    if label == "normal" {
        info!("[ADAPTIVE] Avg WPM: {:.0}, classified: {}, keeping defaults", avg_wpm, label);
        return;
    }

    info!(
        "[ADAPTIVE] Avg WPM: {:.0}, classified: {}, reconnecting (endpointing={:.2}s, max_dur={:.0}s)",
        avg_wpm, label, new_endp, new_max_dur
    );
    state.adaptive_params = Some((new_endp, new_max_dur));
    state.needs_adaptive_reconnect = true;
    state.disconnected = true;

    let mut s = sink.lock().await;
    let _ = s.send(tungstenite::Message::Text(
        r#"{"type":"stop_recording"}"#.to_string().into()
    )).await;
}

fn emit_final(
    sessions: &Sessions,
    session_id: &str,
    transcript: &str,
    uid: u64,
    source_lang: &Lang,
    style_params: Option<StyleParams>,
    utterance_start: Instant,
    host_audio: Vec<u8>,
) {
    let utterance_end = Instant::now();
    let utterance_dur = utterance_end.duration_since(utterance_start);

    let session = match sessions.get(session_id) {
        Some(s) => s,
        None => return,
    };

    session.send_to_host(crate::pipeline::to_ws(&ServerMsg::Final {
        transcript: transcript.to_string(),
        utterance_id: uid,
    }));

    let active = session.active_langs();
    let sp = style_params.unwrap_or_default();
    let tier = session.tier;
    info!(
        "[PIPELINE] #{} active langs: {:?} tier={} utterance_dur={}ms host_audio={}B ({:.1}s)",
        uid, active, tier, utterance_dur.as_millis(),
        host_audio.len(), host_audio.len() as f64 / BYTES_PER_SEC,
    );

    if active.is_empty() { return; }

    let sessions_clone = sessions.clone();
    let sid = session_id.to_string();
    let src = source_lang.clone();
    let text = transcript.to_string();
    drop(session);
    tokio::spawn(async move {
        crate::pipeline::tts::run_pipeline(
            &text, uid, &src, &active, &sessions_clone, &sid, &sp, tier,
            utterance_start, utterance_end, host_audio,
        ).await;
    });
}
