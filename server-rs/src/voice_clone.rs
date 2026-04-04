//! Voice cloning via ElevenLabs — clone, persist, delete.

use serde::Deserialize;
use std::time::Instant;
use tracing::{info, warn, error};

use crate::constants::{BYTES_PER_SEC, VOICE_CLONE_FILE};
use crate::tts::TTS_API_KEY;

// ── Voice Cloning ────────────────────────────────────────

fn pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    let sample_rate: u32 = 44100;
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

/// Load persisted voice clone ID from disk (if any).
pub fn load_persisted_voice() -> Option<String> {
    match std::fs::read_to_string(VOICE_CLONE_FILE) {
        Ok(id) => {
            let id = id.trim().to_string();
            if id.is_empty() { return None; }
            info!("[VOICE_CLONE] loaded persisted voice: {}", id);
            Some(id)
        }
        Err(_) => None,
    }
}

/// Save voice clone ID to disk for persistence across sessions.
fn persist_voice(voice_id: &str) {
    if let Err(e) = std::fs::write(VOICE_CLONE_FILE, voice_id) {
        error!("[VOICE_CLONE] failed to persist voice_id: {}", e);
    } else {
        info!("[VOICE_CLONE] persisted voice_id={}", voice_id);
    }
}

/// Clean up old brivva voices from ElevenLabs to free up voice slots.
async fn cleanup_old_brivva_voices() {
    let client = &*crate::HTTP_CLIENT;
    let resp = match client
        .get("https://api.elevenlabs.io/v1/voices")
        .header("xi-api-key", &*TTS_API_KEY)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            error!("[VOICE_CLONE] list voices failed: {}", r.status());
            return;
        }
        Err(e) => {
            error!("[VOICE_CLONE] list voices error: {}", e);
            return;
        }
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
                delete_cloned_voice(&v.voice_id).await;
            }
        }
    }
}

/// Clone voice via ElevenLabs IVC (standalone — no session required).
/// Returns the voice_id on success. Cleans up old brivva voices first.
pub async fn clone_voice_standalone(pcm: Vec<u8>) -> Result<String, String> {
    let clone_start = Instant::now();
    let wav = pcm_to_wav(&pcm);
    info!(
        "[VOICE_CLONE] starting ElevenLabs IVC: {}B PCM -> {}B WAV ({:.1}s audio)",
        pcm.len(), wav.len(), pcm.len() as f64 / BYTES_PER_SEC
    );

    // Clean up old clones first
    if let Some(old_id) = load_persisted_voice() {
        warn!("[VOICE_CLONE] replacing old clone {}", old_id);
        delete_cloned_voice(&old_id).await;
    }
    cleanup_old_brivva_voices().await;

    let client = &*crate::HTTP_CLIENT;
    let form = reqwest::multipart::Form::new()
        .text("name", "brivva-clone".to_string())
        .part(
            "files",
            reqwest::multipart::Part::bytes(wav)
                .file_name("voice_sample.wav")
                .mime_str("audio/wav")
                .unwrap(),
        );

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

    info!("[VOICE_CLONE] ElevenLabs success! voice_id={} ({}ms)", parsed.voice_id, clone_start.elapsed().as_millis());
    persist_voice(&parsed.voice_id);
    Ok(parsed.voice_id)
}

/// Delete a cloned voice from ElevenLabs.
pub async fn delete_cloned_voice(voice_id: &str) {
    let client = &*crate::HTTP_CLIENT;
    let url = format!("https://api.elevenlabs.io/v1/voices/{}", voice_id);
    match client
        .delete(&url)
        .header("xi-api-key", &*TTS_API_KEY)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => info!("[VOICE_CLONE] deleted {}", voice_id),
        Ok(r) => error!("[VOICE_CLONE] delete error: {}", r.status()),
        Err(e) => error!("[VOICE_CLONE] delete error: {}", e),
    }
}
