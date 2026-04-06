//! Soniox v4 WebSocket connection setup.

use std::time::Duration;
use tokio_tungstenite::tungstenite;
use tracing::{info, error};

use crate::core::types::Sessions;

use super::config::{
    SONIOX_CONNECT_MAX_ATTEMPTS, SONIOX_CONNECT_RETRY_DELAY_SECS,
    SONIOX_MODEL, SONIOX_WS_URL,
};
use super::state::WsStream;

pub(super) struct SonioxConfig {
    pub api_key: String,
    pub source_lang: String,
    pub target_lang: Option<String>,
    pub max_endpoint_delay_ms: u64,
    pub sample_rate: u32,
}

pub(super) struct ConnectSession<'a> {
    pub session_id: &'a str,
    pub sessions: &'a Sessions,
}

pub(super) async fn connect_soniox(
    sess: &ConnectSession<'_>,
    config: &SonioxConfig,
) -> Option<WsStream> {
    if check_stt_circuit_breaker(sess) {
        return None;
    }

    let retry_delay = Duration::from_secs(SONIOX_CONNECT_RETRY_DELAY_SECS);

    for attempt in 1..=SONIOX_CONNECT_MAX_ATTEMPTS {
        if !sess.sessions.contains_key(sess.session_id) {
            info!("[STT] Session {} gone, stopping", sess.session_id);
            return None;
        }

        match try_connect(config).await {
            Ok(stream) => {
                record_stt_success(sess);
                info!(
                    "[STT] Connected to Soniox {} (attempt {}, lang={}, target={:?})",
                    SONIOX_MODEL, attempt, config.source_lang, config.target_lang,
                );
                return Some(stream);
            }
            Err(e) => {
                record_stt_failure(sess);
                error!(
                    "[STT] attempt {}/{} failed: {}",
                    attempt, SONIOX_CONNECT_MAX_ATTEMPTS, e,
                );
                tokio::time::sleep(retry_delay).await;
            }
        }
    }

    error!(
        "[STT] Failed to connect after {} attempts",
        SONIOX_CONNECT_MAX_ATTEMPTS,
    );
    None
}

fn check_stt_circuit_breaker(sess: &ConnectSession<'_>) -> bool {
    if let Some(session) = sess.sessions.get(sess.session_id) {
        if session.stt_circuit_breaker.is_open() {
            tracing::warn!("[STT] circuit breaker OPEN for {}, skipping connect", sess.session_id);
            return true;
        }
    }
    false
}

fn record_stt_success(sess: &ConnectSession<'_>) {
    if let Some(session) = sess.sessions.get(sess.session_id) {
        session.stt_circuit_breaker.record_success();
    }
}

fn record_stt_failure(sess: &ConnectSession<'_>) {
    if let Some(session) = sess.sessions.get(sess.session_id) {
        session.stt_circuit_breaker.record_failure();
    }
}

async fn try_connect(config: &SonioxConfig) -> Result<WsStream, String> {
    let mut stream = open_websocket().await?;
    send_config_message(&mut stream, config).await?;
    Ok(stream)
}

async fn open_websocket() -> Result<WsStream, String> {
    use tungstenite::client::IntoClientRequest;

    let request = SONIOX_WS_URL
        .into_client_request()
        .map_err(|e| format!("Failed to build WS request: {}", e))?;

    let (stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("WS connect failed: {}", e))?;

    Ok(stream)
}

async fn send_config_message(
    stream: &mut WsStream,
    config: &SonioxConfig,
) -> Result<(), String> {
    use futures_util::SinkExt;

    let payload = build_config_json(config);
    let text = serde_json::to_string(&payload)
        .map_err(|e| format!("Config serialize failed: {}", e))?;

    stream
        .send(tungstenite::Message::Text(text.into()))
        .await
        .map_err(|e| format!("Config send failed: {}", e))
}

fn build_config_json(config: &SonioxConfig) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "api_key": config.api_key,
        "model": SONIOX_MODEL,
        "audio_format": "pcm_s16le",
        "sample_rate": config.sample_rate,
        "num_channels": 1,
        "language_hints": [config.source_lang],
        "enable_endpoint_detection": true,
        "max_endpoint_delay_ms": config.max_endpoint_delay_ms,
    });

    if let Some(ref target) = config.target_lang {
        payload["translation"] = serde_json::json!({
            "type": "one_way",
            "target_language": target,
        });
    }

    payload
}
