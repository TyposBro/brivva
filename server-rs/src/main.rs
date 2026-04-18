mod auth;
mod ffmpeg;
mod pipeline;
mod room;
mod types;
mod workers_api;

use axum::{Router, routing::get};
use dashmap::DashMap;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

/// Shared application state. DB moved to Workers + D1 (Phase 3) —
/// only the live in-memory Rooms map lives on Fargate now.
#[derive(Clone)]
pub struct AppState {
    pub rooms: Arc<DashMap<String, types::Room>>,
}

#[tokio::main]
async fn main() {
    // Kill any orphan FFmpeg processes from a previous crash.
    ffmpeg::kill_orphan_ffmpeg();

    let state = AppState {
        rooms: Arc::new(DashMap::new()),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(|| async { "Brivva Translation Server (media-only)" }))
        .route("/api/room", get(room::ws_handler))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    println!("Listening on http://localhost:3000");
    println!("WebSocket at ws://localhost:3000/api/room");
    axum::serve(listener, app).await.unwrap();
}
