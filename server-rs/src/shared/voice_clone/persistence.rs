//! Persistence for voice clone IDs.

use tracing::{info, error};
use crate::core::config::VOICE_CLONE_FILE;

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

pub fn persist_voice(voice_id: &str) {
    if let Err(e) = std::fs::write(VOICE_CLONE_FILE, voice_id) {
        error!("[VOICE_CLONE] failed to persist voice_id: {}", e);
    } else {
        info!("[VOICE_CLONE] persisted voice_id={}", voice_id);
    }
}
