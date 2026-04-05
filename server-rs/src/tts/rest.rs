//! ElevenLabs REST fallback TTS.

use tracing::debug;

use super::config::TTS_API_KEY;
use super::SynthesisRequest;

pub async fn do_tts_rest(
    client: &reqwest::Client,
    req: &SynthesisRequest<'_>,
) -> Result<usize, String> {
    let mp3 = send_tts_request(client, req).await?;
    let pcm = decode_and_truncate(&mp3, req.max_bytes).await?;

    log_rest_complete(req.lang, mp3.len(), pcm.len());
    append_to_stream(&pcm, req.streaming);
    Ok(pcm.len())
}

// ── Request building ───

async fn send_tts_request(
    client: &reqwest::Client,
    req: &SynthesisRequest<'_>,
) -> Result<Vec<u8>, String> {
    let body = build_tts_body(req);
    let url = build_rest_url(req.voice_id);
    debug!("[TTS:{}] REST fallback", req.lang);

    let resp = post_request(client, &url, &body).await?;
    check_response(resp).await
}

fn build_tts_body(req: &SynthesisRequest<'_>) -> serde_json::Value {
    serde_json::json!({
        "text": req.text,
        "model_id": req.model_id,
        "voice_settings": req.voice_settings,
        "language_code": req.lang,
    })
}

fn build_rest_url(voice_id: &str) -> String {
    format!("https://api.elevenlabs.io/v1/text-to-speech/{}?output_format=mp3_44100_128", voice_id)
}

// ── HTTP ───

async fn post_request(client: &reqwest::Client, url: &str, body: &serde_json::Value) -> Result<reqwest::Response, String> {
    client
        .post(url)
        .header("xi-api-key", &*TTS_API_KEY)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("REST request error: {}", e))
}

async fn check_response(resp: reqwest::Response) -> Result<Vec<u8>, String> {
    if !resp.status().is_success() {
        return Err(format_error_response(resp).await);
    }
    read_response_bytes(resp).await
}

async fn format_error_response(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    format!("ElevenLabs REST error {}: {}", status, body)
}

async fn read_response_bytes(resp: reqwest::Response) -> Result<Vec<u8>, String> {
    let mp3 = resp.bytes().await
        .map(|b| b.to_vec())
        .map_err(|e| format!("REST body read error: {}", e))?;
    if mp3.is_empty() {
        return Err("Empty response from REST".to_string());
    }
    Ok(mp3)
}

// ── PCM processing ───

async fn decode_and_truncate(mp3: &[u8], max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut pcm = crate::streaming::decode_mp3_to_pcm(mp3).await?;
    if pcm.len() > max_bytes {
        crate::streaming::truncate_with_fadeout(&mut pcm, max_bytes);
    }
    Ok(pcm)
}

fn append_to_stream(pcm: &[u8], streaming: Option<&crate::streaming::StreamingPcm>) {
    if let Some(s) = streaming {
        s.append(pcm);
    }
}

fn log_rest_complete(lang: &str, mp3_len: usize, pcm_len: usize) {
    debug!(
        "[TTS:{}] REST complete: {}KB MP3 -> {}KB PCM in N/A",
        lang, mp3_len / 1024, pcm_len / 1024
    );
}
