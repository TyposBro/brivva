mod db;
mod ffmpeg;
mod pipeline;
mod room;
mod routes;
mod types;
mod youtube;

use axum::{
    Router,
    routing::{delete, get, post},
};
use dashmap::DashMap;
use sqlx::SqlitePool;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

/// Shared application state: rooms + database pool
#[derive(Clone)]
pub struct AppState {
    pub rooms: Arc<DashMap<String, types::Room>>,
    pub db: SqlitePool,
}

#[tokio::main]
async fn main() {
    let pool = db::init_db().await;
    let rooms = Arc::new(DashMap::new());

    let state = AppState {
        rooms: rooms.clone(),
        db: pool,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Existing
        .route("/", get(|| async { "Brivva Translation Server" }))
        .route("/api/room", get(room::ws_handler))
        // YouTube OAuth
        .route("/auth/youtube", get(routes::youtube_auth))
        .route("/auth/youtube/callback", get(routes::youtube_callback))
        // User
        .route("/api/user", get(routes::get_user))
        // Sessions
        .route("/api/sessions", post(routes::create_session))
        .route("/api/sessions", get(routes::list_sessions))
        .route("/api/sessions/{id}", get(routes::get_session))
        .route("/api/sessions/{id}", delete(routes::delete_session))
        .route("/api/sessions/{id}/streams", post(routes::add_stream))
        .route("/api/sessions/{session_id}/streams/{stream_id}", delete(routes::remove_stream))
        // Voices
        .route("/api/voices", post(routes::create_voice))
        .route("/api/voices", get(routes::list_voices))
        .route("/api/voices/{id}", delete(routes::delete_voice))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    println!("Listening on http://localhost:3000");
    println!("WebSocket at ws://localhost:3000/api/room");
    axum::serve(listener, app).await.unwrap();
}
