use crate::features::broadcast::domain::LiveSession;
use crate::orchestration::config::AppConfig;
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub live_sessions: Arc<DashMap<String, LiveSession>>,
    pub config: Arc<AppConfig>,
}

pub fn app_state() -> AppState {
    AppState {
        live_sessions: Arc::new(DashMap::new()),
        config: AppConfig::from_env(),
    }
}
