use crate::features::broadcast::domain::LiveSession;
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub live_sessions: Arc<DashMap<String, LiveSession>>,
}

pub fn app_state() -> AppState {
    AppState {
        live_sessions: Arc::new(DashMap::new()),
    }
}
