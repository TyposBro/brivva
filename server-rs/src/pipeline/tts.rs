use std::time::{Duration, Instant};
use tracing::{info, error, debug};

use crate::constants::BYTES_PER_SEC;
use crate::stt::config::PASSTHROUGH_PADDING_SECS;
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
    let (voice_clone_id, tts_model) = read_session_config(sessions, session_id);

    log_pipeline_start(utterance_id, transcript, target_langs, &voice_clone_id, utterance_start);

    let handles = spawn_all_lang_tasks(
        transcript, utterance_id, source_lang, target_langs,
        sessions, session_id, &voice_clone_id, &tts_model,
        style_params, tier, utterance_start, utterance_end, &host_audio,
    );

    await_all(handles).await;

    info!(
        "[PIPELINE] #{} all langs done (total {}ms since pipeline start, {}ms since utterance start)",
        utterance_id, pipeline_start.elapsed().as_millis(), utterance_start.elapsed().as_millis()
    );
}

// ── Spawning ───

fn spawn_all_lang_tasks(
    transcript: &str,
    utterance_id: u64,
    source_lang: &Lang,
    target_langs: &[Lang],
    sessions: &Sessions,
    session_id: &str,
    voice_clone_id: &Option<String>,
    tts_model: &str,
    style_params: &StyleParams,
    tier: u8,
    utterance_start: Instant,
    utterance_end: Instant,
    host_audio: &[u8],
) -> Vec<tokio::task::JoinHandle<()>> {
    let client = &*crate::HTTP_CLIENT;
    let mut handles = Vec::new();

    for lang in target_langs {
        if lang == source_lang {
            if let Some(h) = spawn_source_passthrough(
                sessions, session_id, lang, host_audio,
                utterance_start, utterance_end,
            ) {
                handles.push(h);
            }
            continue;
        }

        handles.push(spawn_translate_and_tts(
            transcript, utterance_id, source_lang, lang,
            client.clone(), sessions.clone(), session_id,
            voice_clone_id.clone(), tts_model, style_params,
            tier, utterance_start, utterance_end,
        ));
    }

    handles
}

fn spawn_source_passthrough(
    sessions: &Sessions,
    session_id: &str,
    lang: &Lang,
    host_audio: &[u8],
    utterance_start: Instant,
    utterance_end: Instant,
) -> Option<tokio::task::JoinHandle<()>> {
    let rtmp_mgr = sessions.get(session_id).and_then(|s| s.rtmp_manager.clone());

    if let Some(manager) = rtmp_mgr
        && !host_audio.is_empty()
    {
        let mut pcm = host_audio.to_vec();
        let pcm_len = pcm.len();
        let lang_str = lang.to_string();
        let mgr = manager.clone();

        Some(tokio::spawn(async move {
            truncate_if_too_long(&mut pcm, utterance_start, utterance_end);
            let locked = mgr.lock().await;
            locked.queue_audio(&lang_str, pcm, utterance_start);
            debug!("[PASSTHROUGH] Queued host audio for {} ({}KB)", lang_str, pcm_len / 1024);
        }))
    } else {
        None
    }
}

fn spawn_translate_and_tts(
    transcript: &str,
    utterance_id: u64,
    source_lang: &Lang,
    target_lang: &Lang,
    client: reqwest::Client,
    sessions: Sessions,
    session_id: &str,
    voice_clone_id: Option<String>,
    tts_model: &str,
    style_params: &StyleParams,
    tier: u8,
    utterance_start: Instant,
    utterance_end: Instant,
) -> tokio::task::JoinHandle<()> {
    let transcript = transcript.to_string();
    let source = source_lang.clone();
    let target = target_lang.clone();
    let session_id = session_id.to_string();
    let sp = style_params.clone();
    let tts_model = tts_model.to_string();

    tokio::spawn(async move {
        let step_start = Instant::now();
        log_translate_start(utterance_id, &source, &target, &transcript);

        let (translated_text, translate_ms) = match translate(&transcript, &source, &target, utterance_id).await {
            Some(result) => result,
            None => return,
        };

        info!("[TRANSLATE] {} -> {} = '{}' ({}ms)", source, target, translated_text, translate_ms);
        send_translation_to_host(&sessions, &session_id, &target, &translated_text, utterance_id, translate_ms);

        run_tts_if_eligible(
            tier, &client, &translated_text, utterance_id, &target,
            &sessions, &session_id, voice_clone_id.as_deref(), &sp,
            utterance_start, utterance_end, &tts_model, step_start, translate_ms,
        ).await;
    })
}

// ── Helpers ───

fn read_session_config(sessions: &Sessions, session_id: &str) -> (Option<String>, String) {
    match sessions.get(session_id) {
        Some(s) => (s.voice_clone_id.clone(), s.tts_model.clone()),
        None => (None, crate::constants::DEFAULT_TTS_MODEL.to_string()),
    }
}

fn truncate_if_too_long(pcm: &mut Vec<u8>, utterance_start: Instant, utterance_end: Instant) {
    let utterance_dur = utterance_end.duration_since(utterance_start);
    let max_dur = utterance_dur + Duration::from_secs_f64(PASSTHROUGH_PADDING_SECS);
    let max_bytes = (max_dur.as_secs_f64() * BYTES_PER_SEC) as usize;
    if pcm.len() > max_bytes {
        crate::ffmpeg::truncate_with_fadeout(pcm, max_bytes);
    }
}

async fn translate(transcript: &str, source: &Lang, target: &Lang, utterance_id: u64) -> Option<(String, u64)> {
    match crate::translation::translate(transcript, None, source, target).await {
        Ok((t, ms)) => Some((t, ms)),
        Err(e) => {
            error!("[TRANSLATE] #{} {}: {}", utterance_id, target, e);
            None
        }
    }
}

fn send_translation_to_host(
    sessions: &Sessions,
    session_id: &str,
    target: &Lang,
    translated_text: &str,
    utterance_id: u64,
    translate_ms: u64,
) {
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(to_ws(&ServerMsg::Translation {
            lang: target.to_string(),
            text: translated_text.to_string(),
            utterance_id,
            translate_ms,
        }));
    }
}

async fn run_tts_if_eligible(
    tier: u8,
    client: &reqwest::Client,
    translated_text: &str,
    utterance_id: u64,
    target: &Lang,
    sessions: &Sessions,
    session_id: &str,
    voice_clone_id: Option<&str>,
    style_params: &StyleParams,
    utterance_start: Instant,
    utterance_end: Instant,
    tts_model: &str,
    step_start: Instant,
    translate_ms: u64,
) {
    if tier >= 2 {
        debug!(
            "[PIPELINE] #{} {} starting TTS (translate took {}ms, total pipeline elapsed {}ms)",
            utterance_id, target, translate_ms, step_start.elapsed().as_millis()
        );
        crate::tts::do_tts(
            client, translated_text, utterance_id, target,
            sessions, session_id, voice_clone_id, style_params,
            utterance_start, utterance_end, tts_model,
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
}

fn log_pipeline_start(
    utterance_id: u64,
    transcript: &str,
    target_langs: &[Lang],
    voice_clone_id: &Option<String>,
    utterance_start: Instant,
) {
    info!(
        "[PIPELINE] #{} starting: '{}' -> {:?} (voice_clone={}) delay_since_utterance_start={}ms",
        utterance_id, &transcript[..transcript.len().min(60)],
        target_langs.iter().map(|l| l.to_string()).collect::<Vec<_>>(),
        voice_clone_id.as_deref().unwrap_or("none"),
        utterance_start.elapsed().as_millis()
    );
}

fn log_translate_start(utterance_id: u64, source: &Lang, target: &Lang, transcript: &str) {
    debug!(
        "[TRANSLATE] #{} {} -> {}: '{}' ({} chars)",
        utterance_id, source, target, &transcript[..transcript.len().min(80)], transcript.len()
    );
}

async fn await_all(handles: Vec<tokio::task::JoinHandle<()>>) {
    for handle in handles {
        let _ = handle.await;
    }
}
