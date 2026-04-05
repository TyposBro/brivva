pub mod core;
pub mod features;
pub mod orchestration;
pub mod shared;

use dashmap::DashMap;
use std::sync::Arc;

use core::types::Sessions;
use orchestration::config::AppConfig;
use orchestration::di::AppContext;

pub async fn run_server() {
    init_logging();
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

fn init_logging() {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let log_dir = std::env::temp_dir().join("brivva");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("server.log");
    eprintln!("Logging to: {}", log_path.display());

    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open log file: {}", e);
            return;
        }
    };

    let file_layer = fmt::layer()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false);

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr);

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();
}

fn load_env() {
    if dotenvy::from_filename(".env.local").is_err()
        && let Err(e) = dotenvy::dotenv() {
            tracing::warn!(".env not loaded ({e}). Using existing environment variables.");
        }
}
