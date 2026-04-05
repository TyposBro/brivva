//! ElevenLabs WebSocket streaming TTS.

use std::ops::ControlFlow;
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

    let (total, got_audio) = receive_and_decode_chunks(&mut ws, lang, max_bytes, streaming, &tts_start).await?;

    if !got_audio {
        warn!("[TTS:{}] WARNING: stream ended with 0 audio bytes ({}ms)", lang, tts_start.elapsed().as_millis());
    }

    Ok(total)
}

// ── Types ───

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

struct ChunkState {
    chunk_count: u32,
    total_pcm_bytes: usize,
    got_audio: bool,
}

impl ChunkState {
    fn new() -> Self {
        Self { chunk_count: 0, total_pcm_bytes: 0, got_audio: false }
    }

    fn accumulate(&mut self, pcm: &[u8]) {
        if !pcm.is_empty() {
            self.got_audio = true;
            self.total_pcm_bytes += pcm.len();
        }
    }
}

// ── Connection ───

async fn connect_elevenlabs(voice_id: &str, model_id: &str, lang: &str) -> Result<WsStream, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let connect_start = Instant::now();
    let url = build_ws_url(voice_id, model_id, lang);
    let request = url.into_client_request()
        .map_err(|e| format!("WS request build failed: {}", e))?;
    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("WS connect failed: {}", e))?;
    debug!("[TTS:{}] connected in {}ms", lang, connect_start.elapsed().as_millis());
    Ok(ws)
}

fn build_ws_url(voice_id: &str, model_id: &str, lang: &str) -> String {
    format!(
        "wss://api.elevenlabs.io/v1/text-to-speech/{}/stream-input?model_id={}&output_format=mp3_44100_128&language_code={}",
        voice_id, model_id, lang
    )
}

// ── Sending ───

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

// ── Receiving & decoding ───

async fn receive_and_decode_chunks(
    ws: &mut WsStream,
    lang: &str,
    max_bytes: usize,
    streaming: Option<&crate::ffmpeg::StreamingPcm>,
    tts_start: &Instant,
) -> Result<(usize, bool), String> {
    let mut decoder = crate::ffmpeg::IncrementalMp3Decoder::new().await
        .map_err(|e| format!("IncrementalMp3Decoder init failed: {}", e))?;
    let mut state = ChunkState::new();

    receive_loop(ws, &mut decoder, &mut state, lang, max_bytes, streaming, tts_start).await?;
    drain_remaining(&mut state, decoder, max_bytes, streaming).await?;

    Ok((state.total_pcm_bytes, state.got_audio))
}

async fn receive_loop(
    ws: &mut WsStream,
    decoder: &mut crate::ffmpeg::IncrementalMp3Decoder,
    state: &mut ChunkState,
    lang: &str,
    max_bytes: usize,
    streaming: Option<&crate::ffmpeg::StreamingPcm>,
    tts_start: &Instant,
) -> Result<(), String> {
    use futures_util::StreamExt;

    while let Some(msg_result) = ws.next().await {
        let msg = msg_result.map_err(|e| format!("WS read error: {}", e))?;
        let flow = process_ws_message(msg, decoder, state, lang, max_bytes, streaming, tts_start).await?;
        if flow.is_break() { break; }
    }
    Ok(())
}

async fn process_ws_message(
    msg: tokio_tungstenite::tungstenite::Message,
    decoder: &mut crate::ffmpeg::IncrementalMp3Decoder,
    state: &mut ChunkState,
    lang: &str,
    max_bytes: usize,
    streaming: Option<&crate::ffmpeg::StreamingPcm>,
    tts_start: &Instant,
) -> Result<ControlFlow<()>, String> {
    let text_data = match extract_text_payload(msg, lang)? {
        Some(t) => t,
        None => return Ok(ControlFlow::Continue(())),
    };

    check_elevenlabs_error(&text_data)?;
    let resp = parse_response(&text_data)?;

    if is_final(&resp, state, lang, tts_start) {
        return Ok(ControlFlow::Break(()));
    }

    handle_audio_chunk(&resp, decoder, state, lang, max_bytes, streaming, tts_start).await?;
    Ok(ControlFlow::Continue(()))
}

fn extract_text_payload(
    msg: tokio_tungstenite::tungstenite::Message,
    lang: &str,
) -> Result<Option<String>, String> {
    match msg {
        tokio_tungstenite::tungstenite::Message::Text(t) => Ok(Some(t.to_string())),
        tokio_tungstenite::tungstenite::Message::Close(frame) => {
            let reason = frame.map(|f| format!("code={} reason='{}'", f.code, f.reason))
                .unwrap_or_else(|| "no frame".to_string());
            debug!("[TTS:{}] WS closed by server: {}", lang, reason);
            Ok(None)
        }
        _ => Ok(Some(String::new())),
    }
}

fn parse_response(text_data: &str) -> Result<ElevenLabsTtsResponse, String> {
    serde_json::from_str(text_data)
        .map_err(|e| format!("WS parse error: {} | raw: {}", e, &text_data[..text_data.len().min(200)]))
}

fn is_final(resp: &ElevenLabsTtsResponse, state: &ChunkState, lang: &str, tts_start: &Instant) -> bool {
    if resp.is_final.unwrap_or(false) {
        debug!("[TTS:{}] done: {} chunks, {}KB PCM in {}ms", lang, state.chunk_count, state.total_pcm_bytes / 1024, tts_start.elapsed().as_millis());
        return true;
    }
    false
}

async fn handle_audio_chunk(
    resp: &ElevenLabsTtsResponse,
    decoder: &mut crate::ffmpeg::IncrementalMp3Decoder,
    state: &mut ChunkState,
    lang: &str,
    max_bytes: usize,
    streaming: Option<&crate::ffmpeg::StreamingPcm>,
    tts_start: &Instant,
) -> Result<(), String> {
    let audio_b64 = match resp.audio.as_deref() {
        Some(b) if !b.is_empty() => b,
        _ => return Ok(()),
    };

    let (pcm, _is_first) = decode_audio_chunk(decoder, audio_b64, &mut state.chunk_count, lang, tts_start).await?;
    state.accumulate(&pcm);
    append_to_stream(&pcm, max_bytes, streaming);
    Ok(())
}

fn append_to_stream(pcm: &[u8], max_bytes: usize, streaming: Option<&crate::ffmpeg::StreamingPcm>) {
    if !pcm.is_empty() {
        if let Some(s) = streaming {
            s.append_with_limit(pcm, max_bytes);
        }
    }
}

async fn drain_remaining(
    state: &mut ChunkState,
    decoder: crate::ffmpeg::IncrementalMp3Decoder,
    max_bytes: usize,
    streaming: Option<&crate::ffmpeg::StreamingPcm>,
) -> Result<(), String> {
    let remaining = drain_decoder(decoder).await?;
    state.accumulate(&remaining);
    append_to_stream(&remaining, max_bytes, streaming);
    Ok(())
}

// ── Error checking ───

fn check_elevenlabs_error(text_data: &str) -> Result<(), String> {
    if text_data.is_empty() { return Ok(()); }
    let raw = match serde_json::from_str::<serde_json::Value>(text_data) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    check_detail_field(&raw)?;
    check_message_field(&raw)
}

fn check_detail_field(raw: &serde_json::Value) -> Result<(), String> {
    if let Some(detail) = raw.get("detail") {
        return Err(format!("ElevenLabs error: {}", detail));
    }
    Ok(())
}

fn check_message_field(raw: &serde_json::Value) -> Result<(), String> {
    if let Some(msg) = raw.get("message").and_then(|m| m.as_str())
        && raw.get("audio").is_none() {
            return Err(format!("ElevenLabs error: {}", msg));
        }
    Ok(())
}

// ── Decode helpers ───

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
