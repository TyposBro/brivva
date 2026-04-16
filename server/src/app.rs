use axum::{
    extract::Query,
    extract::ws::WebSocketUpgrade,
    response::IntoResponse,
    routing::get,
    Router,
};

use serde::Deserialize;

use crate::ingest::ws_handler::{handle_source_socket, spawn_source_runtime};

#[derive(Debug, Deserialize)]
struct SourceParams {
    output_url: String,
    delay_ms: Option<u64>,
}

pub fn app() -> Router {
    Router::new()
        .route("/", get(health))
        .route("/ws/source", get(ws_source))
}

async fn health() -> &'static str {
    "ok"
}

async fn ws_source(
    ws: WebSocketUpgrade,
    Query(params): Query<SourceParams>,
) -> impl IntoResponse {
    match spawn_source_runtime(params.output_url, params.delay_ms.unwrap_or(1_000)) {
        Ok(runtime) => ws.on_upgrade(move |socket| async move {
            let session = runtime.session.clone();
            handle_source_socket(socket, session).await;
            let _ = runtime.stop_tx.send(());
        }).into_response(),
        Err(err) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, err).into_response(),
    }
}
