use crate::features::broadcast::domain::{Lang, LiveSessions, ServerMsg};
use futures_util::StreamExt;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use super::to_ws;

static ELEVENLABS_API_KEY: LazyLock<String> =
    LazyLock::new(|| std::env::var("ELEVENLABS_API_KEY").unwrap_or_default());

const ELEVENLABS_BASE_URL_DEFAULT: &str = "https://api.elevenlabs.io";

static ELEVENLABS_BASE_URL: LazyLock<String> = LazyLock::new(|| {
    std::env::var("ELEVENLABS_BASE_URL")
        .unwrap_or_else(|_| ELEVENLABS_BASE_URL_DEFAULT.to_string())
        .trim_end_matches('/')
        .to_string()
});

pub async fn broadcast_translated_tts(
    text: &str,
    utterance_id: u64,
    lang: &Lang,
    live_sessions: &LiveSessions,
    live_session_id: &str,
    selected_voice_id: Option<&str>,
) {
    let tts_start = Instant::now();
    let tts_deadline = Duration::from_secs(5);

    let is_cloned = selected_voice_id.is_some();
    let voice_id = selected_voice_id
        .map(str::to_string)
        .unwrap_or_else(|| lang.voice_id().to_string());
    let model_id = if is_cloned {
        "eleven_multilingual_v2"
    } else {
        "eleven_flash_v2_5"
    };
    let url = format!(
        "{}/v1/text-to-speech/{}/stream?output_format=mp3_44100_128",
        &*ELEVENLABS_BASE_URL, &voice_id
    );

    let client = reqwest::Client::new();
    let body = serde_json::json!({ "text": text, "model_id": model_id });
    let tts_result = tokio::time::timeout(tts_deadline, async {
        let mut audio_buffer = Vec::new();
        let response = client
            .post(&url)
            .header("xi-api-key", &*ELEVENLABS_API_KEY)
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

    let audio_buffer = match tts_result {
        Ok(buffer) if !buffer.is_empty() => buffer,
        Ok(_) => return,
        Err(_) => {
            eprintln!(
                "[TTS] TIMEOUT utterance {} lang={} — dropped to silence (>{:?})",
                utterance_id, lang, tts_deadline
            );
            return;
        }
    };

    let tts_ms = tts_start.elapsed().as_millis() as u64;
    push_tts_into_rtmp(lang, live_sessions, live_session_id, &audio_buffer).await;
    notify_host_tts_complete(lang, utterance_id, tts_ms, live_sessions, live_session_id);
}

async fn push_tts_into_rtmp(
    lang: &Lang,
    live_sessions: &LiveSessions,
    live_session_id: &str,
    audio_buffer: &[u8],
) {
    let rtmp_manager = live_sessions
        .get(live_session_id)
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

fn notify_host_tts_complete(
    lang: &Lang,
    utterance_id: u64,
    tts_ms: u64,
    live_sessions: &LiveSessions,
    live_session_id: &str,
) {
    let Some(live_session) = live_sessions.get(live_session_id) else {
        return;
    };

    live_session.send_to_host(to_ws(&ServerMsg::TtsEnd {
        utterance_id,
        target_lang: lang.to_string(),
        tts_ms,
    }));
    live_session.send_to_host(to_ws(&ServerMsg::VideoEnd { utterance_id }));
}
