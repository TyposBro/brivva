use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{info, error, debug};

use crate::constants::{
    BYTES_PER_SEC, CHUNK_PIPELINE_CAPACITY,
    TTS_DEADLINE_CAP_MS, TTS_DEADLINE_MARGIN_MS,
};
use crate::tts::{StyleParams, DEFAULT_VOICE};
use crate::types::{Lang, Sessions, ServerMsg};

use super::to_ws;

/// Spawn a chunked pipeline that processes sub-utterance chunks via an mpsc channel.
/// One StreamingPcm per (utterance, language) — multiple TTS chunks feed the same buffer.
pub(super) fn spawn_chunked_pipeline(
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
                    if tier >= 2
                        && let Some(ref s) = streaming {
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

                            let tts_deadline = std::time::Duration::from_millis(
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
