//! ElevenLabs REST fallback TTS.

use tracing::debug;

use super::config::TTS_API_KEY;

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
    let mp3 = send_tts_request(client, text, voice_id, lang, voice_settings, model_id).await?;
    let mut pcm = crate::ffmpeg::decode_mp3_to_pcm(&mp3).await?;

    if pcm.len() > max_bytes {
        crate::ffmpeg::truncate_with_fadeout(&mut pcm, max_bytes);
    }

    let total = pcm.len();
    debug!(
        "[TTS:{}] REST complete: {}KB MP3 -> {}KB PCM in N/A",
        lang, mp3.len() / 1024, total / 1024
    );
    if let Some(s) = streaming {
        s.append(&pcm);
    }
    Ok(total)
}

async fn send_tts_request(
    client: &reqwest::Client,
    text: &str,
    voice_id: &str,
    lang: &str,
    voice_settings: &serde_json::Value,
    model_id: &str,
) -> Result<Vec<u8>, String> {
    let tts_body = serde_json::json!({
        "text": text,
        "model_id": model_id,
        "voice_settings": voice_settings,
        "language_code": lang,
    });
    let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{}?output_format=mp3_44100_128", voice_id);

    debug!("[TTS:{}] REST fallback", lang);
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
    Ok(mp3)
}
