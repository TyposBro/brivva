//! Composition root: builds feature-owned state from env-sourced AppConfig.
//!
//! Lower layers never import this module. Orchestration constructs the
//! feature state and hands it to each feature's router at build time.

use crate::features::broadcast::data::BroadcastState;
use crate::features::broadcast::domain::LiveSessions;
use crate::orchestration::config::AppConfig;
use dashmap::DashMap;
use std::sync::Arc;

pub fn broadcast_state() -> BroadcastState {
    let cfg = AppConfig::from_env();
    let live_sessions: LiveSessions = Arc::new(DashMap::new());
    BroadcastState {
        live_sessions,
        jwt_secret: cfg.jwt_secret.clone(),
        workers_api_url: cfg.workers_api_url.clone(),
        internal_secret: cfg.internal_secret.clone(),
        soniox_api_key: cfg.soniox_api_key.clone(),
        soniox_ws_url: cfg.soniox_ws_url.clone(),
        elevenlabs_api_key: cfg.elevenlabs_api_key.clone(),
        elevenlabs_base_url: cfg.elevenlabs_base_url.clone(),
        force_default_voice: cfg.force_default_voice,
        force_rtmp_not_rtmps: cfg.force_rtmp_not_rtmps,
        session_logs_enabled: cfg.session_logs_enabled,
        session_logs_verbose: cfg.session_logs_verbose,
        v2_timeline_shadow: cfg.v2_timeline_shadow,
        v2_timestamped_audio: cfg.v2_timestamped_audio,
    }
}
