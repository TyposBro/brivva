use crate::features::broadcast::data::session_ws_handler;
use crate::orchestration::state::AppState;
use axum::{Router, routing::get};
use tower_http::cors::{Any, CorsLayer};

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
        .route("/health", get(|| async { "ok" }))
        .route("/api/session", get(session_ws_handler))
        .route("/api/room", get(session_ws_handler))
        .layer(cors)
        .with_state(state)
}
