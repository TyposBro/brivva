//! ElevenLabs voice cloning API calls.

use serde::Deserialize;
use tracing::{info, warn, error};
use crate::tts::TTS_API_KEY;

pub async fn clone_voice(wav: Vec<u8>) -> Result<String, String> {
    let client = &*crate::HTTP_CLIENT;
    let form = reqwest::multipart::Form::new()
        .text("name", "brivva-clone".to_string())
        .part("files", reqwest::multipart::Part::bytes(wav)
            .file_name("voice_sample.wav")
            .mime_str("audio/wav")
            .unwrap());

    let resp = client
        .post("https://api.elevenlabs.io/v1/voices/add")
        .header("xi-api-key", &*TTS_API_KEY)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("request error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        error!("[VOICE_CLONE] ElevenLabs error {}: {}", status, body);
        return Err(format!("ElevenLabs {}: {}", status, body));
    }

    #[derive(Deserialize)]
    struct CloneResp { voice_id: String }
    let parsed = resp.json::<CloneResp>().await
        .map_err(|e| format!("parse error: {}", e))?;
    Ok(parsed.voice_id)
}

pub async fn delete_voice(voice_id: &str) {
    let client = &*crate::HTTP_CLIENT;
    let url = format!("https://api.elevenlabs.io/v1/voices/{}", voice_id);
    match client.delete(&url).header("xi-api-key", &*TTS_API_KEY).send().await {
        Ok(r) if r.status().is_success() => info!("[VOICE_CLONE] deleted {}", voice_id),
        Ok(r) => error!("[VOICE_CLONE] delete error: {}", r.status()),
        Err(e) => error!("[VOICE_CLONE] delete error: {}", e),
    }
}

pub async fn cleanup_old_voices() {
    let client = &*crate::HTTP_CLIENT;
    let resp = match client
        .get("https://api.elevenlabs.io/v1/voices")
        .header("xi-api-key", &*TTS_API_KEY)
        .send().await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => { error!("[VOICE_CLONE] list voices failed: {}", r.status()); return; }
        Err(e) => { error!("[VOICE_CLONE] list voices error: {}", e); return; }
    };

    #[derive(Deserialize)]
    struct Voice { voice_id: String, name: String }
    #[derive(Deserialize)]
    struct VoiceList { voices: Vec<Voice> }

    if let Ok(list) = resp.json::<VoiceList>().await {
        let brivva_voices: Vec<_> = list.voices.iter()
            .filter(|v| v.name.starts_with("brivva-"))
            .collect();
        if !brivva_voices.is_empty() {
            warn!("[VOICE_CLONE] cleaning up {} old brivva voice(s)", brivva_voices.len());
            for v in &brivva_voices {
                warn!("[VOICE_CLONE] deleting old voice {} ({})", v.voice_id, v.name);
                delete_voice(&v.voice_id).await;
            }
        }
    }
}
