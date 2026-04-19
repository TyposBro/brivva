//! Composition root for runtime configuration.
//!
//! CLAUDE.md §7: environment variables are read ONLY here. Lower layers
//! receive the values they need through constructor injection via `AppState`.
//!
//! Defaults bias toward production endpoints so a missing override is safe;
//! required secrets (JWT/INTERNAL) default to empty and fail fast at use.

use std::sync::Arc;

const SONIOX_WS_URL_DEFAULT: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
const ELEVENLABS_BASE_URL_DEFAULT: &str = "https://api.elevenlabs.io";

pub struct AppConfig {
    pub jwt_secret: String,
    pub workers_api_url: String,
    pub internal_secret: String,
    pub soniox_api_key: String,
    pub soniox_ws_url: String,
    pub elevenlabs_api_key: String,
    pub elevenlabs_base_url: String,
}

impl AppConfig {
    pub fn from_env() -> Arc<Self> {
        Arc::new(Self {
            jwt_secret: env_or_default("JWT_SECRET", ""),
            workers_api_url: env_or_default("WORKERS_API_URL", "")
                .trim_end_matches('/')
                .to_string(),
            internal_secret: env_or_default("INTERNAL_SECRET", ""),
            soniox_api_key: env_or_default("SONIOX_API_KEY", ""),
            soniox_ws_url: env_or_default("SONIOX_WS_URL", SONIOX_WS_URL_DEFAULT),
            elevenlabs_api_key: env_or_default("ELEVENLABS_API_KEY", ""),
            elevenlabs_base_url: env_or_default("ELEVENLABS_BASE_URL", ELEVENLABS_BASE_URL_DEFAULT)
                .trim_end_matches('/')
                .to_string(),
        })
    }
}

fn env_or_default(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn from_env_applies_soniox_and_elevenlabs_defaults_when_unset() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            for key in [
                "JWT_SECRET",
                "WORKERS_API_URL",
                "INTERNAL_SECRET",
                "SONIOX_API_KEY",
                "SONIOX_WS_URL",
                "ELEVENLABS_API_KEY",
                "ELEVENLABS_BASE_URL",
            ] {
                std::env::remove_var(key);
            }
        }

        let cfg = AppConfig::from_env();
        assert_eq!(cfg.soniox_ws_url, SONIOX_WS_URL_DEFAULT);
        assert_eq!(cfg.elevenlabs_base_url, ELEVENLABS_BASE_URL_DEFAULT);
        assert!(cfg.jwt_secret.is_empty());
    }

    #[test]
    fn from_env_trims_trailing_slash_on_urls() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("WORKERS_API_URL", "https://example.com/");
            std::env::set_var("ELEVENLABS_BASE_URL", "https://el.example.com/");
        }

        let cfg = AppConfig::from_env();
        assert_eq!(cfg.workers_api_url, "https://example.com");
        assert_eq!(cfg.elevenlabs_base_url, "https://el.example.com");
    }
}
