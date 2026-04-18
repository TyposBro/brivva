use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::Serialize;
use serde_json::{Value, json};
use server_rs::{app_state, build_app};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Serialize)]
struct TestClaims<'a> {
    sub: &'a str,
    exp: i64,
    iss: &'a str,
    aud: &'a str,
}

fn make_token(secret: &str, sub: &str) -> String {
    encode(
        &Header::default(),
        &TestClaims {
            sub,
            exp: 4_102_444_800,
            iss: "brivva-api",
            aud: "brivva-fargate",
        },
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("encode jwt")
}

#[derive(Clone)]
struct MockWorkersState {
    session_user_id: String,
    fail_fetch: bool,
    requests: Arc<tokio::sync::Mutex<Vec<RecordedRequest>>>,
}

#[derive(Clone, Debug)]
struct RecordedRequest {
    method: String,
    path: String,
    internal_secret: Option<String>,
    body: Option<Value>,
}

async fn get_session(
    State(state): State<MockWorkersState>,
    Path(session_id): Path<String>,
) -> axum::http::StatusCode {
    state.requests.lock().await.push(RecordedRequest {
        method: "GET".into(),
        path: format!("/internal/sessions/{}", session_id),
        internal_secret: None,
        body: None,
    });

    if state.fail_fetch {
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    } else {
        axum::http::StatusCode::OK
    }
}

async fn get_session_json(
    State(state): State<MockWorkersState>,
    Path(session_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Json<Value> {
    state.requests.lock().await.push(RecordedRequest {
        method: "GET".into(),
        path: format!("/internal/sessions/{}", session_id),
        internal_secret: headers
            .get("X-Internal-Secret")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        body: None,
    });

    Json(json!({
        "session": {
            "id": session_id,
            "user_id": state.session_user_id,
            "voice_id": "voice-row-1",
            "title": "Session",
            "source_lang": "en",
            "target_langs": "ja,ko",
            "status": "scheduled",
            "room_id": null,
            "created_at": 1
        },
        "streams": [],
        "voice": {
            "id": "voice-row-1",
            "elevenlabs_voice_id": "voice-clone-123",
            "name": "Cloned Voice"
        }
    }))
}

async fn patch_session(
    State(state): State<MockWorkersState>,
    Path(session_id): Path<String>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> axum::http::StatusCode {
    state.requests.lock().await.push(RecordedRequest {
        method: "PATCH".into(),
        path: format!("/internal/sessions/{}", session_id),
        internal_secret: headers
            .get("X-Internal-Secret")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        body: Some(body),
    });

    axum::http::StatusCode::OK
}

async fn spawn_workers_mock(
    session_user_id: &str,
    fail_fetch: bool,
) -> (String, MockWorkersState, JoinHandle<()>) {
    let state = MockWorkersState {
        session_user_id: session_user_id.to_string(),
        fail_fetch,
        requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    };

    let app = if fail_fetch {
        Router::new()
            .route(
                "/internal/sessions/{id}",
                get(get_session).patch(patch_session),
            )
            .with_state(state.clone())
    } else {
        Router::new()
            .route(
                "/internal/sessions/{id}",
                get(get_session_json).patch(patch_session),
            )
            .with_state(state.clone())
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind workers mock");
    let addr = listener.local_addr().expect("workers local addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve workers mock");
    });
    (format!("http://{}", addr), state, handle)
}

async fn spawn_app() -> (String, server_rs::AppState, JoinHandle<()>) {
    let state = app_state();
    let app = build_app(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind app");
    let addr = listener.local_addr().expect("app local addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve app");
    });
    (format!("http://{}", addr), state, handle)
}

async fn wait_for_room_count(state: &server_rs::AppState, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if state.live_sessions.len() == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("room count update");
}

async fn wait_for_requests(state: &MockWorkersState, expected_len: usize) -> Vec<RecordedRequest> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let requests = state.requests.lock().await.clone();
            if requests.len() >= expected_len {
                return requests;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("expected workers requests")
}

#[tokio::test]
async fn happy_path_session_lifecycle_updates_workers_and_cleans_room() {
    let (_guard, workers_url, workers_state, workers_server, app_url, app_state, app_server) = {
        let guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let (workers_url, workers_state, workers_server) =
            spawn_workers_mock("host-1", false).await;
        unsafe {
            std::env::set_var("JWT_SECRET", "e2e-secret");
            std::env::set_var("WORKERS_API_URL", &workers_url);
            std::env::set_var("INTERNAL_SECRET", "internal-secret");
        }
        let (app_url, app_state, app_server) = spawn_app().await;
        (
            guard,
            workers_url,
            workers_state,
            workers_server,
            app_url,
            app_state,
            app_server,
        )
    };

    let token = make_token("e2e-secret", "host-1");
    let ws_url = format!(
        "{}/api/session?token={}&sessionId=session-1&sourceLang=en",
        app_url.replacen("http", "ws", 1),
        token
    );
    let (mut socket, _) = connect_async(&ws_url).await.expect("connect ws");

    wait_for_room_count(&app_state, 1).await;
    let live_session = app_state
        .live_sessions
        .iter()
        .next()
        .expect("live session exists");
    let runtime_id = live_session.key().clone();
    assert_eq!(
        live_session.selected_voice_id.as_deref(),
        Some("voice-clone-123")
    );
    drop(live_session);

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            "host:end".into(),
        ))
        .await
        .expect("send host:end");
    let _ = tokio::time::timeout(Duration::from_secs(2), socket.next()).await;

    wait_for_room_count(&app_state, 0).await;
    let requests = wait_for_requests(&workers_state, 3).await;

    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/internal/sessions/session-1");
    assert_eq!(
        requests[0].internal_secret.as_deref(),
        Some("internal-secret")
    );

    assert_eq!(requests[1].method, "PATCH");
    assert_eq!(
        requests[1]
            .body
            .as_ref()
            .and_then(|b| b.get("status"))
            .and_then(Value::as_str),
        Some("live")
    );
    assert_eq!(
        requests[1]
            .body
            .as_ref()
            .and_then(|b| b.get("room_id"))
            .and_then(Value::as_str),
        Some(runtime_id.as_str())
    );

    assert_eq!(requests[2].method, "PATCH");
    assert_eq!(
        requests[2]
            .body
            .as_ref()
            .and_then(|b| b.get("status"))
            .and_then(Value::as_str),
        Some("ended")
    );
    assert_eq!(
        requests[2].body.as_ref().and_then(|b| b.get("room_id")),
        Some(&Value::Null)
    );
    assert_eq!(runtime_id.len(), 6);
    assert!(
        runtime_id
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    );
    assert_eq!(workers_url.starts_with("http://"), true);

    app_server.abort();
    workers_server.abort();
}

#[tokio::test]
async fn sad_path_owner_mismatch_rejects_connection_before_room_creation() {
    let (_guard, workers_state, workers_server, app_url, app_state, app_server) = {
        let guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let (workers_url, workers_state, workers_server) =
            spawn_workers_mock("someone-else", false).await;
        unsafe {
            std::env::set_var("JWT_SECRET", "e2e-secret");
            std::env::set_var("WORKERS_API_URL", &workers_url);
            std::env::set_var("INTERNAL_SECRET", "internal-secret");
        }
        let (app_url, app_state, app_server) = spawn_app().await;
        (
            guard,
            workers_state,
            workers_server,
            app_url,
            app_state,
            app_server,
        )
    };

    let token = make_token("e2e-secret", "host-1");
    let ws_url = format!(
        "{}/api/session?token={}&sessionId=session-1",
        app_url.replacen("http", "ws", 1),
        token
    );
    let (_socket, _) = connect_async(&ws_url).await.expect("connect ws");

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(app_state.live_sessions.len(), 0);

    let requests = wait_for_requests(&workers_state, 1).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");

    app_server.abort();
    workers_server.abort();
}

#[tokio::test]
async fn edge_case_workers_fetch_failure_rejects_before_room_creation() {
    let (_guard, workers_state, workers_server, app_url, app_state, app_server) = {
        let guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let (workers_url, workers_state, workers_server) = spawn_workers_mock("host-1", true).await;
        unsafe {
            std::env::set_var("JWT_SECRET", "e2e-secret");
            std::env::set_var("WORKERS_API_URL", &workers_url);
            std::env::set_var("INTERNAL_SECRET", "internal-secret");
        }
        let (app_url, app_state, app_server) = spawn_app().await;
        (
            guard,
            workers_state,
            workers_server,
            app_url,
            app_state,
            app_server,
        )
    };

    let token = make_token("e2e-secret", "host-1");
    let ws_url = format!(
        "{}/api/session?token={}&sessionId=session-1",
        app_url.replacen("http", "ws", 1),
        token
    );
    let (_socket, _) = connect_async(&ws_url).await.expect("connect ws");

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(app_state.live_sessions.len(), 0);

    let requests = wait_for_requests(&workers_state, 1).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");

    app_server.abort();
    workers_server.abort();
}
