use crate::features::broadcast::domain::{Lang, LiveSessionHandle, ServerMsg};
use futures_util::StreamExt;
use std::time::{Duration, Instant};

use super::to_ws;

/// All inputs to the TTS broadcast path bundled so the public entry point
/// stays within the §3.3 arg budget.
pub struct TtsRequest {
    pub text: String,
    pub utterance_id: u64,
    pub target_lang: Lang,
    pub handle: LiveSessionHandle,
    pub selected_voice_id: Option<String>,
}

pub async fn broadcast_translated_tts(req: TtsRequest) {
    let tts_start = Instant::now();
    let tts_deadline = Duration::from_secs(5);

    let Some((api_key, base_url, force_default_voice)) =
        req.handle.sessions.get(&req.handle.id).map(|s| {
            (
                s.pipeline_config.elevenlabs_api_key.clone(),
                s.pipeline_config.elevenlabs_base_url.clone(),
                s.pipeline_config.force_default_voice,
            )
        })
    else {
        return;
    };

    // Kill-switch: BRIVVA_FALLBACK_TO_DEFAULT_VOICE=1 skips the cloned voice
    // regardless of whether the session bundle supplied one. See runbook.
    let is_cloned = req.selected_voice_id.is_some() && !force_default_voice;
    let voice_id = if is_cloned {
        req.selected_voice_id
            .clone()
            .unwrap_or_else(|| req.target_lang.voice_id().to_string())
    } else {
        req.target_lang.voice_id().to_string()
    };
    let model_id = if is_cloned {
        "eleven_multilingual_v2"
    } else {
        "eleven_flash_v2_5"
    };
    let url = format!(
        "{}/v1/text-to-speech/{}/stream?output_format=mp3_44100_128",
        base_url, &voice_id
    );

    let audio_buffer = match fetch_tts_audio(FetchTtsArgs {
        url: &url,
        api_key: &api_key,
        text: &req.text,
        model_id,
        lang: &req.target_lang,
        deadline: tts_deadline,
    })
    .await
    {
        Some(buf) => buf,
        None => {
            eprintln!(
                "[TTS] no audio for utterance {} lang={}",
                req.utterance_id, req.target_lang
            );
            return;
        }
    };

    let tts_ms = tts_start.elapsed().as_millis() as u64;
    push_tts_into_rtmp(&req.target_lang, &req.handle, &audio_buffer).await;
    notify_host_tts_complete(NotifyCompleteArgs {
        lang: &req.target_lang,
        utterance_id: req.utterance_id,
        tts_ms,
        handle: &req.handle,
    });
}

struct FetchTtsArgs<'a> {
    url: &'a str,
    api_key: &'a str,
    text: &'a str,
    model_id: &'a str,
    lang: &'a Lang,
    deadline: Duration,
}

async fn fetch_tts_audio(args: FetchTtsArgs<'_>) -> Option<Vec<u8>> {
    let FetchTtsArgs {
        url,
        api_key,
        text,
        model_id,
        lang,
        deadline,
    } = args;
    let client = reqwest::Client::new();
    let body = serde_json::json!({ "text": text, "model_id": model_id });
    let tts_result = tokio::time::timeout(deadline, async {
        let mut audio_buffer = Vec::new();
        let response = client
            .post(url)
            .header("xi-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                let mut stream = resp.bytes_stream();
                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => audio_buffer.extend_from_slice(&chunk),
                        Err(error) => {
                            eprintln!("TTS stream error for {}: {}", lang, error);
                            break;
                        }
                    }
                }
            }
            Ok(resp) => eprintln!("TTS error: {} - {:?}", resp.status(), resp.text().await),
            Err(error) => eprintln!("TTS request error for {}: {}", lang, error),
        }

        audio_buffer
    })
    .await;

    match tts_result {
        Ok(buffer) if !buffer.is_empty() => Some(buffer),
        Ok(_) => None,
        Err(_) => {
            eprintln!("[TTS] TIMEOUT lang={} (>{:?})", lang, deadline);
            None
        }
    }
}

async fn push_tts_into_rtmp(lang: &Lang, handle: &LiveSessionHandle, audio_buffer: &[u8]) {
    let rtmp_manager = handle
        .sessions
        .get(&handle.id)
        .and_then(|session| session.rtmp_manager.clone());
    let Some(manager) = rtmp_manager else {
        return;
    };

    match crate::features::broadcast::data::ffmpeg::decode_mp3_to_pcm(audio_buffer).await {
        Ok(pcm) => {
            let manager = manager.lock().await;
            manager.push_tts(&lang.to_string(), pcm);
        }
        Err(error) => eprintln!("[RTMP] MP3→PCM decode failed: {}", error),
    }
}

struct NotifyCompleteArgs<'a> {
    lang: &'a Lang,
    utterance_id: u64,
    tts_ms: u64,
    handle: &'a LiveSessionHandle,
}

fn notify_host_tts_complete(args: NotifyCompleteArgs<'_>) {
    let Some(live_session) = args.handle.sessions.get(&args.handle.id) else {
        return;
    };

    live_session.send_to_host(to_ws(&ServerMsg::TtsEnd {
        utterance_id: args.utterance_id,
        target_lang: args.lang.to_string(),
        tts_ms: args.tts_ms,
    }));
    live_session.send_to_host(to_ws(&ServerMsg::VideoEnd {
        utterance_id: args.utterance_id,
    }));
}
