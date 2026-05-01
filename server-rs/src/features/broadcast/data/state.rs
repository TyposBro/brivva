//! Feature-owned axum State for the broadcast slice.
//!
//! CLAUDE.md §1.2: features may NOT import from orchestration. Axum's
//! `State<T>` extractor needs a concrete type, so the feature declares its
//! own state struct + builder. Orchestration creates an instance from its
//! `AppConfig` and hands it to the router. Downstream handlers only know
//! about `BroadcastState`, not the wider orchestration context.

use crate::features::broadcast::domain::LiveSessions;
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct BroadcastState {
    pub live_sessions: LiveSessions,
    pub jwt_secret: String,
    pub workers_api_url: String,
    pub internal_secret: String,
    pub soniox_api_key: String,
    pub soniox_ws_url: String,
    pub elevenlabs_api_key: String,
    pub elevenlabs_base_url: String,
    /// Kill-switch: force default voice library, skip cloning. See runbook.
    pub force_default_voice: bool,
    /// Kill-switch: downgrade rtmps:// to rtmp:// at FFmpeg spawn. See runbook.
    pub force_rtmp_not_rtmps: bool,
    /// Upload structured session events to Workers/D1 when enabled.
    pub session_logs_enabled: bool,
    /// Include high-volume debug events in the session log stream.
    pub session_logs_verbose: bool,
    /// V2 Phase 1 timeline shadow mode. Logs metrics only; no media behavior change.
    pub v2_timeline_shadow: bool,
    /// V2 Phase 2 timestamped audio ingest. Logs timing only; old PCM remains default.
    pub v2_timestamped_audio: bool,
    /// V2 Phase 3 output health/control logs. Logs/contracts only; no process control.
    pub v2_output_controls: bool,
    /// V2 Phase 4 render graph adapter logs. Wraps current FFmpeg path only.
    pub v2_render_graph: bool,
}

impl BroadcastState {
    pub fn new() -> Self {
        Self {
            live_sessions: Arc::new(DashMap::new()),
            jwt_secret: String::new(),
            workers_api_url: String::new(),
            internal_secret: String::new(),
            soniox_api_key: String::new(),
            soniox_ws_url: String::new(),
            elevenlabs_api_key: String::new(),
            elevenlabs_base_url: String::new(),
            force_default_voice: false,
            force_rtmp_not_rtmps: false,
            session_logs_enabled: false,
            session_logs_verbose: false,
            v2_timeline_shadow: false,
            v2_timestamped_audio: false,
            v2_output_controls: false,
            v2_render_graph: false,
        }
    }
}

impl Default for BroadcastState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_initializes_all_strings_empty_and_flags_false() {
        let s = BroadcastState::new();
        assert!(s.jwt_secret.is_empty());
        assert!(s.workers_api_url.is_empty());
        assert!(s.internal_secret.is_empty());
        assert!(s.soniox_api_key.is_empty());
        assert!(s.soniox_ws_url.is_empty());
        assert!(s.elevenlabs_api_key.is_empty());
        assert!(s.elevenlabs_base_url.is_empty());
        assert!(!s.force_default_voice);
        assert!(!s.force_rtmp_not_rtmps);
        assert!(!s.session_logs_enabled);
        assert!(!s.session_logs_verbose);
        assert!(!s.v2_timeline_shadow);
        assert!(!s.v2_timestamped_audio);
        assert!(!s.v2_output_controls);
        assert!(!s.v2_render_graph);
        assert!(s.live_sessions.is_empty());
    }

    #[test]
    fn default_and_new_produce_equivalent_state() {
        let default_state = BroadcastState::default();
        let new_state = BroadcastState::new();
        assert_eq!(default_state.jwt_secret, new_state.jwt_secret);
        assert_eq!(
            default_state.force_default_voice,
            new_state.force_default_voice
        );
    }

    #[test]
    fn clone_shares_live_sessions_arc_so_mutations_are_visible_to_both() {
        let a = BroadcastState::new();
        let b = a.clone();
        let sessions = a.live_sessions.clone();
        sessions.insert(
            "ROOM".into(),
            crate::features::broadcast::domain::LiveSession::new(
                "ROOM".into(),
                crate::features::broadcast::domain::Lang::En,
                None,
                std::sync::Arc::new(crate::features::broadcast::domain::PipelineConfig::default()),
            ),
        );
        assert_eq!(b.live_sessions.len(), 1);
    }
}
