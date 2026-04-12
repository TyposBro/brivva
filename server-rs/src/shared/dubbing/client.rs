//! ElevenLabs Dubbing API HTTP calls.
//!
//! Follows the same auth-header and error-handling patterns as
//! `shared::voice_clone::elevenlabs`.

use std::path::Path;

use tracing::{info, error};

use super::types::{CreateDubbingResponse, DubbingStatus, DubbingStatusResponse};

const BASE_URL: &str = "https://api.elevenlabs.io/v1/dubbing";

/// Create a dubbing job from a video file.
///
/// Returns the `dubbing_id` on success.
pub async fn create_dubbing(
    client: &reqwest::Client,
    api_key: &str,
    video_path: &Path,
    source_lang: &str,
    target_lang: &str,
) -> Result<String, String> {
    let file_bytes = read_video_file(video_path)?;
    let form = build_dubbing_form(file_bytes, video_path, source_lang, target_lang);
    let resp = send_create_request(client, api_key, form).await?;
    parse_create_response(resp).await
}

/// Poll the status of an existing dubbing job.
pub async fn poll_status(
    client: &reqwest::Client,
    api_key: &str,
    dubbing_id: &str,
) -> Result<DubbingStatus, String> {
    let url = format!("{}/{}", BASE_URL, dubbing_id);
    let resp = client.get(&url)
        .header("xi-api-key", api_key)
        .send()
        .await
        .map_err(|e| format!("poll request error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format_api_error("poll_status", resp).await);
    }

    resp.json::<DubbingStatusResponse>()
        .await
        .map(|r| r.into_status())
        .map_err(|e| format!("poll parse error: {}", e))
}

/// Download the dubbed audio for a specific language.
///
/// Streams the response body directly to `output_path`.
pub async fn download_audio(
    client: &reqwest::Client,
    api_key: &str,
    dubbing_id: &str,
    lang: &str,
    output_path: &Path,
) -> Result<(), String> {
    let url = format!("{}/{}/audio/{}", BASE_URL, dubbing_id, lang);
    let resp = client.get(&url)
        .header("xi-api-key", api_key)
        .send()
        .await
        .map_err(|e| format!("download request error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format_api_error("download_audio", resp).await);
    }

    stream_to_file(resp, output_path).await
}

// ── create_dubbing helpers ──────────────────────────────────────────────────

fn read_video_file(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("read {}: {}", path.display(), e))
}

fn build_dubbing_form(
    file_bytes: Vec<u8>,
    video_path: &Path,
    source_lang: &str,
    target_lang: &str,
) -> reqwest::multipart::Form {
    let file_name = video_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("video.mp4")
        .to_string();

    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("video/mp4")
        .unwrap();

    reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("source_lang", source_lang.to_string())
        .text("target_lang", target_lang.to_string())
}

async fn send_create_request(
    client: &reqwest::Client,
    api_key: &str,
    form: reqwest::multipart::Form,
) -> Result<reqwest::Response, String> {
    client.post(BASE_URL)
        .header("xi-api-key", api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("create dubbing request error: {}", e))
}

async fn parse_create_response(resp: reqwest::Response) -> Result<String, String> {
    if !resp.status().is_success() {
        return Err(format_api_error("create_dubbing", resp).await);
    }
    let parsed = resp.json::<CreateDubbingResponse>()
        .await
        .map_err(|e| format!("create dubbing parse error: {}", e))?;

    info!("[DUBBING] created dubbing_id={}", parsed.dubbing_id);
    Ok(parsed.dubbing_id)
}

// ── download helpers ────────────────────────────────────────────────────────

async fn stream_to_file(resp: reqwest::Response, path: &Path) -> Result<(), String> {
    let bytes = resp.bytes()
        .await
        .map_err(|e| format!("download body error: {}", e))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }

    std::fs::write(path, &bytes)
        .map_err(|e| format!("write {}: {}", path.display(), e))?;

    info!("[DUBBING] downloaded {} bytes to {}", bytes.len(), path.display());
    Ok(())
}

// ── Shared error formatting ─────────────────────────────────────────────────

async fn format_api_error(op: &str, resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    error!("[DUBBING] {} error {}: {}", op, status, body);
    format!("ElevenLabs {} {}: {}", op, status, body)
}
