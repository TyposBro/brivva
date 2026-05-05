use server_rs::{app_state, build_app, features::broadcast::data::ffmpeg};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() {
    init_tracing();

    // Kill any orphan FFmpeg processes from a previous crash.
    ffmpeg::kill_orphan_ffmpeg();

    let state = app_state();
    ffmpeg::log_startup_runtime_self_check(state.video_encoder);

    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    tracing::info!(
        addr = "0.0.0.0:3000",
        ws_path = "/api/session",
        ws_alias = "/api/room",
        "brivva media server listening"
    );
    axum::serve(listener, app).await.unwrap();
}

/// Wire up tracing. In production (default) emit JSON so CloudWatch Logs
/// Insights can query `$.fields.<name>`. Set `RUST_LOG_FORMAT=pretty` for
/// human-readable output during local development. RUST_LOG controls the
/// level filter (default info).
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let pretty = std::env::var("RUST_LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("pretty"))
        .unwrap_or(false);

    if pretty {
        fmt().with_env_filter(filter).with_target(false).init();
    } else {
        fmt()
            .json()
            .with_current_span(true)
            .with_span_list(false)
            .with_env_filter(filter)
            .with_target(true)
            .init();
    }
}
