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
        }
    }
}

impl Default for BroadcastState {
    fn default() -> Self {
        Self::new()
    }
}
