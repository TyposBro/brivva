mod room;
mod types;

use axum::{Router, routing::get};
use dashmap::DashMap;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let rooms = Arc::new(DashMap::new());

    let app = Router::new()
        .route("/", get(|| async { "Brivva Translation Server" }))
        .route("/ws", get(room::ws_handler))
        .with_state(rooms);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    println!("Listening on http://localhost:3000");
    println!("WebSocket at ws://localhost:3000/ws");
    axum::serve(listener, app).await.unwrap();
}
