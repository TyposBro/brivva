//! HTTP client for Fargate → Workers /internal endpoints.
//!
//! Workers is the canonical store for users/voices/sessions/streams.
//! Fargate calls in at session start to fetch context and at session end
//! (or state change) to report status. Shared secret auth via
//! `X-Internal-Secret` header.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SessionRow {
    pub id: String,
    pub user_id: String,
    pub voice_id: Option<String>,
    pub title: String,
    pub source_lang: String,
    pub target_langs: String,
    pub status: String,
    pub live_session_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamRow {
    pub id: String,
    #[allow(dead_code)]
    pub session_id: String,
    pub lang: String,
    #[allow(dead_code)]
    pub platform: String,
    pub rtmp_url: Option<String>,
    pub stream_key: Option<String>,
    #[allow(dead_code)]
    pub status: String,
    /// Per-stream output delay in ms — Fargate holds original host media
    /// for this long before emitting to RTMP, giving STT+translate+TTS a
    /// window to produce the translated audio that overlays at emit time.
    #[serde(default = "default_delay_ms")]
    pub delay_ms: u64,
    /// Gain applied to the delayed host audio at mix time. 1.0 = full volume
    /// (source-language stream), 0.2 = quiet underlay (target streams where
    /// translated TTS should dominate).
    #[serde(default = "default_host_gain")]
    pub host_gain: f32,
}

fn default_delay_ms() -> u64 {
    2000
}
fn default_host_gain() -> f32 {
    0.2
}

#[derive(Debug, Clone, Deserialize)]
pub struct VoiceRow {
    #[allow(dead_code)]
    pub id: String,
    pub elevenlabs_voice_id: String,
    #[allow(dead_code)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct SessionBundle {
    pub session: SessionRow,
    pub streams: Vec<StreamRow>,
    pub voice: Option<VoiceRow>,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("reqwest client")
}

fn base() -> Result<String, String> {
    let url = std::env::var("WORKERS_API_URL")
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string();
    if url.is_empty() {
        return Err("WORKERS_API_URL not set".into());
    }
    Ok(url)
}

pub async fn fetch_session_bundle(session_id: &str) -> Result<SessionBundle, String> {
    let url = format!("{}/internal/sessions/{}", base()?, session_id);
    let resp = client()
        .get(&url)
        .header(
            "X-Internal-Secret",
            std::env::var("INTERNAL_SECRET").unwrap_or_default(),
        )
        .send()
        .await
        .map_err(|e| format!("workers fetch error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "workers session fetch {} → {}",
            session_id,
            resp.status()
        ));
    }
    resp.json::<SessionBundle>()
        .await
        .map_err(|e| format!("workers session decode: {e}"))
}

#[derive(Serialize)]
struct StatusUpdate<'a> {
    status: &'a str,
    live_session_id: Option<&'a str>,
}

pub async fn update_session_status(
    session_id: &str,
    status: &str,
    live_session_id: Option<&str>,
) -> Result<(), String> {
    let url = format!("{}/internal/sessions/{}", base()?, session_id);
    let resp = client()
        .patch(&url)
        .header(
            "X-Internal-Secret",
            std::env::var("INTERNAL_SECRET").unwrap_or_default(),
        )
        .json(&StatusUpdate {
            status,
            live_session_id,
        })
        .send()
        .await
        .map_err(|e| format!("workers status update error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("workers status {} → {}", status, resp.status()));
    }
    Ok(())
}
