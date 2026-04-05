//! Voice cloning via ElevenLabs — clone, persist, delete.

pub mod persistence;
pub mod elevenlabs;

use std::time::Instant;
use tracing::{info, warn};
use crate::constants::BYTES_PER_SEC;

// Re-export for existing callers
pub use persistence::load_persisted_voice;
pub use elevenlabs::delete_voice as delete_cloned_voice;

/// Abstraction over voice cloning providers.
/// Implementations: `ElevenLabsCloner` (current).
pub trait VoiceCloner: Send + Sync {
    fn clone_voice(&self, wav: Vec<u8>) -> impl std::future::Future<Output = Result<String, String>> + Send;
    fn delete_voice(&self, voice_id: &str) -> impl std::future::Future<Output = ()> + Send;
    fn cleanup_old_voices(&self) -> impl std::future::Future<Output = ()> + Send;
}

/// Clone voice via ElevenLabs IVC (standalone — no session required).
pub async fn clone_voice_standalone(pcm: Vec<u8>) -> Result<String, String> {
    clone_with(&elevenlabs::ElevenLabsCloner, pcm).await
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

fn encode_pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    let wav = crate::audio::wav::pcm_to_wav(pcm);
    info!(
        "[VOICE_CLONE] starting ElevenLabs IVC: {}B PCM -> {}B WAV ({:.1}s audio)",
        pcm.len(), wav.len(), pcm.len() as f64 / BYTES_PER_SEC
    );
    wav
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
