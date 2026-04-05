//! Voice cloning via ElevenLabs — clone, persist, delete.

pub mod persistence;
pub mod elevenlabs;

use std::time::Instant;
use tracing::{info, warn};
use crate::constants::BYTES_PER_SEC;

// Re-export for existing callers
pub use persistence::load_persisted_voice;
pub use elevenlabs::delete_voice as delete_cloned_voice;

/// Clone voice via ElevenLabs IVC (standalone — no session required).
pub async fn clone_voice_standalone(pcm: Vec<u8>) -> Result<String, String> {
    let clone_start = Instant::now();
    let wav = crate::audio::wav::pcm_to_wav(&pcm);
    info!(
        "[VOICE_CLONE] starting ElevenLabs IVC: {}B PCM -> {}B WAV ({:.1}s audio)",
        pcm.len(), wav.len(), pcm.len() as f64 / BYTES_PER_SEC
    );

    if let Some(old_id) = persistence::load_persisted_voice() {
        warn!("[VOICE_CLONE] replacing old clone {}", old_id);
        elevenlabs::delete_voice(&old_id).await;
    }
    elevenlabs::cleanup_old_voices().await;

    let voice_id = elevenlabs::clone_voice(wav).await?;
    info!("[VOICE_CLONE] success! voice_id={} ({}ms)", voice_id, clone_start.elapsed().as_millis());
    persistence::persist_voice(&voice_id);
    Ok(voice_id)
}
