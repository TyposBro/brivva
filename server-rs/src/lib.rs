pub mod auth;
pub mod ffmpeg;
pub mod pipeline;
pub mod room;
pub mod types;
pub mod workers_api;

use axum::{Router, routing::get};
use dashmap::DashMap;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct AppState {
    pub rooms: Arc<DashMap<String, types::Room>>,
}

pub fn app_state() -> AppState {
    AppState {
        rooms: Arc::new(DashMap::new()),
    }
}

pub fn build_app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route(
            "/",
            get(|| async { "Brivva Translation Server (media-only)" }),
        )
        .route("/api/room", get(room::ws_handler))
        .layer(cors)
        .with_state(state)
}
