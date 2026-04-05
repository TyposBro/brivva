pub mod core;
pub mod features;
pub mod orchestration;
pub mod shared;

use dashmap::DashMap;
use std::sync::Arc;

use features::broadcast::domain::Sessions;
use orchestration::config::AppConfig;
use orchestration::di::AppContext;

pub async fn run_server() {
    load_env();
    let config = AppConfig::from_env();
    let app_ctx = Arc::new(AppContext::new(config));
    features::broadcast::data::streaming::kill_orphan_ffmpeg();

    let sessions: Sessions = Arc::new(DashMap::new());
    let app = orchestration::router::build_router(sessions, app_ctx);

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
