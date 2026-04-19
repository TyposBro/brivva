//! Integration coverage for the two kill-switch env vars wired into
//! `server-rs` per `docs/runbook.md`:
//!
//! * `BRIVVA_FALLBACK_TO_DEFAULT_VOICE=1` forces the target-language default
//!   voice id through `resolve_voice`, even when a cloned voice was selected.
//! * `BRIVVA_FORCE_RTMP_NOT_RTMPS=1` rewrites `rtmps://` destinations to
//!   `rtmp://` at FFmpeg spawn time via `maybe_downgrade_rtmps`.
//!
//! These tests exercise branch selection only — no external calls to
//! ElevenLabs or FFmpeg happen. Mutating the env vars is serialized via a
//! process-wide Mutex to keep concurrent test runs honest.

#![allow(clippy::await_holding_lock)]

use server_rs::features::broadcast::data::pipeline::tts::{ResolveVoiceArgs, resolve_voice};
use server_rs::features::broadcast::data::session_ws::maybe_downgrade_rtmps;
use server_rs::features::broadcast::domain::Lang;
use server_rs::orchestration::config::AppConfig;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn clear_kill_switches() {
    unsafe {
        std::env::remove_var("BRIVVA_FALLBACK_TO_DEFAULT_VOICE");
        std::env::remove_var("BRIVVA_FORCE_RTMP_NOT_RTMPS");
    }
}

#[test]
fn fallback_to_default_voice_kill_switch_forces_default_voice_id() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_kill_switches();
    unsafe {
        std::env::set_var("BRIVVA_FALLBACK_TO_DEFAULT_VOICE", "1");
    }
    let cfg = AppConfig::from_env();
    assert!(cfg.force_default_voice);

    let resolved = resolve_voice(ResolveVoiceArgs {
        selected_voice_id: Some("EL-clone-123"),
        enrollment_lang: Some(&Lang::Ko),
        target_lang: &Lang::En,
        force_default_voice: cfg.force_default_voice,
    });
    assert!(!resolved.is_cloned, "kill-switch must force default voice");
    assert_eq!(resolved.voice_id, Lang::En.voice_id().to_string());

    clear_kill_switches();
}

#[test]
fn without_kill_switch_cloned_voice_is_kept_when_enrollment_matches_target() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_kill_switches();
    let cfg = AppConfig::from_env();
    assert!(!cfg.force_default_voice);

    let resolved = resolve_voice(ResolveVoiceArgs {
        selected_voice_id: Some("EL-clone-123"),
        enrollment_lang: Some(&Lang::En),
        target_lang: &Lang::En,
        force_default_voice: cfg.force_default_voice,
    });
    assert!(resolved.is_cloned);
    assert_eq!(resolved.voice_id, "EL-clone-123");
    assert!(!resolved.is_cloned_fallback_due_to_enrollment);
}

#[test]
fn cloned_voice_falls_back_when_enrollment_language_differs_from_target() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_kill_switches();

    let resolved = resolve_voice(ResolveVoiceArgs {
        selected_voice_id: Some("EL-clone-123"),
        enrollment_lang: Some(&Lang::Ko),
        target_lang: &Lang::En,
        force_default_voice: false,
    });
    assert!(!resolved.is_cloned, "cross-lingual clone must fall back");
    assert!(resolved.is_cloned_fallback_due_to_enrollment);
    assert_eq!(resolved.voice_id, Lang::En.voice_id().to_string());
}

#[test]
fn force_rtmp_not_rtmps_kill_switch_downgrades_rtmps_urls() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_kill_switches();
    unsafe {
        std::env::set_var("BRIVVA_FORCE_RTMP_NOT_RTMPS", "true");
    }
    let cfg = AppConfig::from_env();
    assert!(cfg.force_rtmp_not_rtmps);

    let rewritten = maybe_downgrade_rtmps(
        "rtmps://live.grip.fans:443/live/KEY",
        cfg.force_rtmp_not_rtmps,
    );
    assert_eq!(rewritten, "rtmp://live.grip.fans:443/live/KEY");

    clear_kill_switches();
}

#[test]
fn force_rtmp_not_rtmps_does_not_touch_already_unsecured_urls() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_kill_switches();

    let rewritten = maybe_downgrade_rtmps("rtmp://example.com/live/KEY", true);
    assert_eq!(rewritten, "rtmp://example.com/live/KEY");
}

#[test]
fn without_kill_switch_rtmps_urls_are_preserved_as_is() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_kill_switches();
    let cfg = AppConfig::from_env();
    assert!(!cfg.force_rtmp_not_rtmps);

    let url = "rtmps://live.grip.fans:443/live/KEY";
    let preserved = maybe_downgrade_rtmps(url, cfg.force_rtmp_not_rtmps);
    assert_eq!(preserved, url);
}
