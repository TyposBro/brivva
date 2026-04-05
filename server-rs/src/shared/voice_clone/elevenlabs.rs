//! ElevenLabs voice cloning API calls.

use serde::Deserialize;
use tracing::{info, warn, error};

/// Dependencies needed by the ElevenLabs cloner.
pub struct ElevenLabsCloner<'a> {
    pub api_key: &'a str,
    pub client: &'a reqwest::Client,
}

impl<'a> super::VoiceCloner for ElevenLabsCloner<'a> {
    async fn clone_voice(&self, wav: Vec<u8>) -> Result<String, String> {
        let form = build_clone_form(wav);
        let resp = send_clone_request(self.client, self.api_key, form).await?;
        parse_clone_response(resp).await
    }

    async fn delete_voice(&self, voice_id: &str) {
        delete_voice_by_id(self.client, self.api_key, voice_id).await;
    }

    async fn cleanup_old_voices(&self) {
        cleanup_brivva_voices(self.client, self.api_key).await;
    }
}

// ── Public facade ───────────────────────────────────────────────────────────

pub async fn clone_voice(client: &reqwest::Client, api_key: &str, wav: Vec<u8>) -> Result<String, String> {
    use super::VoiceCloner;
    ElevenLabsCloner { api_key, client }.clone_voice(wav).await
}

pub async fn delete_voice(client: &reqwest::Client, api_key: &str, voice_id: &str) {
    delete_voice_by_id(client, api_key, voice_id).await;
}

pub async fn cleanup_old_voices(client: &reqwest::Client, api_key: &str) {
    cleanup_brivva_voices(client, api_key).await;
}

// ── Internal helpers ────────────────────────────────────────────────────────

async fn delete_voice_by_id(client: &reqwest::Client, api_key: &str, voice_id: &str) {
    let url = format!("https://api.elevenlabs.io/v1/voices/{}", voice_id);
    match client.delete(&url).header("xi-api-key", api_key).send().await {
        Ok(r) if r.status().is_success() => info!("[VOICE_CLONE] deleted {}", voice_id),
        Ok(r) => error!("[VOICE_CLONE] delete error: {}", r.status()),
        Err(e) => error!("[VOICE_CLONE] delete error: {}", e),
    }
}

async fn cleanup_brivva_voices(client: &reqwest::Client, api_key: &str) {
    let voices = match list_all_voices(client, api_key).await {
        Some(v) => v,
        None => return,
    };
    delete_brivva_voices(client, api_key, &voices).await;
}

// ── clone_voice helpers ─────────────────────────────────────────────────────

fn build_clone_form(wav: Vec<u8>) -> reqwest::multipart::Form {
    reqwest::multipart::Form::new()
        .text("name", "brivva-clone".to_string())
        .part("files", reqwest::multipart::Part::bytes(wav)
            .file_name("voice_sample.wav")
            .mime_str("audio/wav")
            .unwrap())
}

async fn send_clone_request(client: &reqwest::Client, api_key: &str, form: reqwest::multipart::Form) -> Result<reqwest::Response, String> {
    client
        .post("https://api.elevenlabs.io/v1/voices/add")
        .header("xi-api-key", api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("request error: {}", e))
}

async fn parse_clone_response(resp: reqwest::Response) -> Result<String, String> {
    if !resp.status().is_success() {
        return Err(format_clone_error(resp).await);
    }

    #[derive(Deserialize)]
    struct CloneResp { voice_id: String }
    resp.json::<CloneResp>().await
        .map(|r| r.voice_id)
        .map_err(|e| format!("parse error: {}", e))
}

async fn format_clone_error(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    error!("[VOICE_CLONE] ElevenLabs error {}: {}", status, body);
    format!("ElevenLabs {}: {}", status, body)
}

// ── cleanup_old_voices helpers ──────────────────────────────────────────────

#[derive(Deserialize)]
struct Voice { voice_id: String, name: String }

#[derive(Deserialize)]
struct VoiceList { voices: Vec<Voice> }

async fn list_all_voices(client: &reqwest::Client, api_key: &str) -> Option<Vec<Voice>> {
    let resp = match client
        .get("https://api.elevenlabs.io/v1/voices")
        .header("xi-api-key", api_key)
        .send().await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => { error!("[VOICE_CLONE] list voices failed: {}", r.status()); return None; }
        Err(e) => { error!("[VOICE_CLONE] list voices error: {}", e); return None; }
    };

    resp.json::<VoiceList>().await.ok().map(|l| l.voices)
}

async fn delete_brivva_voices(client: &reqwest::Client, api_key: &str, voices: &[Voice]) {
    let brivva: Vec<_> = voices.iter()
        .filter(|v| v.name.starts_with("brivva-"))
        .collect();

    if brivva.is_empty() { return; }

    warn!("[VOICE_CLONE] cleaning up {} old brivva voice(s)", brivva.len());
    for v in &brivva {
        warn!("[VOICE_CLONE] deleting old voice {} ({})", v.voice_id, v.name);
        delete_voice_by_id(client, api_key, &v.voice_id).await;
    }
}
