//! DashScope (Qwen3-TTS) voice cloning API calls.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use tracing::{info, error};

pub const DASHSCOPE_VOICE_CLONE_URL: &str =
    "https://dashscope-intl.aliyuncs.com/api/v1/services/audio/tts/customization";

const CLONE_SAMPLE_RATE: u32 = 24000;
const TARGET_MODEL: &str = "qwen3-tts-vc-realtime-2026-01-15";
const ENROLLMENT_MODEL: &str = "qwen-voice-enrollment";
const PREFERRED_NAME: &str = "brivva_clone";
/// DashScope max reference audio duration in seconds.
const MAX_AUDIO_DURATION_SEC: f64 = 55.0; // 60s limit, leave 5s margin
const INPUT_SAMPLE_RATE: u32 = 44100;
const BYTES_PER_SAMPLE: usize = 2;

/// Dependencies needed by the DashScope cloner.
pub struct DashScopeCloner<'a> {
    pub api_key: &'a str,
    pub client: &'a reqwest::Client,
}

impl<'a> super::VoiceCloner for DashScopeCloner<'a> {
    async fn clone_voice(&self, pcm: Vec<u8>) -> Result<String, String> {
        let pcm = trim_to_max_duration(&pcm);
        let wav = encode_wav_24k(pcm);
        let body = build_create_body(&wav);
        let resp = post_customization(self.client, self.api_key, &body).await?;
        parse_create_response(resp).await
    }

    async fn delete_voice(&self, voice_name: &str) {
        if !voice_name.starts_with("qwen-tts-") {
            info!("[VOICE_CLONE] skipping DashScope delete for non-DashScope voice: {}", voice_name);
            return;
        }
        delete_voice_by_name(self.client, self.api_key, voice_name).await;
    }

    async fn cleanup_old_voices(&self) {
        // DashScope has no list-voices endpoint; nothing to clean up.
    }
}

// ── Audio trimming ──────────────────────────────────────────────────────────

/// Trim PCM to MAX_AUDIO_DURATION_SEC at 44.1kHz input rate.
fn trim_to_max_duration(pcm: &[u8]) -> &[u8] {
    let max_bytes = (MAX_AUDIO_DURATION_SEC * INPUT_SAMPLE_RATE as f64) as usize * BYTES_PER_SAMPLE;
    if pcm.len() > max_bytes {
        tracing::warn!(
            "[VOICE_CLONE] DashScope: trimming {:.1}s to {:.0}s",
            pcm.len() as f64 / (INPUT_SAMPLE_RATE as f64 * BYTES_PER_SAMPLE as f64),
            MAX_AUDIO_DURATION_SEC,
        );
        &pcm[..max_bytes]
    } else {
        pcm
    }
}

// ── WAV encoding ────────────────────────────────────────────────────────────

fn encode_wav_24k(pcm_44k: &[u8]) -> Vec<u8> {
    let resampled = resample_44100_to_24000(pcm_44k);
    let wav = crate::core::audio::pcm_to_wav_at(&resampled, CLONE_SAMPLE_RATE);
    info!(
        "[VOICE_CLONE] DashScope: {}B PCM -> {}B WAV ({}Hz)",
        pcm_44k.len(), wav.len(), CLONE_SAMPLE_RATE,
    );
    wav
}

/// Resample 44100Hz 16-bit mono PCM to 24000Hz via linear interpolation.
fn resample_44100_to_24000(pcm: &[u8]) -> Vec<u8> {
    let src_rate = 44100_f64;
    let dst_rate = 24000_f64;
    let ratio = src_rate / dst_rate;
    let src_samples = pcm.len() / 2;
    let dst_samples = ((src_samples as f64) / ratio) as usize;
    let mut out = Vec::with_capacity(dst_samples * 2);

    for i in 0..dst_samples {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        let frac = src_pos - idx as f64;
        let s0 = read_sample(pcm, idx);
        let s1 = read_sample(pcm, (idx + 1).min(src_samples - 1));
        let interpolated = s0 as f64 + frac * (s1 as f64 - s0 as f64);
        out.extend_from_slice(&(interpolated as i16).to_le_bytes());
    }
    out
}

fn read_sample(pcm: &[u8], idx: usize) -> i16 {
    let offset = idx * 2;
    i16::from_le_bytes([pcm[offset], pcm[offset + 1]])
}

// ── Create voice ────────────────────────────────────────────────────────────

fn build_create_body(wav: &[u8]) -> CreateRequest {
    let b64 = BASE64.encode(wav);
    let data_uri = format!("data:audio/wav;base64,{}", b64);
    CreateRequest {
        model: ENROLLMENT_MODEL.to_string(),
        input: CreateInput {
            action: "create".to_string(),
            target_model: TARGET_MODEL.to_string(),
            preferred_name: PREFERRED_NAME.to_string(),
            audio: AudioData { data: data_uri },
            language: "en".to_string(),
        },
    }
}

async fn post_customization(
    client: &reqwest::Client,
    api_key: &str,
    body: &CreateRequest,
) -> Result<reqwest::Response, String> {
    client
        .post(DASHSCOPE_VOICE_CLONE_URL)
        .bearer_auth(api_key)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("DashScope request error: {}", e))
}

async fn parse_create_response(resp: reqwest::Response) -> Result<String, String> {
    if !resp.status().is_success() {
        return Err(format_error(resp, "create").await);
    }

    resp.json::<CreateResponse>()
        .await
        .map(|r| r.output.voice)
        .map_err(|e| format!("DashScope parse error: {}", e))
}

// ── Delete voice ────────────────────────────────────────────────────────────

async fn delete_voice_by_name(
    client: &reqwest::Client,
    api_key: &str,
    voice_name: &str,
) {
    let body = DeleteRequest {
        model: ENROLLMENT_MODEL.to_string(),
        input: DeleteInput {
            action: "delete".to_string(),
            voice: voice_name.to_string(),
        },
    };

    match client
        .post(DASHSCOPE_VOICE_CLONE_URL)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            info!("[VOICE_CLONE] DashScope deleted {}", voice_name)
        }
        Ok(r) => error!("[VOICE_CLONE] DashScope delete error: {}", r.status()),
        Err(e) => error!("[VOICE_CLONE] DashScope delete error: {}", e),
    }
}

// ── Error formatting ────────────────────────────────────────────────────────

async fn format_error(resp: reqwest::Response, action: &str) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    error!("[VOICE_CLONE] DashScope {} error {}: {}", action, status, body);
    format!("DashScope {}: {}", status, body)
}

// ── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct CreateRequest {
    model: String,
    input: CreateInput,
}

#[derive(Serialize)]
struct CreateInput {
    action: String,
    target_model: String,
    preferred_name: String,
    audio: AudioData,
    language: String,
}

#[derive(Serialize)]
struct AudioData {
    data: String,
}

#[derive(Deserialize)]
struct CreateResponse {
    output: CreateOutput,
}

#[derive(Deserialize)]
struct CreateOutput {
    voice: String,
}

#[derive(Serialize)]
struct DeleteRequest {
    model: String,
    input: DeleteInput,
}

#[derive(Serialize)]
struct DeleteInput {
    action: String,
    voice: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_resample_correct_number_of_samples() {
        // 44100 samples at 44.1kHz = 1 second -> 24000 samples at 24kHz
        let src_samples = 44100;
        let pcm: Vec<u8> = (0..src_samples)
            .flat_map(|_| 0i16.to_le_bytes())
            .collect();

        let result = resample_44100_to_24000(&pcm);

        let dst_samples = result.len() / 2;
        assert_eq!(dst_samples, 24000);
    }

    #[test]
    fn should_preserve_dc_signal_after_resample() {
        let value: i16 = 1000;
        let src_samples = 4410;
        let pcm: Vec<u8> = (0..src_samples)
            .flat_map(|_| value.to_le_bytes())
            .collect();

        let result = resample_44100_to_24000(&pcm);

        // Every output sample should be ~1000 (linear interp of constant = constant)
        for chunk in result.chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            assert_eq!(sample, value);
        }
    }

    #[test]
    fn should_build_create_body_with_base64_audio() {
        let wav = vec![0u8; 44]; // minimal WAV header size

        let body = build_create_body(&wav);

        assert_eq!(body.model, ENROLLMENT_MODEL);
        assert_eq!(body.input.action, "create");
        assert_eq!(body.input.target_model, TARGET_MODEL);
        assert_eq!(body.input.preferred_name, PREFERRED_NAME);
        assert!(body.input.audio.data.starts_with("data:audio/wav;base64,"));
        assert_eq!(body.input.language, "en");
    }

    #[test]
    fn should_read_sample_at_index() {
        let pcm: Vec<u8> = vec![
            0x00, 0x01, // sample 0 = 256
            0xFF, 0x7F, // sample 1 = 32767
        ];

        assert_eq!(read_sample(&pcm, 0), 256);
        assert_eq!(read_sample(&pcm, 1), 32767);
    }
}
