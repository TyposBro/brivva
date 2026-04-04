//! ElevenLabs TTS integration — WebSocket streaming + REST fallback.

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tracing::{info, warn, error, debug};

use crate::constants::{
    BYTES_PER_SEC, DEFAULT_VOICE_ID,
    TTS_DEADLINE_CAP_MS, TTS_DEADLINE_MARGIN_MS, DEFAULT_BROADCAST_DELAY_MS,
};
use crate::types::{Lang, Sessions, ServerMsg};

// ── Service Keys ──────────────────────────────────────────

/// TTS provider API key (ElevenLabs)
pub(crate) static TTS_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("TTS_API_KEY").unwrap_or_default()
});

/// Default ElevenLabs voice. Rachel — multilingual, works with all models.
/// Override with DEFAULT_VOICE env var.
pub static DEFAULT_VOICE: LazyLock<String> = LazyLock::new(|| {
    std::env::var("DEFAULT_VOICE")
        .unwrap_or_else(|_| DEFAULT_VOICE_ID.to_string())
});

// ── TTS Style Params ─────────────────────────────────────

/// TTS style parameters mapped from prosody/emotion analysis.
/// Used by ElevenLabs voice_settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleParams {
    #[serde(default = "default_speed")]
    pub speed: f64,
    #[serde(default = "default_emotion")]
    pub emotion: String,
}

fn default_speed() -> f64 { 1.0 }
fn default_emotion() -> String { "neutral".to_string() }

impl Default for StyleParams {
    fn default() -> Self {
        Self { speed: 1.0, emotion: "neutral".to_string() }
    }
}

// ── ElevenLabs Response Types ────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct ElevenLabsTtsResponse {
    #[serde(default)]
    audio: Option<String>,
    #[serde(default, rename = "isFinal")]
    is_final: Option<bool>,
}

// ── Main TTS Entry Point ─────────────────────────────────

/// ElevenLabs Turbo v2.5 TTS — WebSocket streaming for low-latency output.
/// Streams raw PCM s16le 44100Hz chunks directly to RTMP as they arrive.
/// Falls back to REST if WebSocket connection fails.
pub async fn do_tts(
    client: &reqwest::Client,
    text: &str,
    utterance_id: u64,
    lang: &Lang,
    sessions: &Sessions,
    session_id: &str,
    voice_clone_id: Option<&str>,
    style_params: &StyleParams,
    utterance_start: Instant,
    utterance_end: Instant,
    tts_model: &str,
) {
    let tts_start = Instant::now();

    let broadcast_delay_ms = sessions.get(session_id)
        .map(|s| s.broadcast_delay_ms)
        .unwrap_or(DEFAULT_BROADCAST_DELAY_MS);
    let tts_deadline = {
        let sync_deadline = Duration::from_millis(
            broadcast_delay_ms.saturating_sub(TTS_DEADLINE_MARGIN_MS)
        );
        let hard_cap = Duration::from_millis(TTS_DEADLINE_CAP_MS);
        sync_deadline.min(hard_cap)
    };

    let voice_id = match voice_clone_id {
        Some(id) => id.to_string(),
        None => DEFAULT_VOICE.clone(),
    };

    let is_cloned = voice_clone_id.is_some();
    let lang_str = lang.to_string();

    // Map emotion -> ElevenLabs voice_settings via prosody mapping
    let (stability, similarity_boost, style, _) = crate::stt::map_style(&style_params.emotion);
    let voice_settings = serde_json::json!({
        "stability": stability,
        "similarity_boost": similarity_boost,
        "style": style,
        "speed": style_params.speed,
    });

    info!(
        "[TTS] elevenlabs WS voice={}{} lang={} emotion={} speed={:.2} text='{}' [deadline={}ms]",
        &voice_id[..8.min(voice_id.len())],
        if is_cloned { " (cloned)" } else { "" },
        lang, style_params.emotion, style_params.speed, text,
        tts_deadline.as_millis()
    );

    // Calculate max PCM bytes for this utterance (duration + 2s tolerance)
    let utterance_dur = utterance_end.duration_since(utterance_start);
    let max_dur = utterance_dur + Duration::from_millis(2000);
    let max_bytes = (max_dur.as_secs_f64() * BYTES_PER_SEC) as usize;

    // Queue streaming audio slot to RTMP immediately (before TTS starts)
    let rtmp_mgr = sessions.get(session_id).and_then(|s| s.rtmp_manager.clone());
    let streaming = if let Some(ref manager) = rtmp_mgr {
        let mgr = manager.lock().await;
        let s = mgr.queue_streaming_audio(&lang_str, utterance_start);
        debug!(
            "[TTS] #{} {} queued streaming audio slot to RTMP (max_bytes={}B = {:.1}s)",
            utterance_id, lang_str, max_bytes, max_bytes as f64 / BYTES_PER_SEC
        );
        Some(s)
    } else {
        debug!("[TTS] #{} {} no RTMP manager, audio won't be streamed", utterance_id, lang_str);
        None
    };

    // Notify host: TTS started
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(crate::pipeline::to_ws(&ServerMsg::TtsStart { lang: lang_str.clone(), utterance_id }));
    }

    // Try WebSocket streaming, fall back to REST
    let tts_result = tokio::time::timeout(tts_deadline, async {
        match do_tts_ws(
            text, &voice_id, &lang_str, &voice_settings, max_bytes, streaming.as_ref(), tts_model,
        ).await {
            Ok(total_bytes) => Ok(total_bytes),
            Err(ws_err) => {
                warn!("[TTS] #{} {} WebSocket failed: {}, falling back to REST", utterance_id, lang_str, ws_err);
                do_tts_rest(
                    client, text, &voice_id, &lang_str, &voice_settings, max_bytes, streaming.as_ref(), tts_model,
                ).await
            }
        }
    }).await;

    // Ensure streaming is marked complete on any exit path
    if let Some(ref s) = streaming {
        s.finish();
    }

    let tts_ms = tts_start.elapsed().as_millis() as u64;
    match tts_result {
        Ok(Ok(total_bytes)) => {
            info!("[TTS] {}KB in {}ms for {} (streaming PCM)", total_bytes / 1024, tts_ms, lang);
        }
        Ok(Err(e)) => {
            error!("[TTS] Failed for {}: {} ({}ms)", lang, e, tts_ms);
        }
        Err(_) => {
            error!(
                "[TTS] TIMEOUT: utterance {} for {} exceeded {}ms",
                utterance_id, lang, tts_deadline.as_millis()
            );
        }
    }

    // Notify host: TTS ended
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(crate::pipeline::to_ws(&ServerMsg::TtsEnd { lang: lang_str, utterance_id, tts_ms }));
    }
}

// ── ElevenLabs WebSocket TTS ─────────────────────────────
//
// Each connection is single-use (voice_id baked into URL).
// No pooling needed — Turbo v2.5 has fast enough TTFB.

pub async fn do_tts_ws(
    text: &str,
    voice_id: &str,
    lang: &str,
    voice_settings: &serde_json::Value,
    max_bytes: usize,
    streaming: Option<&crate::ffmpeg::StreamingPcm>,
    model_id: &str,
) -> Result<usize, String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let connect_start = Instant::now();
    let url = format!(
        "wss://api.elevenlabs.io/v1/text-to-speech/{}/stream-input\
         ?model_id={}\
         &output_format=mp3_44100_128\
         &language_code={}",
        voice_id, model_id, lang
    );
    let request = url.into_client_request()
        .map_err(|e| format!("WS request build failed: {}", e))?;
    let (mut ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("WS connect failed: {}", e))?;
    debug!("[TTS:{}] connected in {}ms", lang, connect_start.elapsed().as_millis());

    let tts_start = Instant::now();

    // BOS: initialize with API key, voice settings, and low-latency chunking
    let bos = serde_json::json!({
        "text": " ",
        "xi_api_key": &*TTS_API_KEY,
        "voice_settings": voice_settings,
        "generation_config": { "chunk_length_schedule": [50] }
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&bos).unwrap().into()))
        .await.map_err(|e| format!("BOS send failed: {}", e))?;

    // Send full text with flush to force generation
    let text_msg = serde_json::json!({ "text": text, "flush": true });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&text_msg).unwrap().into()))
        .await.map_err(|e| format!("text send failed: {}", e))?;

    // EOS: signal end of input
    ws.send(tokio_tungstenite::tungstenite::Message::Text(r#"{"text":""}"#.to_string().into()))
        .await.map_err(|e| format!("EOS send failed: {}", e))?;

    // Incremental MP3->PCM decode: each chunk decoded and streamed to RTMP immediately
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

        // Detect error responses from ElevenLabs (not captured by our struct)
        if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&text_data) {
            if let Some(detail) = raw.get("detail") {
                return Err(format!("ElevenLabs error: {}", detail));
            }
            if let Some(msg) = raw.get("message").and_then(|m| m.as_str()) {
                if raw.get("audio").is_none() {
                    return Err(format!("ElevenLabs error: {}", msg));
                }
            }
        }

        let resp: ElevenLabsTtsResponse = serde_json::from_str(&text_data)
            .map_err(|e| format!("WS parse error: {} | raw: {}", e, &text_data[..text_data.len().min(200)]))?;

        if resp.is_final.unwrap_or(false) {
            debug!(
                "[TTS:{}] done: {} chunks, {}KB PCM streamed in {}ms",
                lang, chunk_count, total_pcm_bytes / 1024, tts_start.elapsed().as_millis()
            );
            break;
        }

        if let Some(audio_b64) = resp.audio {
            if audio_b64.is_empty() { continue; }
            let mp3_chunk = base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                &audio_b64,
            ).map_err(|e| format!("base64 decode error: {}", e))?;

            chunk_count += 1;
            if chunk_count == 1 {
                debug!(
                    "[TTS:{}] TTFB {}ms ({}B first MP3 chunk)",
                    lang, tts_start.elapsed().as_millis(), mp3_chunk.len()
                );
            }

            // Incrementally decode MP3->PCM and stream to RTMP
            let pcm_chunk = decoder.feed(&mp3_chunk).await
                .map_err(|e| format!("Incremental decode failed: {}", e))?;
            if !pcm_chunk.is_empty() {
                got_audio = true;
                total_pcm_bytes += pcm_chunk.len();
                if let Some(s) = streaming {
                    s.append_with_limit(&pcm_chunk, max_bytes);
                }
            }
        }
    }

    // Drain remaining PCM from the decoder
    let remaining_pcm = decoder.finish().await
        .map_err(|e| format!("Decoder finish failed: {}", e))?;
    if !remaining_pcm.is_empty() {
        got_audio = true;
        total_pcm_bytes += remaining_pcm.len();
        if let Some(s) = streaming {
            s.append_with_limit(&remaining_pcm, max_bytes);
        }
    }

    if !got_audio {
        warn!("[TTS:{}] WARNING: stream ended with 0 audio bytes ({}ms)", lang, tts_start.elapsed().as_millis());
        return Ok(0);
    }

    Ok(total_pcm_bytes)
}

/// REST fallback TTS — used when WebSocket connection fails.
pub async fn do_tts_rest(
    client: &reqwest::Client,
    text: &str,
    voice_id: &str,
    lang: &str,
    voice_settings: &serde_json::Value,
    max_bytes: usize,
    streaming: Option<&crate::ffmpeg::StreamingPcm>,
    model_id: &str,
) -> Result<usize, String> {
    let tts_body = serde_json::json!({
        "text": text,
        "model_id": model_id,
        "voice_settings": voice_settings,
        "language_code": lang,
    });

    let url = format!(
        "https://api.elevenlabs.io/v1/text-to-speech/{}?output_format=mp3_44100_128",
        voice_id
    );

    debug!("[TTS:{}] REST fallback", lang);
    let rest_start = Instant::now();
    let resp = client
        .post(&url)
        .header("xi-api-key", &*TTS_API_KEY)
        .json(&tts_body)
        .send()
        .await
        .map_err(|e| format!("REST request error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("ElevenLabs REST error {}: {}", status, body));
    }

    let mp3 = resp.bytes().await
        .map(|b| b.to_vec())
        .map_err(|e| format!("REST body read error: {}", e))?;

    if mp3.is_empty() {
        return Err("Empty response from REST".to_string());
    }

    // Decode MP3 -> PCM s16le 44100Hz
    let mut pcm = crate::ffmpeg::decode_mp3_to_pcm(&mp3).await?;

    if pcm.len() > max_bytes {
        crate::ffmpeg::truncate_with_fadeout(&mut pcm, max_bytes);
    }

    let total = pcm.len();
    debug!(
        "[TTS:{}] REST complete: {}KB MP3 -> {}KB PCM in {}ms",
        lang, mp3.len() / 1024, total / 1024, rest_start.elapsed().as_millis()
    );
    if let Some(s) = streaming {
        s.append(&pcm);
    }

    Ok(total)
}
