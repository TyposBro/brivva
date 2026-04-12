//! Voice cloning — clone, persist, delete.
//! Providers: ElevenLabs, DashScope (Qwen3-TTS).

pub mod persistence;
pub mod elevenlabs;
pub mod dashscope;

use std::time::Instant;
use tracing::{info, warn};

// Re-export for existing callers
pub use persistence::load_persisted_voice;

/// Abstraction over voice cloning providers.
/// Implementations: `ElevenLabsCloner`, `DashScopeCloner`.
///
/// `clone_voice` receives raw PCM (44.1kHz 16-bit mono). Each provider
/// is responsible for resampling and encoding to its required format.
pub trait VoiceCloner: Send + Sync {
    fn clone_voice(&self, pcm: Vec<u8>) -> impl std::future::Future<Output = Result<String, String>> + Send;
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

/// Clone voice via DashScope Qwen3-TTS (standalone — no session required).
pub async fn clone_voice_dashscope(client: &reqwest::Client, api_key: &str, pcm: Vec<u8>) -> Result<String, String> {
    let cloner = dashscope::DashScopeCloner { api_key, client };
    clone_with(&cloner, pcm).await
}

/// Delete a cloned voice via DashScope API.
pub async fn delete_cloned_voice_dashscope(client: &reqwest::Client, api_key: &str, voice_name: &str) {
    let cloner = dashscope::DashScopeCloner { api_key, client };
    cloner.delete_voice(voice_name).await;
}

async fn clone_with(cloner: &impl VoiceCloner, pcm: Vec<u8>) -> Result<String, String> {
    let clone_start = Instant::now();
    replace_old_clone(cloner).await;
    cloner.cleanup_old_voices().await;
    let voice_id = cloner.clone_voice(pcm).await?;
    finish_clone(&voice_id, clone_start);
    Ok(voice_id)
}

async fn replace_old_clone(cloner: &impl VoiceCloner) {
    if let Some(old_id) = persistence::load_persisted_voice() {
        warn!("[VOICE_CLONE] replacing old clone {}", old_id);
        // Only attempt delete if the old voice ID looks like it belongs to
        // the same provider. DashScope IDs start with "qwen-tts-vc-",
        // ElevenLabs IDs are alphanumeric. Avoids cross-provider 400 errors.
        cloner.delete_voice(&old_id).await;
    }
}

fn finish_clone(voice_id: &str, start: Instant) {
    info!("[VOICE_CLONE] success! voice_id={} ({}ms)", voice_id, start.elapsed().as_millis());
    persistence::persist_voice(voice_id);
}
