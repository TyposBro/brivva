//! ElevenLabs WebSocket streaming TTS.

use std::ops::ControlFlow;
use std::time::Instant;
use tracing::{debug, warn};

use super::config::{ElevenLabsTtsResponse, CHUNK_LENGTH_SCHEDULE};
use super::SynthesisRequest;

pub async fn do_tts_ws(req: &SynthesisRequest<'_>) -> Result<usize, String> {
    let (total, got_audio) = do_tts_ws_once(req).await?;

    if !got_audio {
        warn!("[TTS:{}] 0 audio bytes on first attempt (cold start), retrying", req.lang);
        let (retry_total, retry_got_audio) = do_tts_ws_once(req).await?;
        if !retry_got_audio {
            warn!("[TTS:{}] 0 audio bytes on retry, falling back to REST", req.lang);
            return Err("WS returned 0 audio bytes after retry".to_string());
        }
        return Ok(retry_total);
    }

    Ok(total)
}

async fn do_tts_ws_once(req: &SynthesisRequest<'_>) -> Result<(usize, bool), String> {
    let mut ws = connect_elevenlabs(req).await?;
    let tts_start = Instant::now();

    send_bos(&mut ws, req.voice_settings, req.api_key).await?;
    send_text_and_eos(&mut ws, req.text).await?;

    let recv_ctx = WsRecvContext {
        lang: req.lang,
        max_bytes: req.max_bytes,
        streaming: req.streaming,
        tts_start: &tts_start,
    };
    receive_and_decode_chunks(&mut ws, &recv_ctx).await
}

// ── Types ───

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

struct WsRecvContext<'a> {
    lang: &'a str,
    max_bytes: usize,
    streaming: Option<&'a crate::features::broadcast::data::streaming::StreamingPcm>,
    tts_start: &'a Instant,
}

struct DecodeState {
    decoder: Option<crate::features::broadcast::data::streaming::IncrementalMp3Decoder>,
    chunk_state: ChunkState,
}

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

async fn connect_elevenlabs(req: &SynthesisRequest<'_>) -> Result<WsStream, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let connect_start = Instant::now();
    let url = build_ws_url(req);
    let request = url.into_client_request()
        .map_err(|e| format!("WS request build failed: {}", e))?;
    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("WS connect failed: {}", e))?;
    debug!("[TTS:{}] connected in {}ms", req.lang, connect_start.elapsed().as_millis());
    Ok(ws)
}

fn build_ws_url(req: &SynthesisRequest<'_>) -> String {
    format!(
        "wss://api.elevenlabs.io/v1/text-to-speech/{}/stream-input?model_id={}&output_format=mp3_44100_128&language_code={}",
        req.voice_id, req.model_id, req.lang
    )
}

// ── Sending ───

async fn send_bos(ws: &mut WsStream, voice_settings: &serde_json::Value, api_key: &str) -> Result<(), String> {
    use futures_util::SinkExt;

    let bos = serde_json::json!({
        "text": " ",
        "xi_api_key": api_key,
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
    recv_ctx: &WsRecvContext<'_>,
) -> Result<(usize, bool), String> {
    let decoder = crate::features::broadcast::data::streaming::IncrementalMp3Decoder::new().await
        .map_err(|e| format!("IncrementalMp3Decoder init failed: {}", e))?;
    let mut decode = DecodeState {
        decoder: Some(decoder),
        chunk_state: ChunkState::new(),
    };

    receive_loop(ws, &mut decode, recv_ctx).await?;
    drain_remaining(&mut decode, recv_ctx).await?;

    Ok((decode.chunk_state.total_pcm_bytes, decode.chunk_state.got_audio))
}

async fn receive_loop(
    ws: &mut WsStream,
    decode: &mut DecodeState,
    recv_ctx: &WsRecvContext<'_>,
) -> Result<(), String> {
    use futures_util::StreamExt;

    while let Some(msg_result) = ws.next().await {
        let msg = msg_result.map_err(|e| format!("WS read error: {}", e))?;
        let flow = process_ws_message(msg, decode, recv_ctx).await?;
        if flow.is_break() { break; }
    }
    Ok(())
}

async fn process_ws_message(
    msg: tokio_tungstenite::tungstenite::Message,
    decode: &mut DecodeState,
    recv_ctx: &WsRecvContext<'_>,
) -> Result<ControlFlow<()>, String> {
    let text_data = match extract_text_payload(msg, recv_ctx.lang)? {
        Some(t) => t,
        None => return Ok(ControlFlow::Continue(())),
    };

    let resp = validate_and_parse(&text_data)?;

    if is_final(&resp, &decode.chunk_state, recv_ctx) {
        return Ok(ControlFlow::Break(()));
    }

    handle_audio_chunk(&resp, decode, recv_ctx).await?;
    Ok(ControlFlow::Continue(()))
}

fn validate_and_parse(text_data: &str) -> Result<ElevenLabsTtsResponse, String> {
    check_elevenlabs_error(text_data)?;
    parse_response(text_data)
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

fn is_final(resp: &ElevenLabsTtsResponse, state: &ChunkState, recv_ctx: &WsRecvContext<'_>) -> bool {
    if resp.is_final.unwrap_or(false) {
        debug!("[TTS:{}] done: {} chunks, {}KB PCM in {}ms", recv_ctx.lang, state.chunk_count, state.total_pcm_bytes / 1024, recv_ctx.tts_start.elapsed().as_millis());
        return true;
    }
    false
}

async fn handle_audio_chunk(
    resp: &ElevenLabsTtsResponse,
    decode: &mut DecodeState,
    recv_ctx: &WsRecvContext<'_>,
) -> Result<(), String> {
    let audio_b64 = match resp.audio.as_deref() {
        Some(b) if !b.is_empty() => b,
        _ => return Ok(()),
    };

    let (pcm, _is_first) = decode_audio_chunk(decode, audio_b64, recv_ctx).await?;
    decode.chunk_state.accumulate(&pcm);
    append_to_stream(&pcm, recv_ctx);
    Ok(())
}

fn append_to_stream(pcm: &[u8], recv_ctx: &WsRecvContext<'_>) {
    if !pcm.is_empty() {
        if let Some(s) = recv_ctx.streaming {
            s.append_with_limit(pcm, recv_ctx.max_bytes);
        }
    }
}

async fn drain_remaining(
    decode: &mut DecodeState,
    recv_ctx: &WsRecvContext<'_>,
) -> Result<(), String> {
    let decoder = decode.decoder.take()
        .ok_or_else(|| "Decoder already consumed".to_string())?;
    let remaining = drain_decoder(decoder).await?;
    decode.chunk_state.accumulate(&remaining);
    append_to_stream(&remaining, recv_ctx);
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
    decode: &mut DecodeState,
    audio_b64: &str,
    recv_ctx: &WsRecvContext<'_>,
) -> Result<(Vec<u8>, bool), String> {
    let mp3_chunk = decode_base64(audio_b64)?;
    let is_first = advance_chunk_counter(&mut decode.chunk_state.chunk_count);
    log_ttfb_if_first(is_first, recv_ctx.lang, recv_ctx.tts_start, mp3_chunk.len());

    let decoder = decode.decoder.as_mut()
        .ok_or_else(|| "Decoder already consumed".to_string())?;
    let pcm_chunk = decoder.feed(&mp3_chunk).await
        .map_err(|e| format!("Incremental decode failed: {}", e))?;
    Ok((pcm_chunk, is_first))
}

fn decode_base64(audio_b64: &str) -> Result<Vec<u8>, String> {
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, audio_b64)
        .map_err(|e| format!("base64 decode error: {}", e))
}

fn advance_chunk_counter(chunk_count: &mut u32) -> bool {
    *chunk_count += 1;
    *chunk_count == 1
}

fn log_ttfb_if_first(is_first: bool, lang: &str, tts_start: &Instant, mp3_len: usize) {
    if is_first {
        debug!("[TTS:{}] TTFB {}ms ({}B first MP3 chunk)", lang, tts_start.elapsed().as_millis(), mp3_len);
    }
}

async fn drain_decoder(decoder: crate::features::broadcast::data::streaming::IncrementalMp3Decoder) -> Result<Vec<u8>, String> {
    decoder.finish().await
        .map_err(|e| format!("Decoder finish failed: {}", e))
}
