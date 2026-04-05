use std::time::{Duration, Instant};
use tracing::{info, error, debug};

use crate::constants::BYTES_PER_SEC;
use crate::tts::StyleParams;
use crate::types::{Lang, Sessions, ServerMsg};

use super::to_ws;

// ── Translation Pipeline (legacy full-utterance path) ───

pub(super) async fn run_pipeline(
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
            if let Some(manager) = rtmp_mgr
                && !host_audio.is_empty() {
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
