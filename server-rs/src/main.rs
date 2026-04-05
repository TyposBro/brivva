use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let log_dir = std::env::temp_dir().join("brivva");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("server.log");
    eprintln!("Logging to: {}", log_path.display());

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("Failed to open log file");

    let file_layer = fmt::layer()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false);

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    server_rs::run_server().await;
}
