//! ElevenLabs WebSocket streaming TTS.

use std::time::Instant;
use tracing::{debug, warn};

use super::config::{TTS_API_KEY, ElevenLabsTtsResponse, CHUNK_LENGTH_SCHEDULE};

pub async fn do_tts_ws(
    text: &str,
    voice_id: &str,
    lang: &str,
    voice_settings: &serde_json::Value,
    max_bytes: usize,
    streaming: Option<&crate::ffmpeg::StreamingPcm>,
    model_id: &str,
) -> Result<usize, String> {
    let mut ws = connect_elevenlabs(voice_id, model_id, lang).await?;
    let tts_start = Instant::now();

    send_bos(&mut ws, voice_settings).await?;
    send_text_and_eos(&mut ws, text).await?;

    let (total_pcm_bytes, got_audio) = receive_and_decode_chunks(&mut ws, lang, max_bytes, streaming, &tts_start).await?;

    if !got_audio {
        warn!("[TTS:{}] WARNING: stream ended with 0 audio bytes ({}ms)", lang, tts_start.elapsed().as_millis());
    }

    Ok(total_pcm_bytes)
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn connect_elevenlabs(voice_id: &str, model_id: &str, lang: &str) -> Result<WsStream, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let connect_start = Instant::now();
    let url = format!(
        "wss://api.elevenlabs.io/v1/text-to-speech/{}/stream-input?model_id={}&output_format=mp3_44100_128&language_code={}",
        voice_id, model_id, lang
    );
    let request = url.into_client_request()
        .map_err(|e| format!("WS request build failed: {}", e))?;
    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("WS connect failed: {}", e))?;
    debug!("[TTS:{}] connected in {}ms", lang, connect_start.elapsed().as_millis());
    Ok(ws)
}

async fn send_bos(ws: &mut WsStream, voice_settings: &serde_json::Value) -> Result<(), String> {
    use futures_util::SinkExt;

    let bos = serde_json::json!({
        "text": " ",
        "xi_api_key": &*TTS_API_KEY,
        "voice_settings": voice_settings,
        "generation_config": { "chunk_length_schedule": [CHUNK_LENGTH_SCHEDULE] }
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&bos).unwrap().into()))
        .await.map_err(|e| format!("BOS send failed: {}", e))
}

async fn send_text_and_eos(ws: &mut WsStream, text: &str) -> Result<(), String> {
    use futures_util::SinkExt;

    let text_msg = serde_json::json!({ "text": text, "flush": true });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&text_msg).unwrap().into()))
        .await.map_err(|e| format!("text send failed: {}", e))?;
    ws.send(tokio_tungstenite::tungstenite::Message::Text(r#"{"text":""}"#.to_string().into()))
        .await.map_err(|e| format!("EOS send failed: {}", e))
}

async fn receive_and_decode_chunks(
    ws: &mut WsStream,
    lang: &str,
    max_bytes: usize,
    streaming: Option<&crate::ffmpeg::StreamingPcm>,
    tts_start: &Instant,
) -> Result<(usize, bool), String> {
    use futures_util::StreamExt;

    let mut decoder = crate::ffmpeg::IncrementalMp3Decoder::new().await
        .map_err(|e| format!("IncrementalMp3Decoder init failed: {}", e))?;
    let mut chunk_count: u32 = 0;
    let mut total_pcm_bytes: usize = 0;
    let mut got_audio = false;

    while let Some(msg_result) = ws.next().await {
        let msg = msg_result.map_err(|e| format!("WS read error: {}", e))?;

        let text_data = match msg {
            tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
            tokio_tungstenite::tungstenite::Message::Close(frame) => {
                let reason = frame.map(|f| format!("code={} reason='{}'", f.code, f.reason))
                    .unwrap_or_else(|| "no frame".to_string());
                debug!("[TTS:{}] WS closed by server: {}", lang, reason);
                break;
            }
            _ => continue,
        };

        check_elevenlabs_error(&text_data)?;

        let resp: ElevenLabsTtsResponse = serde_json::from_str(&text_data)
            .map_err(|e| format!("WS parse error: {} | raw: {}", e, &text_data[..text_data.len().min(200)]))?;

        if resp.is_final.unwrap_or(false) {
            debug!("[TTS:{}] done: {} chunks, {}KB PCM in {}ms", lang, chunk_count, total_pcm_bytes / 1024, tts_start.elapsed().as_millis());
            break;
        }

        if let Some(audio_b64) = resp.audio {
            if audio_b64.is_empty() { continue; }
            let (pcm_bytes, _is_first) = decode_audio_chunk(&mut decoder, &audio_b64, &mut chunk_count, lang, tts_start).await?;
            if !pcm_bytes.is_empty() {
                got_audio = true;
                total_pcm_bytes += pcm_bytes.len();
                if let Some(s) = streaming {
                    s.append_with_limit(&pcm_bytes, max_bytes);
                }
            }
        }
    }

    let remaining = drain_decoder(decoder).await?;
    if !remaining.is_empty() {
        got_audio = true;
        total_pcm_bytes += remaining.len();
        if let Some(s) = streaming {
            s.append_with_limit(&remaining, max_bytes);
        }
    }

    Ok((total_pcm_bytes, got_audio))
}

fn check_elevenlabs_error(text_data: &str) -> Result<(), String> {
    if let Ok(raw) = serde_json::from_str::<serde_json::Value>(text_data) {
        if let Some(detail) = raw.get("detail") {
            return Err(format!("ElevenLabs error: {}", detail));
        }
        if let Some(msg) = raw.get("message").and_then(|m| m.as_str())
            && raw.get("audio").is_none() {
                return Err(format!("ElevenLabs error: {}", msg));
            }
    }
    Ok(())
}

async fn decode_audio_chunk(
    decoder: &mut crate::ffmpeg::IncrementalMp3Decoder,
    audio_b64: &str,
    chunk_count: &mut u32,
    lang: &str,
    tts_start: &Instant,
) -> Result<(Vec<u8>, bool), String> {
    let mp3_chunk = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        audio_b64,
    ).map_err(|e| format!("base64 decode error: {}", e))?;

    *chunk_count += 1;
    let is_first = *chunk_count == 1;
    if is_first {
        debug!("[TTS:{}] TTFB {}ms ({}B first MP3 chunk)", lang, tts_start.elapsed().as_millis(), mp3_chunk.len());
    }

    let pcm_chunk = decoder.feed(&mp3_chunk).await
        .map_err(|e| format!("Incremental decode failed: {}", e))?;
    Ok((pcm_chunk, is_first))
}

async fn drain_decoder(decoder: crate::ffmpeg::IncrementalMp3Decoder) -> Result<Vec<u8>, String> {
    decoder.finish().await
        .map_err(|e| format!("Decoder finish failed: {}", e))
}
