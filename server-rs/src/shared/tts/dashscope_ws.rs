//! DashScope (Qwen3-TTS) WebSocket streaming TTS.

use std::ops::ControlFlow;
use std::time::Instant;
use tracing::{debug, warn};

use super::SynthesisRequest;

pub const DASHSCOPE_TTS_WS_URL: &str =
    "wss://dashscope-intl.aliyuncs.com/api-ws/v1/realtime";
pub const DASHSCOPE_TTS_MODEL_VC: &str = "qwen3-tts-vc-realtime-2026-01-15";

const INPUT_SAMPLE_RATE: f64 = 24_000.0;
const OUTPUT_SAMPLE_RATE: f64 = 44_100.0;

pub async fn do_tts_dashscope(req: &SynthesisRequest<'_>) -> Result<usize, String> {
    let (total, got_audio) = do_tts_dashscope_once(req).await?;

    if !got_audio {
        warn!("[TTS-DS:{}] 0 audio bytes on first attempt, retrying", req.lang);
        let (retry_total, retry_got) = do_tts_dashscope_once(req).await?;
        if !retry_got {
            warn!("[TTS-DS:{}] 0 audio bytes on retry", req.lang);
            return Err("DashScope WS returned 0 audio bytes after retry".into());
        }
        return Ok(retry_total);
    }

    Ok(total)
}

async fn do_tts_dashscope_once(
    req: &SynthesisRequest<'_>,
) -> Result<(usize, bool), String> {
    let mut ws = connect_dashscope(req.api_key, req.model_id).await?;
    let tts_start = Instant::now();

    wait_session_created(&mut ws, req.lang).await?;
    send_session_update(&mut ws, req.voice_id).await?;
    wait_session_updated(&mut ws, req.lang).await?;
    send_text(&mut ws, req.text).await?;
    send_session_finish(&mut ws).await?;

    let recv_ctx = RecvContext {
        lang: req.lang,
        max_bytes: req.max_bytes,
        streaming: req.streaming,
        tts_start: &tts_start,
    };
    receive_audio(&mut ws, &recv_ctx).await
}

// ── Types ───

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

struct RecvContext<'a> {
    lang: &'a str,
    max_bytes: usize,
    streaming: Option<&'a crate::features::broadcast::data::streaming::StreamingPcm>,
    tts_start: &'a Instant,
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

async fn connect_dashscope(api_key: &str, model: &str) -> Result<WsStream, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let connect_start = Instant::now();
    let url = format!("{}?model={}", DASHSCOPE_TTS_WS_URL, model);
    let mut request = url
        .into_client_request()
        .map_err(|e| format!("DashScope WS request build failed: {}", e))?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", api_key)
            .parse()
            .map_err(|e| format!("auth header build failed: {}", e))?,
    );
    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("DashScope WS connect failed: {}", e))?;
    debug!("[TTS-DS] connected to {} in {}ms", model, connect_start.elapsed().as_millis());
    Ok(ws)
}

// ── Handshake: wait for server messages ───

async fn wait_session_created(ws: &mut WsStream, lang: &str) -> Result<(), String> {
    wait_for_message_type(ws, "session.created", lang).await
}

async fn wait_session_updated(ws: &mut WsStream, lang: &str) -> Result<(), String> {
    wait_for_message_type(ws, "session.updated", lang).await
}

async fn wait_for_message_type(
    ws: &mut WsStream,
    expected_type: &str,
    lang: &str,
) -> Result<(), String> {
    use futures_util::StreamExt;

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(10));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            msg = ws.next() => {
                let text = extract_text_from_ws_msg(msg, lang)?;
                let Some(text) = text else { continue };
                warn!("[TTS-DS:{}] handshake recv: {}", lang, &text[..text.len().min(300)]);
                check_dashscope_error(&text)?;
                if message_has_type(&text, expected_type) {
                    return Ok(());
                }
            }
            _ = &mut timeout => {
                return Err(format!("timeout waiting for {}", expected_type));
            }
        }
    }
}

fn message_has_type(text: &str, expected: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("type")?.as_str().map(|t| t == expected))
        .unwrap_or(false)
}

fn extract_text_from_ws_msg(
    msg: Option<Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>>,
    lang: &str,
) -> Result<Option<String>, String> {
    match msg {
        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(t))) => {
            Ok(Some(t.to_string()))
        }
        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(frame))) => {
            let reason = frame
                .map(|f| format!("code={} reason='{}'", f.code, f.reason))
                .unwrap_or_else(|| "no frame".into());
            debug!("[TTS-DS:{}] WS closed by server: {}", lang, reason);
            Err(format!("DashScope WS closed unexpectedly: {}", reason))
        }
        Some(Err(e)) => Err(format!("DashScope WS read error: {}", e)),
        None => Err("DashScope WS stream ended unexpectedly".into()),
        _ => Ok(None),
    }
}

// ── Sending ───

async fn send_session_update(ws: &mut WsStream, voice_id: &str) -> Result<(), String> {
    use futures_util::SinkExt;

    let msg = serde_json::json!({
        "type": "session.update",
        "session": {
            "model": DASHSCOPE_TTS_MODEL_VC,
            "voice": voice_id,
            "response_format": "pcm",
            "mode": "server_commit"
        }
    });
    warn!("[TTS-DS] session.update: {}", serde_json::to_string(&msg).unwrap());
    ws.send(text_msg(&serde_json::to_string(&msg).unwrap()))
        .await
        .map_err(|e| format!("session.update send failed: {}", e))
}

async fn send_text(ws: &mut WsStream, text: &str) -> Result<(), String> {
    use futures_util::SinkExt;

    let msg = serde_json::json!({
        "type": "input_text_buffer.append",
        "delta": text
    });
    ws.send(text_msg(&serde_json::to_string(&msg).unwrap()))
        .await
        .map_err(|e| format!("input_text_buffer.append send failed: {}", e))
}

async fn send_session_finish(ws: &mut WsStream) -> Result<(), String> {
    use futures_util::SinkExt;

    let msg = serde_json::json!({ "type": "session.finish" });
    ws.send(text_msg(&serde_json::to_string(&msg).unwrap()))
        .await
        .map_err(|e| format!("session.finish send failed: {}", e))
}

fn text_msg(s: &str) -> tokio_tungstenite::tungstenite::Message {
    tokio_tungstenite::tungstenite::Message::Text(s.to_string().into())
}

// ── Receiving ───

async fn receive_audio(
    ws: &mut WsStream,
    recv_ctx: &RecvContext<'_>,
) -> Result<(usize, bool), String> {
    let mut state = ChunkState::new();
    receive_loop(ws, &mut state, recv_ctx).await?;
    log_completion(&state, recv_ctx);
    Ok((state.total_pcm_bytes, state.got_audio))
}

async fn receive_loop(
    ws: &mut WsStream,
    state: &mut ChunkState,
    recv_ctx: &RecvContext<'_>,
) -> Result<(), String> {
    use futures_util::StreamExt;

    while let Some(msg_result) = ws.next().await {
        let msg = msg_result.map_err(|e| format!("DashScope WS read error: {}", e))?;
        let flow = process_message(msg, state, recv_ctx)?;
        if flow.is_break() {
            break;
        }
    }
    Ok(())
}

fn process_message(
    msg: tokio_tungstenite::tungstenite::Message,
    state: &mut ChunkState,
    recv_ctx: &RecvContext<'_>,
) -> Result<ControlFlow<()>, String> {
    let text = match msg {
        tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
        tokio_tungstenite::tungstenite::Message::Close(_) => return Ok(ControlFlow::Break(())),
        _ => return Ok(ControlFlow::Continue(())),
    };

    // Log all non-audio messages for debugging
    let msg_type = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(String::from));
    if msg_type.as_deref() != Some("response.audio.delta") {
        warn!("[TTS-DS:{}] recv: {}", recv_ctx.lang, &text[..text.len().min(300)]);
    }

    check_dashscope_error(&text)?;
    let parsed = parse_message(&text)?;

    if is_terminal(&parsed) {
        return Ok(ControlFlow::Break(()));
    }

    handle_audio_delta(&parsed, state, recv_ctx)?;
    Ok(ControlFlow::Continue(()))
}

fn parse_message(text: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(text)
        .map_err(|e| format!("DashScope parse error: {} | raw: {}", e, &text[..text.len().min(200)]))
}

fn is_terminal(msg: &serde_json::Value) -> bool {
    matches!(
        msg.get("type").and_then(|t| t.as_str()),
        Some("response.done" | "session.finished")
    )
}

fn handle_audio_delta(
    msg: &serde_json::Value,
    state: &mut ChunkState,
    recv_ctx: &RecvContext<'_>,
) -> Result<(), String> {
    let is_audio_delta = msg.get("type").and_then(|t| t.as_str()) == Some("response.audio.delta");
    if !is_audio_delta {
        return Ok(());
    }

    let b64 = match msg.get("delta").and_then(|d| d.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(()),
    };

    let pcm_24k = decode_base64(b64)?;
    log_ttfb_if_first(state, recv_ctx);
    let pcm_44k = resample_24k_to_44k(&pcm_24k);
    state.accumulate(&pcm_44k);
    append_to_stream(&pcm_44k, recv_ctx);
    Ok(())
}

fn append_to_stream(pcm: &[u8], recv_ctx: &RecvContext<'_>) {
    if !pcm.is_empty() {
        if let Some(s) = recv_ctx.streaming {
            s.append_with_limit(pcm, recv_ctx.max_bytes);
        }
    }
}

// ── Error checking ───

fn check_dashscope_error(text: &str) -> Result<(), String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Ok(());
    };
    if let Some(err) = v.get("error") {
        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown");
        let code = err.get("code").and_then(|c| c.as_str()).unwrap_or("unknown");
        return Err(format!("DashScope error [{}]: {}", code, msg));
    }
    Ok(())
}

// ── Decode & resample helpers ───

fn decode_base64(b64: &str) -> Result<Vec<u8>, String> {
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
        .map_err(|e| format!("base64 decode error: {}", e))
}

/// Resample 24kHz 16-bit mono PCM to 44.1kHz using linear interpolation.
///
/// Input and output are raw PCM bytes (i16 little-endian samples).
pub fn resample_24k_to_44k(input: &[u8]) -> Vec<u8> {
    let input_samples = to_i16_samples(input);
    if input_samples.is_empty() {
        return Vec::new();
    }
    let output_samples = interpolate(&input_samples);
    from_i16_samples(&output_samples)
}

fn to_i16_samples(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

fn interpolate(input: &[i16]) -> Vec<i16> {
    let ratio = OUTPUT_SAMPLE_RATE / INPUT_SAMPLE_RATE;
    let output_len = ((input.len() as f64) * ratio).ceil() as usize;
    let last_idx = (input.len() - 1) as f64;

    (0..output_len)
        .map(|i| interpolate_sample(input, i as f64 / ratio, last_idx))
        .collect()
}

fn interpolate_sample(input: &[i16], pos: f64, last_idx: f64) -> i16 {
    let clamped = pos.min(last_idx);
    let idx = clamped as usize;
    let frac = clamped - idx as f64;

    if idx + 1 < input.len() {
        let a = input[idx] as f64;
        let b = input[idx + 1] as f64;
        (a + frac * (b - a)) as i16
    } else {
        input[idx]
    }
}

fn from_i16_samples(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

// ── Logging ───

fn log_ttfb_if_first(state: &mut ChunkState, recv_ctx: &RecvContext<'_>) {
    state.chunk_count += 1;
    if state.chunk_count == 1 {
        debug!(
            "[TTS-DS:{}] TTFB {}ms",
            recv_ctx.lang,
            recv_ctx.tts_start.elapsed().as_millis()
        );
    }
}

fn log_completion(state: &ChunkState, recv_ctx: &RecvContext<'_>) {
    debug!(
        "[TTS-DS:{}] done: {} chunks, {}KB PCM in {}ms",
        recv_ctx.lang,
        state.chunk_count,
        state.total_pcm_bytes / 1024,
        recv_ctx.tts_start.elapsed().as_millis()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── resample_24k_to_44k ──

    #[test]
    fn should_return_empty_for_empty_input() {
        assert!(resample_24k_to_44k(&[]).is_empty());
    }

    #[test]
    fn should_upsample_by_correct_ratio() {
        let input_samples = 100;
        let input = vec![0u8; input_samples * 2];

        let output = resample_24k_to_44k(&input);
        let output_samples = output.len() / 2;

        let expected = ((input_samples as f64) * 44100.0 / 24000.0).ceil() as usize;
        assert_eq!(output_samples, expected);
    }

    #[test]
    fn should_preserve_silence() {
        let input = vec![0u8; 200];

        let output = resample_24k_to_44k(&input);

        let samples = to_i16_samples(&output);
        assert!(samples.iter().all(|&s| s == 0));
    }

    #[test]
    fn should_preserve_dc_offset() {
        let dc_value: i16 = 1000;
        let mut input = Vec::new();
        for _ in 0..50 {
            input.extend_from_slice(&dc_value.to_le_bytes());
        }

        let output = resample_24k_to_44k(&input);

        let samples = to_i16_samples(&output);
        assert!(samples.iter().all(|&s| s == dc_value));
    }

    #[test]
    fn should_handle_single_sample() {
        let input = 12345i16.to_le_bytes().to_vec();

        let output = resample_24k_to_44k(&input);

        let samples = to_i16_samples(&output);
        assert!(!samples.is_empty());
        assert_eq!(samples[0], 12345);
    }

    #[test]
    fn should_ignore_trailing_odd_byte() {
        let input = vec![0u8; 5]; // 2 complete samples + 1 trailing byte

        let output = resample_24k_to_44k(&input);
        let output_samples = output.len() / 2;

        let expected = ((2.0_f64) * 44100.0 / 24000.0).ceil() as usize;
        assert_eq!(output_samples, expected);
    }

    #[test]
    fn should_output_even_number_of_bytes() {
        let input = vec![0u8; 48]; // 24 samples

        let output = resample_24k_to_44k(&input);

        assert_eq!(output.len() % 2, 0);
    }

    // ── interpolate_sample ──

    #[test]
    fn should_interpolate_midpoint() {
        let input: Vec<i16> = vec![0, 1000];

        let result = interpolate_sample(&input, 0.5, 1.0);

        assert_eq!(result, 500);
    }

    #[test]
    fn should_return_exact_value_at_integer_position() {
        let input: Vec<i16> = vec![100, 200, 300];

        assert_eq!(interpolate_sample(&input, 0.0, 2.0), 100);
        assert_eq!(interpolate_sample(&input, 1.0, 2.0), 200);
        assert_eq!(interpolate_sample(&input, 2.0, 2.0), 300);
    }

    // ── to_i16_samples / from_i16_samples roundtrip ──

    #[test]
    fn should_roundtrip_i16_conversion() {
        let original: Vec<i16> = vec![-32768, 0, 32767, -1, 1];
        let bytes = from_i16_samples(&original);
        let recovered = to_i16_samples(&bytes);

        assert_eq!(original, recovered);
    }

    // ── message_has_type ──

    #[test]
    fn should_match_message_type() {
        let msg = r#"{"type": "session.created", "session": {}}"#;
        assert!(message_has_type(msg, "session.created"));
    }

    #[test]
    fn should_not_match_wrong_type() {
        let msg = r#"{"type": "session.created"}"#;
        assert!(!message_has_type(msg, "session.updated"));
    }

    #[test]
    fn should_handle_invalid_json_in_type_check() {
        assert!(!message_has_type("not json", "session.created"));
    }

    // ── check_dashscope_error ──

    #[test]
    fn should_pass_non_error_messages() {
        let msg = r#"{"type": "response.audio.delta", "delta": "abc"}"#;
        assert!(check_dashscope_error(msg).is_ok());
    }

    #[test]
    fn should_detect_error_response() {
        let msg = r#"{"error": {"code": "InvalidParameter", "message": "bad voice"}}"#;
        let result = check_dashscope_error(msg);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bad voice"));
    }

    #[test]
    fn should_handle_invalid_json_gracefully() {
        assert!(check_dashscope_error("not json").is_ok());
    }

    // ── is_terminal ──

    #[test]
    fn should_detect_response_done() {
        let msg = serde_json::json!({"type": "response.done"});
        assert!(is_terminal(&msg));
    }

    #[test]
    fn should_detect_session_finished() {
        let msg = serde_json::json!({"type": "session.finished"});
        assert!(is_terminal(&msg));
    }

    #[test]
    fn should_not_treat_audio_delta_as_terminal() {
        let msg = serde_json::json!({"type": "response.audio.delta"});
        assert!(!is_terminal(&msg));
    }
}
