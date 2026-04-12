//! Pre-warm ElevenLabs WebSocket to avoid cold-start timeout on first utterance.

use std::time::Instant;
use tracing::{info, warn};

/// Fire-and-forget TTS warm-up: opens a WS connection, sends minimal text,
/// drains the response, and closes. This forces ElevenLabs to spin up
/// the inference backend so the first real utterance doesn't time out.
pub async fn warm_up_tts_ws(
    voice_id: &str,
    model_id: &str,
    api_key: &str,
) {
    let start = Instant::now();
    info!("[TTS] warming up WS for voice={}...", &voice_id[..8.min(voice_id.len())]);

    match run_warmup(voice_id, model_id, api_key).await {
        Ok(()) => info!("[TTS] warm-up complete in {}ms", start.elapsed().as_millis()),
        Err(e) => warn!("[TTS] warm-up failed (non-fatal): {}", e),
    }
}

async fn run_warmup(voice_id: &str, model_id: &str, api_key: &str) -> Result<(), String> {
    let mut ws = connect(voice_id, model_id).await?;
    send_warmup_payload(&mut ws, api_key).await?;
    drain_until_final(&mut ws).await?;
    close(&mut ws).await;
    Ok(())
}

// ── Connection ───

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn connect(voice_id: &str, model_id: &str) -> Result<WsStream, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let url = format!(
        "wss://api.elevenlabs.io/v1/text-to-speech/{}/stream-input?model_id={}&output_format=mp3_44100_128",
        voice_id, model_id,
    );
    let request = url
        .into_client_request()
        .map_err(|e| format!("warmup WS request build: {e}"))?;
    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("warmup WS connect: {e}"))?;
    Ok(ws)
}

// ── Sending ───

async fn send_warmup_payload(ws: &mut WsStream, api_key: &str) -> Result<(), String> {
    use futures_util::SinkExt;
    use super::config::CHUNK_LENGTH_SCHEDULE;

    let bos = serde_json::json!({
        "text": " ",
        "xi_api_key": api_key,
        "voice_settings": { "stability": 0.5, "similarity_boost": 0.75 },
        "generation_config": { "chunk_length_schedule": [CHUNK_LENGTH_SCHEDULE] }
    });
    ws.send(msg_text(&serde_json::to_string(&bos).unwrap()))
        .await
        .map_err(|e| format!("warmup BOS: {e}"))?;

    let text_msg = serde_json::json!({ "text": ".", "flush": true });
    ws.send(msg_text(&serde_json::to_string(&text_msg).unwrap()))
        .await
        .map_err(|e| format!("warmup text: {e}"))?;

    ws.send(msg_text(r#"{"text":""}"#))
        .await
        .map_err(|e| format!("warmup EOS: {e}"))?;

    Ok(())
}

fn msg_text(s: &str) -> tokio_tungstenite::tungstenite::Message {
    tokio_tungstenite::tungstenite::Message::Text(s.to_string().into())
}

// ── Receiving (discard audio) ───

async fn drain_until_final(ws: &mut WsStream) -> Result<(), String> {
    use futures_util::StreamExt;

    let timeout = std::time::Duration::from_secs(10);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(t))) => {
                        if is_final_or_error(&t) { break; }
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
                    Some(Err(e)) => return Err(format!("warmup recv: {e}")),
                    None => break,
                    _ => {}
                }
            }
            _ = &mut deadline => {
                warn!("[TTS] warm-up drain timed out at {}s", timeout.as_secs());
                break;
            }
        }
    }
    Ok(())
}

fn is_final_or_error(text: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return false };
    if v.get("isFinal").and_then(|f| f.as_bool()) == Some(true) {
        return true;
    }
    v.get("detail").is_some() || (v.get("message").is_some() && v.get("audio").is_none())
}

// ── Teardown ───

async fn close(ws: &mut WsStream) {
    let _ = ws.close(None).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_detect_final_response() {
        assert!(is_final_or_error(r#"{"isFinal": true}"#));
    }

    #[test]
    fn should_not_detect_non_final_audio_response() {
        assert!(!is_final_or_error(r#"{"audio": "YWJj", "isFinal": false}"#));
    }

    #[test]
    fn should_detect_error_with_detail() {
        assert!(is_final_or_error(r#"{"detail": "quota exceeded"}"#));
    }

    #[test]
    fn should_detect_error_with_message_no_audio() {
        assert!(is_final_or_error(r#"{"message": "invalid voice"}"#));
    }

    #[test]
    fn should_not_flag_message_when_audio_present() {
        assert!(!is_final_or_error(r#"{"audio": "abc", "message": "ok"}"#));
    }

    #[test]
    fn should_handle_invalid_json_gracefully() {
        assert!(!is_final_or_error("not json"));
    }
}
