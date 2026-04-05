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
