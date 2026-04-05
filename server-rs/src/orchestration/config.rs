//! Application configuration — reads ALL environment variables.
//! This is the ONLY place env vars are read (besides dotenvy loading in lib.rs).

use crate::core::config::{DEFAULT_VOICE_ID, SERVER_ADDR, MAX_BODY_SIZE};

/// Typed configuration populated from environment variables at startup.
pub struct AppConfig {
    pub soniox_api_key: String,
    pub tts_api_key: String,
    pub default_voice: String,
    pub server_addr: String,
    pub max_body_size: usize,
}

impl AppConfig {
    /// Read all environment variables once at application bootstrap.
    pub fn from_env() -> Self {
        Self {
            soniox_api_key: std::env::var("SONIOX_API_KEY").unwrap_or_default(),
            tts_api_key: std::env::var("TTS_API_KEY").unwrap_or_default(),
            default_voice: std::env::var("DEFAULT_VOICE")
                .unwrap_or_else(|_| DEFAULT_VOICE_ID.to_string()),
            server_addr: SERVER_ADDR.to_string(),
            max_body_size: MAX_BODY_SIZE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env var tests must run sequentially to avoid races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    unsafe fn set_env(key: &str, val: &str) {
        unsafe { std::env::set_var(key, val); }
    }

    unsafe fn remove_env(key: &str) {
        unsafe { std::env::remove_var(key); }
    }

    #[test]
    fn should_read_soniox_api_key_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { set_env("SONIOX_API_KEY", "test-soniox-key") };

        let config = AppConfig::from_env();

        assert_eq!(config.soniox_api_key, "test-soniox-key");
        unsafe { remove_env("SONIOX_API_KEY") };
    }

    #[test]
    fn should_read_tts_api_key_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { set_env("TTS_API_KEY", "test-tts-key") };

        let config = AppConfig::from_env();

        assert_eq!(config.tts_api_key, "test-tts-key");
        unsafe { remove_env("TTS_API_KEY") };
    }

    #[test]
    fn should_fallback_soniox_key_to_empty_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { remove_env("SONIOX_API_KEY") };

        let config = AppConfig::from_env();

        assert_eq!(config.soniox_api_key, "");
    }

    #[test]
    fn should_fallback_default_voice_to_constant_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { remove_env("DEFAULT_VOICE") };

        let config = AppConfig::from_env();

        assert_eq!(config.default_voice, DEFAULT_VOICE_ID);
    }

    #[test]
    fn should_read_default_voice_from_env_when_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { set_env("DEFAULT_VOICE", "custom-voice-id") };

        let config = AppConfig::from_env();

        assert_eq!(config.default_voice, "custom-voice-id");
        unsafe { remove_env("DEFAULT_VOICE") };
    }

    #[test]
    fn should_use_server_addr_constant() {
        let _guard = ENV_LOCK.lock().unwrap();

        let config = AppConfig::from_env();

        assert_eq!(config.server_addr, SERVER_ADDR);
    }

    #[test]
    fn should_use_max_body_size_constant() {
        let _guard = ENV_LOCK.lock().unwrap();

        let config = AppConfig::from_env();

        assert_eq!(config.max_body_size, MAX_BODY_SIZE);
    }

    #[test]
    fn should_fallback_tts_key_to_empty_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { remove_env("TTS_API_KEY") };

        let config = AppConfig::from_env();

        assert_eq!(config.tts_api_key, "");
    }

    #[test]
    fn should_read_all_keys_when_all_env_vars_are_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            set_env("SONIOX_API_KEY", "soniox-val");
            set_env("TTS_API_KEY", "tts-val");
            set_env("DEFAULT_VOICE", "voice-val");
        }

        let config = AppConfig::from_env();

        assert_eq!(config.soniox_api_key, "soniox-val");
        assert_eq!(config.tts_api_key, "tts-val");
        assert_eq!(config.default_voice, "voice-val");

        unsafe {
            remove_env("SONIOX_API_KEY");
            remove_env("TTS_API_KEY");
            remove_env("DEFAULT_VOICE");
        }
    }

    #[test]
    fn should_fallback_all_keys_to_defaults_when_no_env_vars_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            remove_env("SONIOX_API_KEY");
            remove_env("TTS_API_KEY");
            remove_env("DEFAULT_VOICE");
        }

        let config = AppConfig::from_env();

        assert_eq!(config.soniox_api_key, "");
        assert_eq!(config.tts_api_key, "");
        assert_eq!(config.default_voice, DEFAULT_VOICE_ID);
        assert_eq!(config.server_addr, SERVER_ADDR);
        assert_eq!(config.max_body_size, MAX_BODY_SIZE);
    }
}
