//! Voice cloning via ElevenLabs — clone, persist, delete.

pub mod persistence;
pub mod elevenlabs;

use std::time::Instant;
use tracing::{info, warn};
use crate::core::config::{BYTES_PER_SEC, SAMPLE_RATE};

// Re-export for existing callers
pub use persistence::load_persisted_voice;

/// Abstraction over voice cloning providers.
/// Implementations: `ElevenLabsCloner` (current).
pub trait VoiceCloner: Send + Sync {
    fn clone_voice(&self, wav: Vec<u8>) -> impl std::future::Future<Output = Result<String, String>> + Send;
    fn delete_voice(&self, voice_id: &str) -> impl std::future::Future<Output = ()> + Send;
    fn cleanup_old_voices(&self) -> impl std::future::Future<Output = ()> + Send;
}

/// Clone voice via ElevenLabs IVC (standalone — no session required).
pub async fn clone_voice_standalone(client: &reqwest::Client, api_key: &str, pcm: Vec<u8>) -> Result<String, String> {
    let cloner = elevenlabs::ElevenLabsCloner { api_key, client };
    clone_with(&cloner, pcm).await
}

/// Delete a cloned voice via ElevenLabs API.
pub async fn delete_cloned_voice(client: &reqwest::Client, api_key: &str, voice_id: &str) {
    elevenlabs::delete_voice(client, api_key, voice_id).await;
}

async fn clone_with(cloner: &impl VoiceCloner, pcm: Vec<u8>) -> Result<String, String> {
    let clone_start = Instant::now();
    let wav = encode_pcm_to_wav(&pcm);
    replace_old_clone(cloner).await;
    cloner.cleanup_old_voices().await;
    let voice_id = cloner.clone_voice(wav).await?;
    finish_clone(&voice_id, clone_start);
    Ok(voice_id)
}

const CLONE_SAMPLE_RATE: u32 = SAMPLE_RATE / 2; // 22050Hz — halves file size, well under ElevenLabs 11MB limit

fn encode_pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    let duration_secs = pcm.len() as f64 / BYTES_PER_SEC;
    let downsampled = downsample_2x(pcm);
    let wav = crate::core::audio::pcm_to_wav_at(&downsampled, CLONE_SAMPLE_RATE);
    info!(
        "[VOICE_CLONE] starting ElevenLabs IVC: {}B PCM -> {}B WAV ({:.1}s audio, {}Hz)",
        pcm.len(), wav.len(), duration_secs, CLONE_SAMPLE_RATE,
    );
    wav
}

fn downsample_2x(pcm: &[u8]) -> Vec<u8> {
    let samples: &[u8] = pcm;
    let sample_count = samples.len() / 2;
    let pair_count = sample_count / 2;
    let mut out = Vec::with_capacity(pair_count * 2);
    for i in 0..pair_count {
        let offset = i * 4;
        let s0 = i16::from_le_bytes([samples[offset], samples[offset + 1]]);
        let s1 = i16::from_le_bytes([samples[offset + 2], samples[offset + 3]]);
        let avg = ((s0 as i32 + s1 as i32) / 2) as i16;
        out.extend_from_slice(&avg.to_le_bytes());
    }
    out
}

async fn replace_old_clone(cloner: &impl VoiceCloner) {
    if let Some(old_id) = persistence::load_persisted_voice() {
        warn!("[VOICE_CLONE] replacing old clone {}", old_id);
        cloner.delete_voice(&old_id).await;
    }
}

fn finish_clone(voice_id: &str, start: Instant) {
    info!("[VOICE_CLONE] success! voice_id={} ({}ms)", voice_id, start.elapsed().as_millis());
    persistence::persist_voice(voice_id);
}
