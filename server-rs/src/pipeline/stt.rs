use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tracing::{info, error, debug};

use crate::constants::{
    BYTES_PER_SEC, STT_RECONNECT_MAX, STT_RECONNECT_DELAY_SECS,
};
use crate::tts::StyleParams;
use crate::types::{Lang, Sessions, ServerMsg};

use super::STT_API_KEY;
use super::to_ws;
use super::chunk::spawn_chunked_pipeline;
use super::tts::run_pipeline;

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

    if let Some(session) = sessions.get(session_id) {
        let final_msg = to_ws(&ServerMsg::Final {
            transcript: transcript.to_string(),
            utterance_id: uid,
        });
        session.send_to_host(final_msg);

        let active = session.active_langs();
        let sp = style_params.unwrap_or_default();
        let tier = session.tier;
        info!(
            "[PIPELINE] #{} active langs: {:?} tier={} utterance_dur={}ms host_audio={}B ({:.1}s) text_len={}",
            uid, active, tier, utterance_dur.as_millis(),
            host_audio.len(), host_audio.len() as f64 / BYTES_PER_SEC,
            transcript.len()
        );
        if !active.is_empty() {
            let sessions_clone = sessions.clone();
            let sid = session_id.to_string();
            let src = source_lang.clone();
            let text = transcript.to_string();
            tokio::spawn(async move {
                run_pipeline(
                    &text, uid, &src, &active, &sessions_clone, &sid, &sp, tier,
                    utterance_start, utterance_end, host_audio,
                ).await;
            });
        }
    }
}

/// Create a Gladia live session via POST, returns WebSocket URL.
async fn create_gladia_session(
    lang: &str,
    endpointing: f64,
    max_duration: f64,
) -> Result<crate::stt::GladiaSession, String> {
    let body = serde_json::json!({
        "encoding": "wav/pcm",
        "bit_depth": 16,
        "sample_rate": 44100,
        "channels": 1,
        "endpointing": endpointing,
        "maximum_duration_without_endpointing": max_duration,
        "language_config": {
            "languages": [lang],
            "code_switching": true
        },
        "messages_config": {
            "receive_partial_transcripts": true,
            "receive_final_transcripts": true,
            "receive_speech_events": true,
            "receive_acknowledgments": false,
            "receive_lifecycle_events": false,
            "receive_pre_processing_events": false,
            "receive_realtime_processing_events": false,
            "receive_post_processing_events": false,
            "receive_errors": true
        },
        "realtime_processing": {
            "words_accurate_timestamps": true
        }
    });

    let resp = crate::HTTP_CLIENT
        .post("https://api.gladia.io/v2/live")
        .header("Content-Type", "application/json")
        .header("x-gladia-key", &*STT_API_KEY)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Gladia session POST failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Gladia error {}: {}", status, body));
    }

    resp.json::<crate::stt::GladiaSession>()
        .await
        .map_err(|e| format!("Gladia session parse error: {}", e))
}

// ── STT Helper Types ─────────────────────────────────────

/// Mutable state carried across Gladia messages within one WS connection.
struct SttState {
    utterance_counter: u64,
    utterance_start: Option<Instant>,
    chunk_detector: Box<dyn crate::stt::ChunkDetector>,
    progressive: crate::stt::ProgressiveChunkDetector,
    chunk_index: u16,
    chunk_pipeline_tx: Option<mpsc::Sender<crate::types::ChunkEvent>>,
    last_audio_byte_sent: usize,
    wpm_samples: Vec<u32>,
    adapted: bool,
    disconnected: bool,
    needs_adaptive_reconnect: bool,
    adaptive_params: Option<(f64, f64)>,
}

/// Control-flow signal returned by [`process_gladia_message`].
enum MessageAction {
    Continue,
    Break,
}

// ── STT Helper Functions ─────────────────────────────────

/// WebSocket stream type alias (avoids spelling out the full generic).
type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Try to create a Gladia live session and connect the WebSocket.
///
/// Returns the connected stream on success, or `None` if all attempts fail
/// (in which case the caller should stop STT).
async fn connect_gladia(
    session_id: &str,
    sessions: &Sessions,
    source_lang: &Lang,
    endpointing: f64,
    max_duration: f64,
    reconnect_count: u32,
) -> Option<WsStream> {
    let max_attempts = if reconnect_count == 0 { 10 } else { STT_RECONNECT_MAX };
    let reconnect_delay = Duration::from_secs(STT_RECONNECT_DELAY_SECS);

    for attempt in 1..=max_attempts {
        if !sessions.contains_key(session_id) {
            info!("[STT] Session {} gone, stopping", session_id);
            return None;
        }

        // Step 1: Create Gladia session via POST
        let gladia_session = match create_gladia_session(
            &source_lang.to_string(), endpointing, max_duration,
        ).await {
            Ok(s) => s,
            Err(e) => {
                let delay = if reconnect_count == 0 {
                    Duration::from_secs(3)
                } else {
                    reconnect_delay
                };
                error!("[STT] session create attempt {}/{} failed: {}", attempt, max_attempts, e);
                tokio::time::sleep(delay).await;
                continue;
            }
        };

        // Step 2: Connect WebSocket to returned URL
        use tungstenite::client::IntoClientRequest;
        let request = match gladia_session.url.into_client_request() {
            Ok(req) => req,
            Err(e) => {
                error!("[STT] Failed to build WS request: {}", e);
                return None;
            }
        };

        match tokio_tungstenite::connect_async(request).await {
            Ok((stream, _)) => {
                info!(
                    "[STT] Connected to Gladia Solaria-1 (attempt {}, endpointing={:.2}s, max_dur={:.0}s, session={})",
                    attempt, endpointing, max_duration, gladia_session.id
                );
                return Some(stream);
            }
            Err(e) => {
                let delay = if reconnect_count == 0 {
                    Duration::from_secs(3)
                } else {
                    reconnect_delay
                };
                error!("[STT] WS connect attempt {}/{} failed: {}", attempt, max_attempts, e);
                tokio::time::sleep(delay).await;
            }
        }
    }

    error!("[STT] Failed to connect after {} attempts", max_attempts);
    None
}

/// Handle a final transcript from Gladia.
///
/// Flushes progressive chunks, emits the final event, queues source-language
/// passthrough audio, runs adaptive endpointing analysis, and resets per-utterance
/// state.
async fn handle_final_transcript(
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
    let host_audio: Vec<u8> = {
        let mut acc = acc_rx.lock().unwrap();
        acc.drain(..).flatten().collect()
    };

    // Prosody -> emotion -> style params
    let mut prosody = crate::stt::extract_prosody(&host_audio, 44100);
    let word_count = transcript.split_whitespace().count();
    crate::stt::compute_speaking_rate(&mut prosody, word_count);
    let emotion = crate::stt::classify_emotion(&prosody);
    let (_, _, _, speed) = crate::stt::map_style(emotion);

    debug!(
        "[EMOTION] {} (energy={:.4} pitch_std={:.1} rate={}wpm)",
        emotion, prosody.energy_rms, prosody.pitch_std, prosody.speaking_rate_wpm
    );

    let sp = StyleParams {
        speed,
        emotion: emotion.to_string(),
    };

    // Flush remaining text as final chunk via progressive pipeline
    if let Some(ref tx) = state.chunk_pipeline_tx {
        // Use the same utterance ID that was assigned when the
        // chunked pipeline was spawned (don't increment uc again)
        let uid = state.utterance_counter;
        info!("[FINAL #{}] {} (chunked, {} prior chunks)", uid, transcript, state.chunk_index);

        let flush_context = state.progressive.context().map(|s| s.to_string());
        if let Some(boundary) = state.progressive.flush(transcript) {
            if let Err(e) = tx.try_send(crate::types::ChunkEvent {
                text: boundary.chunk_text,
                chunk_index: state.chunk_index,
                context: flush_context,
                is_utterance_final: true,
                utterance_id: uid,
                utterance_start: start,
                host_audio: host_audio.clone(),
            }) {
                error!("[CHUNK] #{} final chunk dropped: {}", uid, e);
            }
        }
        // Drop the sender to signal pipeline completion
        state.chunk_pipeline_tx = None;
        // Send Final to frontend (chunked path -- emit_final not called)
        if let Some(session) = sessions.get(session_id) {
            session.send_to_host(to_ws(&ServerMsg::Final {
                transcript: transcript.to_string(),
                utterance_id: uid,
            }));
        }
        // Source-language passthrough: queue full host audio once
        if let Some(session) = sessions.get(session_id) {
            if session.active_langs().contains(source_lang) {
                if let Some(ref mgr) = session.rtmp_manager {
                    let mgr = mgr.clone();
                    let lang_str = source_lang.to_string();
                    let mut pcm = host_audio.clone();
                    let max_bytes = ((host_audio.len() as f64 / BYTES_PER_SEC + 2.0) * BYTES_PER_SEC) as usize;
                    if pcm.len() > max_bytes {
                        crate::ffmpeg::truncate_with_fadeout(&mut pcm, max_bytes);
                    }
                    tokio::spawn(async move {
                        let locked = mgr.lock().await;
                        locked.queue_audio(&lang_str, pcm, start);
                    });
                }
            }
        }
    } else {
        // No progressive chunks -- use legacy emit_final path
        state.utterance_counter += 1;
        let uid = state.utterance_counter;
        info!("[FINAL #{}] {}", uid, transcript);
        emit_final(
            sessions, session_id, transcript, uid,
            source_lang, Some(sp.clone()),
            start, host_audio.clone(),
        );
    }

    // Reset progressive state for next utterance
    state.progressive.reset();
    state.chunk_index = 0;
    state.last_audio_byte_sent = 0;
    state.chunk_detector.reset();

    // Adaptive endpointing: track WPM over first 5 finals
    if !state.adapted && prosody.speaking_rate_wpm > 0 && prosody.speaking_rate_wpm <= 500 {
        state.wpm_samples.push(prosody.speaking_rate_wpm);
        if state.wpm_samples.len() >= 5 {
            let avg_wpm = state.wpm_samples.iter().sum::<u32>() as f32
                / state.wpm_samples.len() as f32;
            let (label, new_endp, new_max_dur) =
                crate::stt::classify_speaking_speed(avg_wpm);
            state.adapted = true;

            if label != "normal" {
                info!(
                    "[ADAPTIVE] Avg WPM: {:.0}, classified: {}, reconnecting (endpointing={:.2}s, max_dur={:.0}s)",
                    avg_wpm, label, new_endp, new_max_dur
                );
                state.adaptive_params = Some((new_endp, new_max_dur));
                state.needs_adaptive_reconnect = true;
                state.disconnected = true;
                // Send stop_recording to end session cleanly
                let mut s = sink.lock().await;
                let _ = s.send(tungstenite::Message::Text(
                    r#"{"type":"stop_recording"}"#.to_string().into()
                )).await;
            } else {
                info!(
                    "[ADAPTIVE] Avg WPM: {:.0}, classified: {}, keeping defaults",
                    avg_wpm, label
                );
            }
        }
    }
}

/// Handle an interim (partial) transcript from Gladia.
///
/// Sends the interim to the frontend, runs progressive chunk detection,
/// and spawns chunked pipelines as needed.
async fn handle_interim_transcript(
    state: &mut SttState,
    transcript: &str,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    acc_rx: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
) {
    if state.utterance_start.is_none() {
        state.utterance_start = Some(Instant::now());
    }
    let start = state.utterance_start.unwrap();
    info!("[INTERIM] {}", transcript);

    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(to_ws(&ServerMsg::Interim {
            transcript: transcript.to_string(),
        }));
    }

    // Progressive chunk detection: emit sub-utterance chunks
    // Capture context BEFORE check() -- check() overwrites prev_chunk_text
    let pre_check_context = state.progressive.context().map(|s| s.to_string());
    if let Some(boundary) = state.progressive.check(transcript) {
        let ctx = pre_check_context;

        // Spawn chunked pipeline on first chunk
        if state.chunk_pipeline_tx.is_none() {
            state.utterance_counter += 1; // Pre-increment utterance ID for this utterance
            let sp = StyleParams::default();
            state.chunk_pipeline_tx = spawn_chunked_pipeline(
                sessions, session_id, state.utterance_counter,
                source_lang, sp,
            );
        }

        if let Some(ref tx) = state.chunk_pipeline_tx {
            // Extract incremental host audio for this chunk (non-overlapping)
            let chunk_audio: Vec<u8> = {
                let acc = acc_rx.lock().unwrap();
                let total: Vec<u8> = acc.iter().flatten().cloned().collect();
                let ratio = if transcript.is_empty() { 0.0 }
                    else { boundary.split_pos as f64 / transcript.len() as f64 };
                let end_pos = ((total.len() as f64 * ratio) as usize) & !1;
                let start_pos = state.last_audio_byte_sent.min(end_pos);
                state.last_audio_byte_sent = end_pos;
                total[start_pos..end_pos.min(total.len())].to_vec()
            };

            if let Err(e) = tx.try_send(crate::types::ChunkEvent {
                text: boundary.chunk_text,
                chunk_index: state.chunk_index,
                context: ctx,
                is_utterance_final: false,
                utterance_id: state.utterance_counter,
                utterance_start: start,
                host_audio: chunk_audio,
            }) {
                error!("[CHUNK] #{}.{} dropped (channel full): {}", state.utterance_counter, state.chunk_index, e);
            }
            state.chunk_index += 1;
        }
    }

    // Legacy detector still tracked for metrics
    state.chunk_detector.check(transcript);
}

/// Process a single parsed Gladia message and return a control-flow signal.
async fn process_gladia_message(
    gm: crate::stt::GladiaMessage,
    state: &mut SttState,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    acc_rx: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    sink: &Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
) -> MessageAction {
    if !sessions.contains_key(session_id) {
        return MessageAction::Break;
    }

    // Handle errors from any message
    if let Some(ref err) = gm.error {
        error!("[STT] Gladia error {}: {}", err.status_code, err.message);
        state.disconnected = true;
        return MessageAction::Break;
    }

    match gm.msg_type.as_str() {
        "transcript" => {
            let transcript = match gm.transcript() {
                Some(t) => t,
                None => return MessageAction::Continue,
            };

            if gm.is_final() {
                handle_final_transcript(
                    state, &transcript, sessions, session_id,
                    source_lang, acc_rx, sink,
                ).await;
                // If adaptive reconnect was triggered, break
                if state.needs_adaptive_reconnect {
                    return MessageAction::Break;
                }
            } else {
                handle_interim_transcript(
                    state, &transcript, sessions, session_id,
                    source_lang, acc_rx,
                ).await;
            }
        }
        "speech_start" => {
            debug!("[STT] VAD: speech started");
        }
        "speech_end" => {
            debug!("[STT] VAD: speech ended");
        }
        _ => {}
    }

    MessageAction::Continue
}

pub async fn start_stt(
    session_id: String,
    sessions: Sessions,
    source_lang: Lang,
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let audio_rx = Arc::new(tokio::sync::Mutex::new(audio_rx));
    let mut utterance_counter: u64 = 0;
    let mut reconnect_count: u32 = 0;

    // Shared buffer: accumulate host audio chunks for source-language passthrough
    let audio_acc: Arc<std::sync::Mutex<Vec<Vec<u8>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    // Adaptive endpointing parameters (Gladia uses seconds)
    let mut endpointing: f64 = 0.25;
    let mut max_duration: f64 = 5.0;
    let mut wpm_samples: Vec<u32> = Vec::new();
    let mut adapted = false;

    let reconnect_delay = Duration::from_secs(STT_RECONNECT_DELAY_SECS);

    if STT_API_KEY.is_empty() {
        error!("[STT] STT_API_KEY not set, STT disabled");
        return;
    }

    loop {
        // ── Connect to Gladia ──────────────────────────────
        let ws_stream = match connect_gladia(
            &session_id, &sessions, &source_lang,
            endpointing, max_duration, reconnect_count,
        ).await {
            Some(s) => s,
            None => return,
        };

        let (stt_sink, mut stt_stream) = ws_stream.split();
        let stt_sink = Arc::new(tokio::sync::Mutex::new(stt_sink));

        let sink_for_ctrl = stt_sink.clone();

        // Task 1: Forward host audio -> Gladia + accumulate for passthrough
        let acc_tx = audio_acc.clone();
        let audio_rx_clone = audio_rx.clone();
        let sink_for_audio = stt_sink.clone();
        let sid_audio = session_id.clone();
        let send_task = tokio::spawn(async move {
            let mut rx = audio_rx_clone.lock().await;
            let mut chunk_count: u64 = 0;
            let mut total_bytes: u64 = 0;
            while let Some(data) = rx.recv().await {
                chunk_count += 1;
                total_bytes += data.len() as u64;
                if chunk_count % 100 == 0 {
                    debug!(
                        "[STT:{}] forwarded {} audio chunks ({}KB total) to Gladia",
                        sid_audio, chunk_count, total_bytes / 1024
                    );
                }
                if let Ok(mut acc) = acc_tx.lock() {
                    acc.push(data.clone());
                }
                let mut sink = sink_for_audio.lock().await;
                if sink.send(tungstenite::Message::Binary(data.into())).await.is_err() {
                    error!("[STT:{}] Gladia sink write error, stopping audio forward", sid_audio);
                    break;
                }
            }
            // Send stop_recording on shutdown
            info!("[STT:{}] sending stop_recording to Gladia (total: {} chunks, {}KB)", sid_audio, chunk_count, total_bytes / 1024);
            let mut sink = sink_for_audio.lock().await;
            let _ = sink.send(tungstenite::Message::Text(
                r#"{"type":"stop_recording"}"#.to_string().into()
            )).await;
        });

        // Task 2: Read Gladia responses, extract prosody/emotion, emit events
        let sessions_ref = sessions.clone();
        let sid = session_id.clone();
        let source_lang_clone = source_lang.clone();
        let acc_rx = audio_acc.clone();

        let mut state = SttState {
            utterance_counter,
            utterance_start: None,
            chunk_detector: crate::stt::get_detector(&source_lang.to_string()),
            progressive: crate::stt::ProgressiveChunkDetector::new(&source_lang.to_string()),
            chunk_index: 0,
            chunk_pipeline_tx: None,
            last_audio_byte_sent: 0,
            wpm_samples: wpm_samples.clone(),
            adapted,
            disconnected: false,
            needs_adaptive_reconnect: false,
            adaptive_params: None,
        };

        let recv_task = tokio::spawn(async move {
            while let Some(msg_result) = stt_stream.next().await {
                let msg = match msg_result {
                    Ok(m) => m,
                    Err(e) => {
                        error!("[STT] read error: {}", e);
                        state.disconnected = true;
                        break;
                    }
                };

                let text = match msg {
                    tungstenite::Message::Text(t) => t.to_string(),
                    tungstenite::Message::Close(_) => break,
                    _ => continue,
                };

                let gm: crate::stt::GladiaMessage = match serde_json::from_str(&text) {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                match process_gladia_message(
                    gm, &mut state, &sessions_ref, &sid,
                    &source_lang_clone, &acc_rx, &sink_for_ctrl,
                ).await {
                    MessageAction::Continue => {}
                    MessageAction::Break => break,
                }
            }
            state
        });

        let send_abort = send_task.abort_handle();
        let recv_abort = recv_task.abort_handle();
        tokio::select! {
            _ = send_task => {
                recv_abort.abort();
            },
            result = recv_task => {
                send_abort.abort();
                if let Ok(st) = result {
                    utterance_counter = st.utterance_counter;
                    wpm_samples = st.wpm_samples;
                    adapted = st.adapted;

                    // Check for adaptive reconnect
                    if st.needs_adaptive_reconnect {
                        if let Some((new_endp, new_max_dur)) = st.adaptive_params {
                            endpointing = new_endp;
                            max_duration = new_max_dur;
                        }
                        if let Ok(mut acc) = audio_acc.lock() {
                            acc.clear();
                        }
                        continue;
                    }

                    if !st.disconnected {
                        break;
                    }
                }
            },
        }

        if !sessions.contains_key(&session_id) {
            break;
        }

        reconnect_count += 1;
        if reconnect_count > STT_RECONNECT_MAX {
            error!("[STT] Exceeded max reconnects, giving up");
            break;
        }
        // Clear stale audio from the accumulator to prevent it contaminating
        // the next utterance after reconnect
        if let Ok(mut acc) = audio_acc.lock() {
            acc.clear();
        }
        tokio::time::sleep(reconnect_delay).await;
    }
}
