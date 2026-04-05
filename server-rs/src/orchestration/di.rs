//! Dependency injection — application context constructed once at startup.

use super::config::AppConfig;

/// Application-wide dependencies, constructed in orchestration and passed to lower layers.
pub struct AppContext {
    pub config: AppConfig,
    pub http_client: reqwest::Client,
}

impl AppContext {
    pub fn new(config: AppConfig) -> Self {
        let http_client = reqwest::Client::builder()
            .pool_max_idle_per_host(4)
            .build()
            .expect("failed to build HTTP client");
        Self { config, http_client }
    }
}
