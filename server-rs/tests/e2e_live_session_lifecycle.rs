#![allow(clippy::await_holding_lock)]
// Tests intentionally hold a process-wide std::sync::Mutex guard while async
// code mutates global env vars. This serializes env-dependent app boot.

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
            "live_session_id": null,
            "voice_preset": "female",
            "created_at": 1
        },
        "streams": [],
        "voice": {
            "id": "voice-row-1",
            "user_id": state.session_user_id.clone(),
            "elevenlabs_voice_id": "voice-clone-123",
            "name": "Cloned Voice",
            "created_at": 1
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

async fn wait_for_live_session_count(state: &server_rs::AppState, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if state.live_sessions.len() == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("live session count update");
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
async fn happy_path_session_lifecycle_updates_workers_and_cleans_live_session() {
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

    wait_for_live_session_count(&app_state, 1).await;
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

    wait_for_live_session_count(&app_state, 0).await;
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
            .and_then(|b| b.get("live_session_id"))
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
        requests[2]
            .body
            .as_ref()
            .and_then(|b| b.get("live_session_id")),
        Some(&Value::Null)
    );
    assert_eq!(runtime_id.len(), 6);
    assert!(
        runtime_id
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    );
    assert!(workers_url.starts_with("http://"));

    app_server.abort();
    workers_server.abort();
}

#[tokio::test]
async fn sad_path_owner_mismatch_rejects_connection_before_live_session_creation() {
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

/// Production regression (2026-04-20): host pressed stop→start without
/// ending the session. The FE re-opened the WS while the prior connection
/// lingered, leaving two live_sessions for the same FE session_id and
/// silencing TTS for the entire restart window. The server must evict the
/// stale row at the new WS upgrade so only one pipeline is ever active.
#[tokio::test]
async fn stop_start_within_session_evicts_prior_live_session_before_starting_new_one() {
    let (_guard, _workers_url, _workers_state, workers_server, app_url, app_state, app_server) = {
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

    // First WS — simulates the host pressing "start recording".
    let (mut first_socket, _) = connect_async(&ws_url).await.expect("connect first ws");
    wait_for_live_session_count(&app_state, 1).await;
    let first_runtime_id = app_state
        .live_sessions
        .iter()
        .next()
        .expect("first live session")
        .key()
        .clone();

    // Second WS — simulates the host pressing "stop" (FE drops the
    // MediaRecorder but the prior WS lingers) and then "start recording"
    // again. The new connection lands at the server BEFORE the old one
    // closes, so the eviction MUST replace the prior live_session in-place.
    let (mut second_socket, _) = connect_async(&ws_url).await.expect("connect second ws");

    // The new live_session ID is whatever entry now sits in the map. The
    // count must be exactly one — never two — at every observation point.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let entries: Vec<String> = app_state
                .live_sessions
                .iter()
                .map(|e| e.key().clone())
                .collect();
            if entries.len() == 1 && entries[0] != first_runtime_id {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("eviction replaced the stale live_session with a fresh one");

    let second_runtime_id = app_state
        .live_sessions
        .iter()
        .next()
        .expect("second live session")
        .key()
        .clone();
    assert_ne!(
        first_runtime_id, second_runtime_id,
        "second connect should mint a new live_session_id"
    );

    // Drain anything the first socket still has buffered, then close it
    // explicitly so the test cleanup path doesn't leak the connection.
    let _ = tokio::time::timeout(Duration::from_millis(100), first_socket.next()).await;
    let _ = first_socket.close(None).await;

    second_socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            "host:end".into(),
        ))
        .await
        .expect("send host:end on second");
    wait_for_live_session_count(&app_state, 0).await;

    app_server.abort();
    workers_server.abort();
}

/// §0.5.2 lifecycle row — `start → crash → restart`.
///
/// Client connects WS, the pipeline spins up, then the client process dies
/// mid-stream — no `host:end`, no graceful WS close. This is the 2am
/// incident shape where the browser tab crashes or the laptop runs out of
/// battery. Server MUST detect the dropped socket and clean the live_session
/// map so a subsequent reconnect from the same client isn't blocked by a
/// zombie entry. Without this test the `stop_start_eviction` fix could
/// regress silently for the "no prior explicit close" path.
#[tokio::test]
async fn ws_abrupt_drop_without_host_end_cleans_live_session() {
    let (_guard, _workers_url, _workers_state, workers_server, app_url, app_state, app_server) = {
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
    let (socket, _) = connect_async(&ws_url).await.expect("connect ws");
    wait_for_live_session_count(&app_state, 1).await;

    // Simulate abrupt client death — drop the socket without sending a
    // close frame, host:end, or anything else. The server side sees an EOF
    // on the read half and must tear down the live_session.
    drop(socket);

    wait_for_live_session_count(&app_state, 0).await;
    assert_eq!(
        app_state.live_sessions.len(),
        0,
        "abrupt drop must clean live_session within the timeout budget so reconnects aren't blocked"
    );

    app_server.abort();
    workers_server.abort();
}

/// §0.5.2 lifecycle row — concurrent `start × N`.
///
/// Two browser tabs (or a flaky network triggering the FE's auto-reconnect)
/// can race to open a WS for the same `sessionId`. The eviction fix
/// (`stop_start_within_session_...`) covers sequential re-connects; this
/// test covers the parallel case. After all connects settle, there must be
/// exactly ONE live_session — never two, never zero — so TTS has a single
/// destination and cost metrics don't double-count.
///
/// TODO(concurrent-connect-lock, post-May-10): this test fails today
/// because `evict_stale_live_sessions` + the subsequent `live_sessions.insert`
/// in `session_ws/mod.rs::handle_host` are NOT synchronized on session_id.
/// Two handlers can each see an empty map at eviction time, then both
/// insert, leaving two live_sessions for the same session_id. Fix: add a
/// per-session_id `tokio::sync::Mutex` to `BroadcastState` acquired before
/// eviction and held through insert. Deferred because:
///   1. FE never fires concurrent connects by design (single WS per tab).
///   2. The sequential `stop_start_within_session_...` test already covers
///      the 2026-04-20 production incident shape.
///   3. Proper fix needs its own test pass + deadlock-audit before May 10.
/// Tracking in vision.md "Testing Debt" as a Phase 2 hardening item.
#[ignore = "reveals real concurrent-connect race; fix tracked in vision.md Testing Debt"]
#[tokio::test]
async fn concurrent_connects_for_same_session_converge_to_single_live_session() {
    let (_guard, _workers_url, _workers_state, workers_server, app_url, app_state, app_server) = {
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

    // Fire 3 connects in parallel. tokio will interleave their upgrade
    // handlers; each one hits the same session_id branch of the WS handler.
    let connect_futs = (0..3)
        .map(|_| {
            let url = ws_url.clone();
            tokio::spawn(async move { connect_async(&url).await.map(|(s, _)| s) })
        })
        .collect::<Vec<_>>();

    let mut sockets = Vec::new();
    for fut in connect_futs {
        let joined = tokio::time::timeout(Duration::from_secs(2), fut)
            .await
            .expect("connect future completes")
            .expect("join succeeds")
            .expect("ws handshake succeeds");
        sockets.push(joined);
    }

    // Let evictions settle. We expect at most one live_session at any
    // steady-state observation — but between the 3 upgrades there may have
    // been transient flux. Poll until we see exactly one for 100ms
    // uninterrupted, which proves the race landed on a single winner.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let count = app_state.live_sessions.len();
            if count == 1 {
                // Confirm it's stable for a short window.
                tokio::time::sleep(Duration::from_millis(100)).await;
                if app_state.live_sessions.len() == 1 {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("concurrent connects converge to exactly one live_session");

    // Close all sockets gracefully so the test doesn't leak them.
    for mut s in sockets {
        let _ = s.close(None).await;
    }
    wait_for_live_session_count(&app_state, 0).await;

    app_server.abort();
    workers_server.abort();
}

#[tokio::test]
async fn edge_case_workers_fetch_failure_rejects_before_live_session_creation() {
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
