//! Persistence for voice clone IDs.

use tracing::{info, error};
use crate::core::config::{VOICE_CLONE_FILE, VOICE_CLONE_FILE_DASHSCOPE};

/// Load persisted voice for a specific provider.
pub fn load_persisted_voice_for(provider: &str) -> Option<String> {
    load_from_file(file_for_provider(provider))
}

/// Load persisted voice (legacy — checks ElevenLabs file).
pub fn load_persisted_voice() -> Option<String> {
    load_from_file(VOICE_CLONE_FILE)
}

/// Persist voice for a specific provider.
pub fn persist_voice_for(provider: &str, voice_id: &str) {
    persist_to_file(file_for_provider(provider), voice_id);
}

/// Persist voice (legacy — writes ElevenLabs file).
pub fn persist_voice(voice_id: &str) {
    persist_to_file(VOICE_CLONE_FILE, voice_id);
}

pub fn file_for_provider_pub(provider: &str) -> &str {
    file_for_provider(provider)
}

fn file_for_provider(provider: &str) -> &str {
    match provider {
        "dashscope" => VOICE_CLONE_FILE_DASHSCOPE,
        _ => VOICE_CLONE_FILE,
    }
}

fn load_from_file(path: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(id) => {
            let id = id.trim().to_string();
            if id.is_empty() { return None; }
            info!("[VOICE_CLONE] loaded persisted voice: {}", id);
            Some(id)
        }
        Err(_) => None,
    }
}

fn persist_to_file(path: &str, voice_id: &str) {
    if let Err(e) = std::fs::write(path, voice_id) {
        error!("[VOICE_CLONE] failed to persist voice_id: {}", e);
    } else {
        info!("[VOICE_CLONE] persisted voice_id={}", voice_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::sync::Mutex;

    // Serialize tests that change the working directory.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    /// Run `body` with the current directory set to a fresh temp dir,
    /// restoring the original directory afterward regardless of outcome.
    fn with_temp_cwd<F: FnOnce()>(body: F) {
        let _guard = CWD_LOCK.lock().unwrap();
        let original = env::current_dir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        env::set_current_dir(tmp.path()).unwrap();

        body();

        env::set_current_dir(original).unwrap();
    }

    #[test]
    fn should_return_none_when_file_does_not_exist() {
        with_temp_cwd(|| {
            let result = load_persisted_voice();

            assert!(result.is_none());
        });
    }

    #[test]
    fn should_persist_then_load_voice_id() {
        with_temp_cwd(|| {
            persist_voice("voice-abc-123");

            let loaded = load_persisted_voice();

            assert_eq!(loaded, Some("voice-abc-123".to_string()));
        });
    }

    #[test]
    fn should_return_none_when_file_is_empty() {
        with_temp_cwd(|| {
            fs::write(VOICE_CLONE_FILE, "").unwrap();

            let loaded = load_persisted_voice();

            assert!(loaded.is_none());
        });
    }

    #[test]
    fn should_return_none_when_file_is_only_whitespace() {
        with_temp_cwd(|| {
            fs::write(VOICE_CLONE_FILE, "   \n  ").unwrap();

            let loaded = load_persisted_voice();

            assert!(loaded.is_none());
        });
    }

    #[test]
    fn should_trim_whitespace_from_loaded_voice_id() {
        with_temp_cwd(|| {
            fs::write(VOICE_CLONE_FILE, "  voice-xyz  \n").unwrap();

            let loaded = load_persisted_voice();

            assert_eq!(loaded, Some("voice-xyz".to_string()));
        });
    }

    #[test]
    fn should_overwrite_previous_voice_id() {
        with_temp_cwd(|| {
            persist_voice("first-voice");
            persist_voice("second-voice");

            let loaded = load_persisted_voice();

            assert_eq!(loaded, Some("second-voice".to_string()));
        });
    }
}
