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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_row_applies_delay_and_gain_defaults_when_absent() {
        let json = r#"{
            "id": "s1",
            "session_id": "S",
            "lang": "en",
            "platform": "youtube",
            "rtmp_url": "rtmp://a/b",
            "stream_key": "k",
            "status": "pending"
        }"#;
        let row: StreamRow = serde_json::from_str(json).expect("valid payload");

        assert_eq!(row.id, "s1");
        assert_eq!(row.lang, "en");
        assert_eq!(row.delay_ms, 2000, "delay default must match start_stream contract");
        assert!((row.host_gain - 0.2).abs() < f32::EPSILON, "gain default");
    }

    #[test]
    fn stream_row_keeps_explicit_delay_and_gain_overrides() {
        let json = r#"{
            "id": "s1",
            "session_id": "S",
            "lang": "ja",
            "platform": "custom",
            "rtmp_url": null,
            "stream_key": null,
            "status": "live",
            "delay_ms": 500,
            "host_gain": 1.0
        }"#;
        let row: StreamRow = serde_json::from_str(json).expect("valid payload");

        assert_eq!(row.delay_ms, 500);
        assert!((row.host_gain - 1.0).abs() < f32::EPSILON);
        assert!(row.rtmp_url.is_none());
        assert!(row.stream_key.is_none());
    }

    #[test]
    fn session_bundle_accepts_null_voice_and_empty_streams() {
        let json = r#"{
            "session": {
                "id": "abc",
                "user_id": "u",
                "voice_id": null,
                "title": "t",
                "source_lang": "en",
                "target_langs": "[\"ja\"]",
                "status": "created",
                "live_session_id": null,
                "created_at": 123
            },
            "streams": [],
            "voice": null
        }"#;

        let bundle: SessionBundle = serde_json::from_str(json).expect("null voice is valid");
        assert_eq!(bundle.session.id, "abc");
        assert!(bundle.streams.is_empty());
        assert!(bundle.voice.is_none());
    }

    #[test]
    fn session_bundle_roundtrips_voice_row() {
        let json = r#"{
            "session": {
                "id": "abc",
                "user_id": "u",
                "voice_id": "v",
                "title": "t",
                "source_lang": "en",
                "target_langs": "[\"ja\"]",
                "status": "live",
                "live_session_id": "LIVE01",
                "created_at": 123
            },
            "streams": [{
                "id": "s1",
                "session_id": "abc",
                "lang": "ja",
                "platform": "youtube",
                "rtmp_url": "rtmp://x/y",
                "stream_key": "k",
                "status": "pending"
            }],
            "voice": {
                "id": "v",
                "elevenlabs_voice_id": "EL123",
                "name": "cloned"
            }
        }"#;

        let bundle: SessionBundle = serde_json::from_str(json).expect("valid bundle");
        assert_eq!(bundle.streams.len(), 1);
        assert_eq!(bundle.streams[0].lang, "ja");
        let voice = bundle.voice.expect("voice present");
        assert_eq!(voice.elevenlabs_voice_id, "EL123");
    }
}
