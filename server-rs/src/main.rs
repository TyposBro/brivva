mod pipeline;
mod room;
mod types;

use axum::{Router, routing::get};
use dashmap::DashMap;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() {
    let rooms = Arc::new(DashMap::new());

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(|| async { "Brivva Translation Server" }))
        .route("/api/room", get(room::ws_handler))
        .layer(cors)
        .with_state(rooms);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    println!("Listening on http://localhost:3000");
    println!("WebSocket at ws://localhost:3000/api/room");
    axum::serve(listener, app).await.unwrap();
}
