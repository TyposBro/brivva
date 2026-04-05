//! Gladia session creation and WebSocket connection.

use std::time::Duration;
use tokio_tungstenite::tungstenite;
use tracing::{info, error};

use crate::constants::{SAMPLE_RATE, STT_RECONNECT_MAX, STT_RECONNECT_DELAY_SECS};
use crate::stt::config::INITIAL_CONNECT_MAX_ATTEMPTS;
use crate::types::{Lang, Sessions};

use super::state::WsStream;

/// Try to create a Gladia live session and connect the WebSocket.
pub(super) async fn connect_gladia(
    session_id: &str,
    sessions: &Sessions,
    source_lang: &Lang,
    endpointing: f64,
    max_duration: f64,
    reconnect_count: u32,
) -> Option<WsStream> {
    let max_attempts = if reconnect_count == 0 { INITIAL_CONNECT_MAX_ATTEMPTS } else { STT_RECONNECT_MAX };
    let reconnect_delay = Duration::from_secs(STT_RECONNECT_DELAY_SECS);

    for attempt in 1..=max_attempts {
        if !sessions.contains_key(session_id) {
            info!("[STT] Session {} gone, stopping", session_id);
            return None;
        }

        match try_connect(source_lang, endpointing, max_duration, reconnect_count).await {
            Ok((stream, gladia_id)) => {
                info!(
                    "[STT] Connected to Gladia Solaria-1 (attempt {}, endpointing={:.2}s, max_dur={:.0}s, session={})",
                    attempt, endpointing, max_duration, gladia_id
                );
                return Some(stream);
            }
            Err(e) => {
                let delay = if reconnect_count == 0 { Duration::from_secs(3) } else { reconnect_delay };
                error!("[STT] attempt {}/{} failed: {}", attempt, max_attempts, e);
                tokio::time::sleep(delay).await;
            }
        }
    }

    error!("[STT] Failed to connect after {} attempts", max_attempts);
    None
}

async fn try_connect(
    source_lang: &Lang,
    endpointing: f64,
    max_duration: f64,
    _reconnect_count: u32,
) -> Result<(WsStream, String), String> {
    let session = create_gladia_session(&source_lang.to_string(), endpointing, max_duration).await?;
    let stream = connect_websocket(&session.url).await?;
    Ok((stream, session.id))
}

async fn create_gladia_session(
    lang: &str,
    endpointing: f64,
    max_duration: f64,
) -> Result<crate::stt::GladiaSession, String> {
    let body = build_session_body(lang, endpointing, max_duration);
    let resp = post_gladia_session(&body).await?;
    check_response_status(&resp)?;
    parse_session_response(resp).await
}

async fn post_gladia_session(body: &serde_json::Value) -> Result<reqwest::Response, String> {
    crate::HTTP_CLIENT
        .post("https://api.gladia.io/v2/live")
        .header("Content-Type", "application/json")
        .header("x-gladia-key", &*super::super::STT_API_KEY)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Gladia session POST failed: {}", e))
}

fn check_response_status(resp: &reqwest::Response) -> Result<(), String> {
    if resp.status().is_success() { return Ok(()); }
    Err(format!("Gladia error {}", resp.status()))
}

async fn parse_session_response(resp: reqwest::Response) -> Result<crate::stt::GladiaSession, String> {
    resp.json::<crate::stt::GladiaSession>()
        .await
        .map_err(|e| format!("Gladia session parse error: {}", e))
}

fn build_session_body(lang: &str, endpointing: f64, max_duration: f64) -> serde_json::Value {
    serde_json::json!({
        "encoding": "wav/pcm",
        "bit_depth": 16,
        "sample_rate": SAMPLE_RATE,
        "channels": 1,
        "endpointing": endpointing,
        "maximum_duration_without_endpointing": max_duration,
        "language_config": {
            "languages": [lang],
            "code_switching": true
        },
        "messages_config": {
            "receive_partial_transcripts": true,
            "receive_final_transcripts": true,
            "receive_speech_events": true,
            "receive_acknowledgments": false,
            "receive_lifecycle_events": false,
            "receive_pre_processing_events": false,
            "receive_realtime_processing_events": false,
            "receive_post_processing_events": false,
            "receive_errors": true
        },
        "realtime_processing": {
            "words_accurate_timestamps": true
        }
    })
}

async fn connect_websocket(url: &str) -> Result<WsStream, String> {
    use tungstenite::client::IntoClientRequest;
    let request = url.to_string().into_client_request()
        .map_err(|e| format!("Failed to build WS request: {}", e))?;
    let (stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("WS connect failed: {}", e))?;
    Ok(stream)
}
