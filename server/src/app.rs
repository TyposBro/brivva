use std::{collections::HashMap, sync::{Arc, atomic::{AtomicU64, Ordering}}};

use axum::{
    extract::{Path, Query, State},
    extract::ws::WebSocketUpgrade,
    response::Json,
    response::IntoResponse,
    routing::get,
    Router,
};

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::ingest::ws_handler::{handle_source_socket, spawn_source_runtime};
use crate::session::SourceSessionSnapshot;

#[derive(Clone, Default)]
struct AppState {
    sessions: Arc<Mutex<HashMap<String, crate::ingest::ws_handler::SharedSourceSession>>>,
}

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Deserialize)]
struct SourceParams {
    output_url: String,
    delay_ms: Option<u64>,
}

pub fn app() -> Router {
    let state = AppState::default();
    Router::new()
        .route("/", get(health))
        .route("/sessions", get(list_sessions))
        .route("/sessions/:id", get(get_session))
        .route("/ws/source", get(ws_source))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn ws_source(
    ws: WebSocketUpgrade,
    Query(params): Query<SourceParams>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match spawn_source_runtime(params.output_url, params.delay_ms.unwrap_or(1_000)) {
        Ok(runtime) => {
            let id = format!("source-{}", NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed));
            ws.on_upgrade(move |socket| async move {
            {
                let mut sessions = state.sessions.lock().await;
                sessions.insert(id.clone(), runtime.session.clone());
            }
            let session = runtime.session.clone();
            handle_source_socket(socket, session).await;
            let _ = runtime.stop_tx.send(());
            let mut sessions = state.sessions.lock().await;
            sessions.remove(&id);
        }).into_response()
        }
        Err(err) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, err).into_response(),
    }
}

async fn list_sessions(
    State(state): State<AppState>,
) -> Json<HashMap<String, SourceSessionSnapshot>> {
    let sessions = state.sessions.lock().await;
    let mut out = HashMap::new();
    for (id, session) in sessions.iter() {
        let snapshot = session.lock().await.snapshot();
        out.insert(id.clone(), snapshot);
    }
    Json(out)
}

async fn get_session(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<SourceSessionSnapshot>, axum::http::StatusCode> {
    let session = {
        let sessions = state.sessions.lock().await;
        sessions.get(&id).cloned()
    }
    .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    let snapshot = session.lock().await.snapshot();
    Ok(Json(snapshot))
}
