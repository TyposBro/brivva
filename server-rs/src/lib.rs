pub mod core;
pub mod features;
pub mod orchestration;
pub mod streaming;
pub mod stt;
pub mod translation;
pub mod tts;
pub mod voice_clone;
mod pipeline;
mod ws;
mod api;

use dashmap::DashMap;
use std::sync::Arc;
use std::sync::LazyLock;

use features::broadcast::domain::Sessions;
use orchestration::config::AppConfig;

// PRAGMATIC: HTTP_CLIENT remains a global static during the config-threading
// transition (Phase 4). It will be moved into AppContext once all consumers
// accept &reqwest::Client as a parameter.
pub(crate) static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(4)
        .build()
        .expect("failed to build HTTP client")
});

pub async fn run_server() {
    load_env();
    let _config = AppConfig::from_env();
    streaming::kill_orphan_ffmpeg();

    let sessions: Sessions = Arc::new(DashMap::new());
    let app = orchestration::router::build_router(sessions);

    let listener = tokio::net::TcpListener::bind(core::config::SERVER_ADDR).await.unwrap();
    tracing::info!("Brivva server on http://{}", core::config::SERVER_ADDR);
    axum::serve(listener, app).await.unwrap();
}

fn load_env() {
    if dotenvy::from_filename(".env.local").is_err()
        && let Err(e) = dotenvy::dotenv() {
            tracing::warn!(".env not loaded ({e}). Using existing environment variables.");
        }
}
