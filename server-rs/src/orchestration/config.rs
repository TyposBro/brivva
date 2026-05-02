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
    /// Kill-switch: force default voice library, skip voice cloning. Used
    /// when voice clone produces garbage (e.g. the April 2026 Indian-accent
    /// regression). See `docs/runbook.md`.
    pub force_default_voice: bool,
    /// Kill-switch: downgrade `rtmps://` destinations to `rtmp://` when a
    /// platform's TLS stack is flaking. Only usable on platforms that accept
    /// unsecured RTMP. See `docs/runbook.md`.
    pub force_rtmp_not_rtmps: bool,
    /// Upload structured session logs to Workers/D1 for NDJSON export.
    pub session_logs_enabled: bool,
    /// Include high-volume per-session debug events such as media stats.
    pub session_logs_verbose: bool,
    /// V2 Phase 1 timeline shadow mode. Logs metrics only; no media behavior change.
    pub v2_timeline_shadow: bool,
    /// V2 Phase 2 timestamped audio ingest. Logs timing only; old PCM remains default.
    pub v2_timestamped_audio: bool,
    /// V2 Phase 3 output health/control logs. Logs/contracts only; no process control.
    pub v2_output_controls: bool,
    /// V2 Phase 4 render graph adapter logs. Wraps current FFmpeg path only.
    pub v2_render_graph: bool,
    /// V2 Phase 5 shared decode/fan-out planning. Shadow/contracts only; no live routing.
    pub v2_shared_decode: bool,
    /// V2 Phase 6A GPU worker shadow proof. Logs/contracts only; no live routing.
    pub v2_gpu_workers: bool,
    /// V2 FFmpeg tee fanout. One encoder can publish one localized stream to
    /// multiple platforms. Off by default until Grip tee behavior is proven.
    pub v2_encoded_fanout: bool,
    /// FFmpeg encoder backend. `x264` is portable; `nvenc` enables NVIDIA GPU
    /// encode on GPU hosts with an FFmpeg build that includes h264_nvenc.
    pub video_encoder: crate::features::broadcast::domain::VideoEncoderKind,
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
            force_default_voice: env_flag("BRIVVA_FALLBACK_TO_DEFAULT_VOICE"),
            force_rtmp_not_rtmps: env_flag("BRIVVA_FORCE_RTMP_NOT_RTMPS"),
            session_logs_enabled: env_flag("BRIVVA_SESSION_LOGS"),
            session_logs_verbose: env_flag("BRIVVA_SESSION_LOG_VERBOSE"),
            v2_timeline_shadow: env_flag("BRIVVA_V2_TIMELINE_SHADOW"),
            v2_timestamped_audio: env_flag("BRIVVA_V2_TIMESTAMPED_AUDIO"),
            v2_output_controls: env_flag("BRIVVA_V2_OUTPUT_CONTROLS"),
            v2_render_graph: env_flag("BRIVVA_V2_RENDER_GRAPH"),
            v2_shared_decode: env_flag("BRIVVA_V2_SHARED_DECODE"),
            v2_gpu_workers: env_flag("BRIVVA_V2_GPU_WORKERS"),
            v2_encoded_fanout: env_flag("BRIVVA_V2_ENCODED_FANOUT"),
            video_encoder: crate::features::broadcast::domain::VideoEncoderKind::from_wire(
                &env_or_default("BRIVVA_VIDEO_ENCODER", "x264"),
            ),
        })
    }
}

fn env_or_default(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Truthy values: "1", "true", "yes" (case-insensitive). Anything else is
/// treated as disabled, including empty/unset.
fn env_flag(key: &str) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
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
                "BRIVVA_SESSION_LOGS",
                "BRIVVA_SESSION_LOG_VERBOSE",
                "BRIVVA_V2_TIMELINE_SHADOW",
                "BRIVVA_V2_TIMESTAMPED_AUDIO",
                "BRIVVA_V2_OUTPUT_CONTROLS",
                "BRIVVA_V2_RENDER_GRAPH",
                "BRIVVA_V2_SHARED_DECODE",
                "BRIVVA_V2_GPU_WORKERS",
                "BRIVVA_V2_ENCODED_FANOUT",
                "BRIVVA_VIDEO_ENCODER",
            ] {
                std::env::remove_var(key);
            }
        }

        let cfg = AppConfig::from_env();
        assert_eq!(cfg.soniox_ws_url, SONIOX_WS_URL_DEFAULT);
        assert_eq!(cfg.elevenlabs_base_url, ELEVENLABS_BASE_URL_DEFAULT);
        assert!(cfg.jwt_secret.is_empty());
        assert!(!cfg.v2_timeline_shadow);
        assert!(!cfg.v2_timestamped_audio);
        assert!(!cfg.v2_output_controls);
        assert!(!cfg.v2_render_graph);
        assert!(!cfg.v2_shared_decode);
        assert!(!cfg.v2_gpu_workers);
        assert!(!cfg.v2_encoded_fanout);
        assert_eq!(
            cfg.video_encoder,
            crate::features::broadcast::domain::VideoEncoderKind::X264
        );
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

    #[test]
    fn env_flag_accepts_truthy_values_regardless_of_case() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        for truthy in [
            "1", "true", "yes", "on", "TRUE", "Yes", "ON", " true ", "1\n",
        ] {
            unsafe {
                std::env::set_var("BRIVVA_TEST_FLAG", truthy);
            }
            assert!(
                env_flag("BRIVVA_TEST_FLAG"),
                "expected {truthy:?} to be truthy"
            );
        }
        unsafe {
            std::env::remove_var("BRIVVA_TEST_FLAG");
        }
    }

    #[test]
    fn env_flag_rejects_falsy_values_including_empty_and_non_literal() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        for falsy in ["0", "false", "no", "off", "", "2", "enabled", "disabled"] {
            unsafe {
                std::env::set_var("BRIVVA_TEST_FLAG", falsy);
            }
            assert!(
                !env_flag("BRIVVA_TEST_FLAG"),
                "expected {falsy:?} to be falsy"
            );
        }
        unsafe {
            std::env::remove_var("BRIVVA_TEST_FLAG");
        }
    }

    #[test]
    fn env_flag_is_false_when_env_var_unset() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::remove_var("BRIVVA_TEST_FLAG_MISSING");
        }
        assert!(!env_flag("BRIVVA_TEST_FLAG_MISSING"));
    }

    #[test]
    fn from_env_populates_secrets_and_kill_switches_from_environment() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("JWT_SECRET", "abc");
            std::env::set_var("INTERNAL_SECRET", "xyz");
            std::env::set_var("SONIOX_API_KEY", "sk-soniox");
            std::env::set_var("SONIOX_WS_URL", "wss://custom");
            std::env::set_var("ELEVENLABS_API_KEY", "sk-eleven");
            std::env::set_var("ELEVENLABS_BASE_URL", "https://el");
            std::env::set_var("BRIVVA_FALLBACK_TO_DEFAULT_VOICE", "yes");
            std::env::set_var("BRIVVA_FORCE_RTMP_NOT_RTMPS", "on");
            std::env::set_var("BRIVVA_SESSION_LOGS", "1");
            std::env::set_var("BRIVVA_SESSION_LOG_VERBOSE", "true");
            std::env::set_var("BRIVVA_V2_TIMELINE_SHADOW", "on");
            std::env::set_var("BRIVVA_V2_TIMESTAMPED_AUDIO", "yes");
            std::env::set_var("BRIVVA_V2_OUTPUT_CONTROLS", "true");
            std::env::set_var("BRIVVA_V2_RENDER_GRAPH", "1");
            std::env::set_var("BRIVVA_V2_SHARED_DECODE", "on");
            std::env::set_var("BRIVVA_V2_GPU_WORKERS", "yes");
            std::env::set_var("BRIVVA_V2_ENCODED_FANOUT", "on");
            std::env::set_var("BRIVVA_VIDEO_ENCODER", "h264_nvenc");
        }

        let cfg = AppConfig::from_env();
        assert_eq!(cfg.jwt_secret, "abc");
        assert_eq!(cfg.internal_secret, "xyz");
        assert_eq!(cfg.soniox_api_key, "sk-soniox");
        assert_eq!(cfg.soniox_ws_url, "wss://custom");
        assert_eq!(cfg.elevenlabs_api_key, "sk-eleven");
        assert_eq!(cfg.elevenlabs_base_url, "https://el");
        assert!(cfg.force_default_voice);
        assert!(cfg.force_rtmp_not_rtmps);
        assert!(cfg.session_logs_enabled);
        assert!(cfg.session_logs_verbose);
        assert!(cfg.v2_timeline_shadow);
        assert!(cfg.v2_timestamped_audio);
        assert!(cfg.v2_output_controls);
        assert!(cfg.v2_render_graph);
        assert!(cfg.v2_shared_decode);
        assert!(cfg.v2_gpu_workers);
        assert!(cfg.v2_encoded_fanout);
        assert_eq!(
            cfg.video_encoder,
            crate::features::broadcast::domain::VideoEncoderKind::Nvenc
        );

        unsafe {
            for key in [
                "JWT_SECRET",
                "INTERNAL_SECRET",
                "SONIOX_API_KEY",
                "SONIOX_WS_URL",
                "ELEVENLABS_API_KEY",
                "ELEVENLABS_BASE_URL",
                "BRIVVA_FALLBACK_TO_DEFAULT_VOICE",
                "BRIVVA_FORCE_RTMP_NOT_RTMPS",
                "BRIVVA_SESSION_LOGS",
                "BRIVVA_SESSION_LOG_VERBOSE",
                "BRIVVA_V2_TIMELINE_SHADOW",
                "BRIVVA_V2_TIMESTAMPED_AUDIO",
                "BRIVVA_V2_OUTPUT_CONTROLS",
                "BRIVVA_V2_RENDER_GRAPH",
                "BRIVVA_V2_SHARED_DECODE",
                "BRIVVA_V2_GPU_WORKERS",
                "BRIVVA_V2_ENCODED_FANOUT",
                "BRIVVA_VIDEO_ENCODER",
            ] {
                std::env::remove_var(key);
            }
        }
    }
}
