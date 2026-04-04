//! Translation pipeline: STT → Translate → TTS → RTMP
//!
//! Host audio flows through:
//! 1. Gladia Solaria-1 STT (direct WebSocket)
//! 2. Google Cloud Translation API v2 — per target language, parallel
//! 3. ElevenLabs TTS — streaming PCM output (with optional cloned voice)
//! 4. PCM truncate+fadeout → queue to RTMP manager for synced playback
//! 5. Source-language passthrough: host audio queued directly to RTMP (no TTS)

use axum::extract::ws::Message;
use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tracing::{info, error, debug};

use crate::constants::{
    BYTES_PER_SEC, CHUNK_PIPELINE_CAPACITY, STT_RECONNECT_MAX, STT_RECONNECT_DELAY_SECS,
    TTS_DEADLINE_CAP_MS, TTS_DEADLINE_MARGIN_MS,
};
use crate::tts::{StyleParams, DEFAULT_VOICE};
use crate::types::{Lang, Sessions, ServerMsg};

// ── Service Keys ──────────────────────────────────────────

/// STT provider API key (currently: Gladia)
static STT_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("STT_API_KEY").unwrap_or_default()
});

// ── STT Connection (Direct Gladia) ──────────────────────

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

/// Spawn a chunked pipeline that processes sub-utterance chunks via an mpsc channel.
/// One StreamingPcm per (utterance, language) — multiple TTS chunks feed the same buffer.
fn spawn_chunked_pipeline(
    sessions: &Sessions,
    session_id: &str,
    utterance_id: u64,
    source_lang: &Lang,
    style_params: StyleParams,
) -> Option<mpsc::Sender<crate::types::ChunkEvent>> {
    let session = sessions.get(session_id)?;
    let active = session.active_langs();
    let tier = session.tier;
    let voice_clone_id = session.voice_clone_id.clone();
    let tts_model = session.tts_model.clone();
    let broadcast_delay_ms = session.broadcast_delay_ms;
    drop(session); // release DashMap ref

    if active.is_empty() {
        return None;
    }

    let (chunk_tx, mut chunk_rx) = mpsc::channel::<crate::types::ChunkEvent>(CHUNK_PIPELINE_CAPACITY);
    let sessions = sessions.clone();
    let session_id = session_id.to_string();
    let source_lang = source_lang.clone();

    tokio::spawn(async move {
        // Per-language streaming PCM handles (created on first chunk)
        let mut lang_streaming: std::collections::HashMap<
            String, crate::ffmpeg::StreamingPcm
        > = std::collections::HashMap::new();

        let pipeline_start = Instant::now();
        let mut chunk_count: u16 = 0;

        while let Some(chunk) = chunk_rx.recv().await {
            chunk_count += 1;
            let chunk_text = chunk.text.clone();
            let chunk_idx = chunk.chunk_index;
            let is_final = chunk.is_utterance_final;
            let context = chunk.context.clone();
            let utterance_start = chunk.utterance_start;
            let _host_audio = chunk.host_audio;

            debug!(
                "[CHUNK] #{}.{} text='{}' final={} context={}",
                utterance_id, chunk_idx,
                &chunk_text[..chunk_text.len().min(60)],
                is_final,
                context.as_ref().map(|c| c.len()).unwrap_or(0)
            );

            // Process each language in parallel
            let mut handles = Vec::new();

            for lang in &active {
                let lang_str = lang.to_string();

                if lang == &source_lang {
                    // Source-language passthrough is handled by the FINAL handler
                    // via emit_final (which queues the complete host audio once).
                    // Don't queue partial chunk audio here — it would create
                    // multiple overlapping QueuedAudio entries.
                    continue;
                }

                // Create StreamingPcm on first chunk for this language
                if chunk_idx == 0 {
                    let rtmp_mgr = sessions.get(&session_id).and_then(|s| s.rtmp_manager.clone());
                    if let Some(manager) = rtmp_mgr {
                        let mgr = manager.lock().await;
                        let streaming = mgr.queue_streaming_audio(&lang_str, utterance_start);
                        lang_streaming.insert(lang_str.clone(), streaming);
                        debug!("[CHUNK] #{}.0 {} created StreamingPcm slot", utterance_id, lang_str);
                    }

                    // Notify host: TTS started
                    if let Some(session) = sessions.get(&session_id) {
                        session.send_to_host(to_ws(&ServerMsg::TtsStart {
                            lang: lang_str.clone(), utterance_id,
                        }));
                    }
                }

                let text = chunk_text.clone();
                let ctx = context.clone();
                let source = source_lang.clone();
                let target = lang.clone();
                let sessions_c = sessions.clone();
                let session_id_c = session_id.clone();
                let voice_clone = voice_clone_id.clone();
                let sp = style_params.clone();
                let tts_model_c = tts_model.clone();
                let streaming = lang_streaming.get(&lang_str).cloned();

                // Note: no explicit backpressure — the per-chunk TTS deadline
                // (broadcast_delay - 500ms) already prevents runaway generation.
                // If TTS times out, the StreamingPcm just gets less data and the
                // audio drain writes silence for the remainder.

                handles.push(tokio::spawn(async move {
                    // Translate with context
                    let ctx_ref = ctx.as_deref();
                    let (translated_text, translate_ms) = match crate::translation::translate(
                        &text, ctx_ref, &source, &target,
                    ).await {
                        Ok((t, ms)) => (t, ms),
                        Err(e) => {
                            error!("[TRANSLATE] chunk #{}.{} {}: {}", utterance_id, chunk_idx, target, e);
                            return;
                        }
                    };

                    debug!(
                        "[TRANSLATE] chunk #{}.{} {} = '{}' ({}ms)",
                        utterance_id, chunk_idx, target, translated_text, translate_ms
                    );

                    // Send chunk translation to frontend
                    if let Some(session) = sessions_c.get(&session_id_c) {
                        session.send_to_host(to_ws(&ServerMsg::ChunkTranslation {
                            lang: target.to_string(),
                            text: translated_text.clone(),
                            utterance_id,
                            chunk_index: chunk_idx,
                            translate_ms,
                        }));
                    }

                    // TTS (tier 2+ only)
                    if tier >= 2 {
                        if let Some(ref s) = streaming {
                            let voice_id = voice_clone.as_deref()
                                .unwrap_or(&*DEFAULT_VOICE);
                            let lang_str = target.to_string();

                            let (stability, similarity_boost, style, _) =
                                crate::stt::map_style(&sp.emotion);
                            let voice_settings = serde_json::json!({
                                "stability": stability,
                                "similarity_boost": similarity_boost,
                                "style": style,
                                "speed": sp.speed,
                            });

                            // Max bytes is for the ENTIRE utterance (all chunks share one
                            // StreamingPcm). Use broadcast_delay + margin as total budget.
                            let max_secs = (broadcast_delay_ms as f64 / 1000.0) + 5.0;
                            let max_bytes = (max_secs * BYTES_PER_SEC) as usize;

                            let tts_deadline = Duration::from_millis(
                                broadcast_delay_ms.saturating_sub(TTS_DEADLINE_MARGIN_MS)
                                    .min(TTS_DEADLINE_CAP_MS)
                            );

                            match tokio::time::timeout(tts_deadline, crate::tts::do_tts_ws(
                                &translated_text, voice_id, &lang_str,
                                &voice_settings, max_bytes, Some(s), &tts_model_c,
                            )).await {
                                Ok(Ok(bytes)) => {
                                    debug!(
                                        "[TTS] chunk #{}.{} {} = {}KB PCM",
                                        utterance_id, chunk_idx, target, bytes / 1024
                                    );
                                }
                                Ok(Err(e)) => {
                                    error!("[TTS] chunk #{}.{} {} error: {}", utterance_id, chunk_idx, target, e);
                                }
                                Err(_) => {
                                    error!("[TTS] chunk #{}.{} {} TIMEOUT", utterance_id, chunk_idx, target);
                                }
                            }
                        }
                    }
                }));
            }

            // Wait for all languages to finish this chunk before processing next
            for h in handles {
                let _ = h.await;
            }
        }

        // Mark all streaming buffers as complete
        for (lang_str, streaming) in &lang_streaming {
            streaming.finish();
            // Notify host: TTS ended
            if let Some(session) = sessions.get(&session_id) {
                session.send_to_host(to_ws(&ServerMsg::TtsEnd {
                    lang: lang_str.clone(),
                    utterance_id,
                    tts_ms: pipeline_start.elapsed().as_millis() as u64,
                }));
            }
        }

        info!(
            "[CHUNK] #{} pipeline complete: {} chunks, {}ms total",
            utterance_id, chunk_count, pipeline_start.elapsed().as_millis()
        );
    });

    Some(chunk_tx)
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
        let max_attempts = if reconnect_count == 0 { 10 } else { STT_RECONNECT_MAX };
        let mut ws_stream = None;

        for attempt in 1..=max_attempts {
            if !sessions.contains_key(&session_id) {
                info!("[STT] Session {} gone, stopping", session_id);
                return;
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
                    return;
                }
            };

            match tokio_tungstenite::connect_async(request).await {
                Ok((stream, _)) => {
                    info!(
                        "[STT] Connected to Gladia Solaria-1 (attempt {}, endpointing={:.2}s, max_dur={:.0}s, session={})",
                        attempt, endpointing, max_duration, gladia_session.id
                    );
                    ws_stream = Some(stream);
                    break;
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

        let ws_stream = match ws_stream {
            Some(s) => s,
            None => {
                error!("[STT] Failed to connect after {} attempts", max_attempts);
                return;
            }
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
        let mut uc = utterance_counter;
        let mut utterance_start: Option<Instant> = None;
        let mut chunk_detector = crate::stt::get_detector(&source_lang.to_string());
        let mut progressive = crate::stt::ProgressiveChunkDetector::new(&source_lang.to_string());
        let mut chunk_index: u16 = 0;
        let mut chunk_pipeline_tx: Option<mpsc::Sender<crate::types::ChunkEvent>> = None;
        let mut last_audio_byte_sent: usize = 0; // Track audio position for incremental extraction

        let disconnected_unexpectedly = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let disc_flag = disconnected_unexpectedly.clone();

        let mut local_wpm_samples = wpm_samples.clone();
        let mut local_adapted = adapted;
        let needs_adaptive_reconnect = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let adaptive_flag = needs_adaptive_reconnect.clone();
        let adaptive_endpointing = Arc::new(std::sync::Mutex::new(None::<(f64, f64)>));
        let adaptive_params = adaptive_endpointing.clone();

        let recv_task = tokio::spawn(async move {
            while let Some(msg_result) = stt_stream.next().await {
                let msg = match msg_result {
                    Ok(m) => m,
                    Err(e) => {
                        error!("[STT] read error: {}", e);
                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
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

                if !sessions_ref.contains_key(&sid) {
                    break;
                }

                // Handle errors from any message
                if let Some(ref err) = gm.error {
                    error!("[STT] Gladia error {}: {}", err.status_code, err.message);
                    disc_flag.store(true, std::sync::atomic::Ordering::Release);
                    break;
                }

                match gm.msg_type.as_str() {
                    "transcript" => {
                        let transcript = match gm.transcript() {
                            Some(t) => t,
                            None => continue,
                        };

                        if gm.is_final() {
                            // -- Final event --
                            let start = utterance_start.take().unwrap_or_else(Instant::now);
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
                            if let Some(ref tx) = chunk_pipeline_tx {
                                // Use the same utterance ID that was assigned when the
                                // chunked pipeline was spawned (don't increment uc again)
                                let uid = uc;
                                info!("[FINAL #{}] {} (chunked, {} prior chunks)", uid, transcript, chunk_index);

                                let flush_context = progressive.context().map(|s| s.to_string());
                                if let Some(boundary) = progressive.flush(&transcript) {
                                    if let Err(e) = tx.try_send(crate::types::ChunkEvent {
                                        text: boundary.chunk_text,
                                        chunk_index,
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
                                chunk_pipeline_tx = None;
                                // Send Final to frontend (chunked path -- emit_final not called)
                                if let Some(session) = sessions_ref.get(&sid) {
                                    session.send_to_host(to_ws(&ServerMsg::Final {
                                        transcript: transcript.clone(),
                                        utterance_id: uid,
                                    }));
                                }
                                // Source-language passthrough: queue full host audio once
                                if let Some(session) = sessions_ref.get(&sid) {
                                    if session.active_langs().contains(&source_lang_clone) {
                                        if let Some(ref mgr) = session.rtmp_manager {
                                            let mgr = mgr.clone();
                                            let lang_str = source_lang_clone.to_string();
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
                                uc += 1;
                                let uid = uc;
                                info!("[FINAL #{}] {}", uid, transcript);
                                emit_final(
                                    &sessions_ref, &sid, &transcript, uid,
                                    &source_lang_clone, Some(sp.clone()),
                                    start, host_audio.clone(),
                                );
                            }

                            // Reset progressive state for next utterance
                            progressive.reset();
                            chunk_index = 0;
                            last_audio_byte_sent = 0;
                            chunk_detector.reset();

                            // Adaptive endpointing: track WPM over first 5 finals
                            if !local_adapted && prosody.speaking_rate_wpm > 0 && prosody.speaking_rate_wpm <= 500 {
                                local_wpm_samples.push(prosody.speaking_rate_wpm);
                                if local_wpm_samples.len() >= 5 {
                                    let avg_wpm = local_wpm_samples.iter().sum::<u32>() as f32
                                        / local_wpm_samples.len() as f32;
                                    let (label, new_endp, new_max_dur) =
                                        crate::stt::classify_speaking_speed(avg_wpm);
                                    local_adapted = true;

                                    if label != "normal" {
                                        info!(
                                            "[ADAPTIVE] Avg WPM: {:.0}, classified: {}, reconnecting (endpointing={:.2}s, max_dur={:.0}s)",
                                            avg_wpm, label, new_endp, new_max_dur
                                        );
                                        if let Ok(mut params) = adaptive_params.lock() {
                                            *params = Some((new_endp, new_max_dur));
                                        }
                                        adaptive_flag.store(true, std::sync::atomic::Ordering::Release);
                                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                                        // Send stop_recording to end session cleanly
                                        let mut sink = sink_for_ctrl.lock().await;
                                        let _ = sink.send(tungstenite::Message::Text(
                                            r#"{"type":"stop_recording"}"#.to_string().into()
                                        )).await;
                                        break;
                                    } else {
                                        info!(
                                            "[ADAPTIVE] Avg WPM: {:.0}, classified: {}, keeping defaults",
                                            avg_wpm, label
                                        );
                                    }
                                }
                            }
                        } else {
                            // -- Interim (partial) event --
                            if utterance_start.is_none() {
                                utterance_start = Some(Instant::now());
                            }
                            let start = utterance_start.unwrap();
                            info!("[INTERIM] {}", transcript);

                            if let Some(session) = sessions_ref.get(&sid) {
                                session.send_to_host(to_ws(&ServerMsg::Interim {
                                    transcript: transcript.clone(),
                                }));
                            }

                            // Progressive chunk detection: emit sub-utterance chunks
                            // Capture context BEFORE check() -- check() overwrites prev_chunk_text
                            let pre_check_context = progressive.context().map(|s| s.to_string());
                            if let Some(boundary) = progressive.check(&transcript) {
                                let ctx = pre_check_context;

                                // Spawn chunked pipeline on first chunk
                                if chunk_pipeline_tx.is_none() {
                                    uc += 1; // Pre-increment utterance ID for this utterance
                                    let sp = StyleParams::default();
                                    chunk_pipeline_tx = spawn_chunked_pipeline(
                                        &sessions_ref, &sid, uc,
                                        &source_lang_clone, sp,
                                    );
                                }

                                if let Some(ref tx) = chunk_pipeline_tx {
                                    // Extract incremental host audio for this chunk (non-overlapping)
                                    let chunk_audio: Vec<u8> = {
                                        let acc = acc_rx.lock().unwrap();
                                        let total: Vec<u8> = acc.iter().flatten().cloned().collect();
                                        let ratio = if transcript.is_empty() { 0.0 }
                                            else { boundary.split_pos as f64 / transcript.len() as f64 };
                                        let end_pos = ((total.len() as f64 * ratio) as usize) & !1;
                                        let start_pos = last_audio_byte_sent.min(end_pos);
                                        last_audio_byte_sent = end_pos;
                                        total[start_pos..end_pos.min(total.len())].to_vec()
                                    };

                                    if let Err(e) = tx.try_send(crate::types::ChunkEvent {
                                        text: boundary.chunk_text,
                                        chunk_index,
                                        context: ctx,
                                        is_utterance_final: false,
                                        utterance_id: uc,
                                        utterance_start: start,
                                        host_audio: chunk_audio,
                                    }) {
                                        error!("[CHUNK] #{}.{} dropped (channel full): {}", uc, chunk_index, e);
                                    }
                                    chunk_index += 1;
                                }
                            }

                            // Legacy detector still tracked for metrics
                            chunk_detector.check(&transcript);
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
            }
            (uc, local_wpm_samples, local_adapted)
        });

        let send_abort = send_task.abort_handle();
        let recv_abort = recv_task.abort_handle();
        tokio::select! {
            _ = send_task => {
                recv_abort.abort();
            },
            result = recv_task => {
                send_abort.abort();
                if let Ok((uc, wpm, adapt)) = result {
                    utterance_counter = uc;
                    wpm_samples = wpm;
                    adapted = adapt;
                }
            },
        }

        // Check for adaptive reconnect (creates new Gladia session with adjusted params)
        if needs_adaptive_reconnect.load(std::sync::atomic::Ordering::Acquire) {
            if let Ok(params) = adaptive_endpointing.lock() {
                if let Some((new_endp, new_max_dur)) = *params {
                    endpointing = new_endp;
                    max_duration = new_max_dur;
                }
            }
            if let Ok(mut acc) = audio_acc.lock() {
                acc.clear();
            }
            continue;
        }

        if !disconnected_unexpectedly.load(std::sync::atomic::Ordering::Acquire) {
            break;
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

// ── Translation Pipeline (legacy full-utterance path) ───

async fn run_pipeline(
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
    let client = &*crate::HTTP_CLIENT;
    let mut handles = Vec::new();

    let voice_clone_id = sessions.get(session_id).and_then(|s| s.voice_clone_id.clone());
    let tts_model = sessions.get(session_id).map(|s| s.tts_model.clone()).unwrap_or_else(|| crate::constants::DEFAULT_TTS_MODEL.to_string());
    info!(
        "[PIPELINE] #{} starting: '{}' -> {:?} (voice_clone={}) delay_since_utterance_start={}ms",
        utterance_id, &transcript[..transcript.len().min(60)],
        target_langs.iter().map(|l| l.to_string()).collect::<Vec<_>>(),
        voice_clone_id.as_deref().unwrap_or("none"),
        utterance_start.elapsed().as_millis()
    );

    for lang in target_langs {
        if lang == source_lang {
            // Source-language passthrough: host audio queued directly to RTMP
            let rtmp_mgr = sessions.get(session_id).and_then(|s| s.rtmp_manager.clone());
            if let Some(manager) = rtmp_mgr {
                if !host_audio.is_empty() {
                    let mut pcm = host_audio.clone();
                    let pcm_len = pcm.len();
                    let lang_str = lang.to_string();
                    let mgr = manager.clone();
                    handles.push(tokio::spawn(async move {
                        let utterance_dur = utterance_end.duration_since(utterance_start);
                        let max_dur = utterance_dur + Duration::from_millis(2000);
                        let max_bytes = (max_dur.as_secs_f64() * BYTES_PER_SEC) as usize;
                        if pcm.len() > max_bytes {
                            crate::ffmpeg::truncate_with_fadeout(&mut pcm, max_bytes);
                        }
                        let locked = mgr.lock().await;
                        locked.queue_audio(&lang_str, pcm, utterance_start);
                        debug!(
                            "[PASSTHROUGH] Queued host audio for {} ({}KB)",
                            lang_str, pcm_len / 1024
                        );
                    }));
                }
            }
            continue;
        }

        let transcript = transcript.to_string();
        let source = source_lang.clone();
        let target = lang.clone();
        let client = client.clone(); // reqwest::Client clone is cheap (Arc internally)
        let sessions = sessions.clone();
        let session_id = session_id.to_string();
        let voice_clone_id = voice_clone_id.clone();
        let sp = style_params.clone();
        let tts_model = tts_model.clone();

        handles.push(tokio::spawn(async move {
            // 1. Translate
            let step_start = Instant::now();
            debug!(
                "[TRANSLATE] #{} {} -> {}: '{}' ({} chars)",
                utterance_id, source, target, &transcript[..transcript.len().min(80)], transcript.len()
            );

            let (translated_text, translate_ms) = match crate::translation::translate(
                &transcript, None, &source, &target,
            ).await {
                Ok((t, ms)) => (t, ms),
                Err(e) => {
                    error!("[TRANSLATE] #{} {}: {}", utterance_id, target, e);
                    return;
                }
            };

            info!("[TRANSLATE] {} -> {} = '{}' ({}ms)", source, target, translated_text, translate_ms);

            // 2. Send translation to host
            if let Some(session) = sessions.get(&session_id) {
                session.send_to_host(to_ws(&ServerMsg::Translation {
                    lang: target.to_string(),
                    text: translated_text.clone(),
                    utterance_id,
                    translate_ms,
                }));
            }

            // 3. TTS (only for tier 2+)
            if tier >= 2 {
                debug!(
                    "[PIPELINE] #{} {} starting TTS (translate took {}ms, total pipeline elapsed {}ms)",
                    utterance_id, target, translate_ms, step_start.elapsed().as_millis()
                );
                crate::tts::do_tts(
                    &client, &translated_text, utterance_id, &target,
                    &sessions, &session_id, voice_clone_id.as_deref(), &sp,
                    utterance_start, utterance_end, &tts_model,
                ).await;
                debug!(
                    "[PIPELINE] #{} {} complete (total {}ms since pipeline start)",
                    utterance_id, target, step_start.elapsed().as_millis()
                );
            } else {
                debug!(
                    "[PIPELINE] #{} {} tier={}, skipping TTS",
                    utterance_id, target, tier
                );
            }
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
    info!(
        "[PIPELINE] #{} all langs done (total {}ms since pipeline start, {}ms since utterance start)",
        utterance_id, pipeline_start.elapsed().as_millis(), utterance_start.elapsed().as_millis()
    );
}

pub(crate) fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}

