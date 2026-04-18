use crate::features::broadcast::domain::{LiveSessions, ServerMsg};
use serde::Deserialize;
use std::sync::LazyLock;

use super::to_ws;

static ELEVENLABS_API_KEY: LazyLock<String> =
    LazyLock::new(|| std::env::var("ELEVENLABS_API_KEY").unwrap_or_default());

pub async fn clone_voice(pcm: Vec<u8>, live_sessions: &LiveSessions, live_session_id: &str) {
    let wav = pcm_to_wav(&pcm);
    let session_short = &live_session_id[..6.min(live_session_id.len())];
    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .text("name", format!("brivva-host-{}", session_short))
        .part(
            "files",
            reqwest::multipart::Part::bytes(wav)
                .file_name("host_voice.wav")
                .mime_str("audio/wav")
                .unwrap(),
        );

    let response = client
        .post("https://api.elevenlabs.io/v1/voices/add")
        .header("xi-api-key", &*ELEVENLABS_API_KEY)
        .multipart(form)
        .send()
        .await;

    handle_clone_response(response, live_sessions, live_session_id).await;
}

pub async fn delete_cloned_voice(voice_id: &str) {
    let client = reqwest::Client::new();
    let url = format!("https://api.elevenlabs.io/v1/voices/{}", voice_id);
    match client
        .delete(&url)
        .header("xi-api-key", &*ELEVENLABS_API_KEY)
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            println!("[VOICE_CLONE] deleted cloned voice {}", voice_id);
        }
        Ok(response) => eprintln!(
            "[VOICE_CLONE] delete error {}: {:?}",
            response.status(),
            response.text().await
        ),
        Err(error) => eprintln!("[VOICE_CLONE] delete request error: {}", error),
    }
}

async fn handle_clone_response(
    response: Result<reqwest::Response, reqwest::Error>,
    live_sessions: &LiveSessions,
    live_session_id: &str,
) {
    match response {
        Ok(resp) if resp.status().is_success() => {
            apply_cloned_voice(resp, live_sessions, live_session_id).await;
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            eprintln!("[VOICE_CLONE] error {}: {}", status, body);
            mark_clone_failed(
                live_sessions,
                live_session_id,
                format!("Voice clone failed: {}", status),
            );
        }
        Err(error) => {
            eprintln!("[VOICE_CLONE] request error: {}", error);
            mark_clone_failed(
                live_sessions,
                live_session_id,
                format!("Voice clone error: {}", error),
            );
        }
    }
}

async fn apply_cloned_voice(
    response: reqwest::Response,
    live_sessions: &LiveSessions,
    live_session_id: &str,
) {
    #[derive(Deserialize)]
    struct CloneResp {
        voice_id: String,
    }

    match response.json::<CloneResp>().await {
        Ok(parsed) => {
            if let Some(mut live_session) = live_sessions.get_mut(live_session_id) {
                live_session.selected_voice_id = Some(parsed.voice_id.clone());
                live_session.ephemeral_voice_id = Some(parsed.voice_id.clone());
                live_session.clone_in_progress = false;
                live_session.send_to_host(to_ws(&ServerMsg::VoiceReady {
                    voice_id: parsed.voice_id,
                }));
            } else {
                delete_cloned_voice(&parsed.voice_id).await;
            }
        }
        Err(error) => {
            eprintln!("[VOICE_CLONE] parse error: {}", error);
            clear_clone_in_progress(live_sessions, live_session_id);
        }
    }
}

fn mark_clone_failed(
    live_sessions: &LiveSessions,
    live_session_id: &str,
    message: String,
) {
    let Some(mut live_session) = live_sessions.get_mut(live_session_id) else {
        return;
    };

    live_session.clone_in_progress = false;
    live_session.send_to_host(to_ws(&ServerMsg::Error { message }));
}

fn clear_clone_in_progress(live_sessions: &LiveSessions, live_session_id: &str) {
    if let Some(mut live_session) = live_sessions.get_mut(live_session_id) {
        live_session.clone_in_progress = false;
    }
}

fn pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    let sample_rate: u32 = 44_100;
    let bits_per_sample: u16 = 16;
    let channels: u16 = 1;
    let byte_rate = sample_rate * (bits_per_sample as u32 / 8) * channels as u32;
    let block_align = channels * (bits_per_sample / 8);
    let data_size = pcm.len() as u32;

    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}
