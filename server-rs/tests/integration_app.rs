use futures_util::SinkExt;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::Serialize;
use server_rs::{app_state, build_app};
use std::sync::{Mutex, OnceLock};
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

async fn spawn_app() -> (String, server_rs::AppState, JoinHandle<()>) {
    let state = app_state();
    let app = build_app(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test app");
    let addr = listener.local_addr().expect("local addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve app");
    });
    (format!("http://{}", addr), state, handle)
}

async fn wait_for_room_count(state: &server_rs::AppState, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if state.rooms.len() == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("room count update");
}

#[tokio::test]
async fn root_route_returns_banner() {
    let (base_url, _state, server) = spawn_app().await;

    let body = reqwest::get(&base_url)
        .await
        .expect("get /")
        .text()
        .await
        .expect("body text");

    assert_eq!(body, "Brivva Translation Server (media-only)");
    server.abort();
}

#[tokio::test]
async fn websocket_without_token_is_rejected_and_never_creates_room() {
    let (_guard, base_url, state, server) = {
        let guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::remove_var("JWT_SECRET");
        }
        let (base_url, state, server) = spawn_app().await;
        (guard, base_url, state, server)
    };

    let ws_url = format!(
        "{}/api/room?sourceLang=en",
        base_url.replacen("http", "ws", 1)
    );
    let (_socket, _) = connect_async(&ws_url).await.expect("connect ws");

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(state.rooms.len(), 0);

    server.abort();
}

#[tokio::test]
async fn websocket_with_valid_token_uses_default_en_for_invalid_source_lang() {
    let (_guard, base_url, state, server) = {
        let guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("JWT_SECRET", "integration-secret");
        }
        let (base_url, state, server) = spawn_app().await;
        (guard, base_url, state, server)
    };

    let token = make_token("integration-secret", "host-1");
    let ws_url = format!(
        "{}/api/room?token={}&sourceLang=fr",
        base_url.replacen("http", "ws", 1),
        token
    );
    let (mut socket, _) = connect_async(&ws_url).await.expect("connect ws");

    wait_for_room_count(&state, 1).await;
    let room = state.rooms.iter().next().expect("room exists");
    assert_eq!(room.source_lang, server_rs::types::Lang::En);
    drop(room);

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            "host:end".into(),
        ))
        .await
        .expect("send host:end");
    wait_for_room_count(&state, 0).await;

    server.abort();
}
