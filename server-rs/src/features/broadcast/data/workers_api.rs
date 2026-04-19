//! HTTP client for Fargate → Workers /internal endpoints.
//!
//! Workers is the canonical store for users/voices/sessions/streams.
//! Fargate calls in at session start to fetch context and at session end
//! (or state change) to report status. Shared secret auth via
//! `X-Internal-Secret` header.
//!
//! Wire types are generated from the Workers OpenAPI export via
//! `bun run --cwd contracts gen:rust` — see `core::contracts::workers`.
//!
//! Config is injected at construction time (CLAUDE.md §7) so this module
//! never reads process env vars directly.

use crate::core::contracts::workers::{SessionBundle, SessionStatusUpdate};

pub struct WorkersApi {
    base_url: String,
    internal_secret: String,
    client: reqwest::Client,
}

impl WorkersApi {
    pub fn new(base_url: &str, internal_secret: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            internal_secret: internal_secret.to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("reqwest client"),
        }
    }

    pub async fn fetch_session_bundle(&self, session_id: &str) -> Result<SessionBundle, String> {
        if self.base_url.is_empty() {
            return Err("WORKERS_API_URL not set".into());
        }
        let url = format!("{}/internal/sessions/{}", self.base_url, session_id);
        let resp = self
            .client
            .get(&url)
            .header("X-Internal-Secret", &self.internal_secret)
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

    /// POST billing metrics to Workers. The endpoint is not in the current
    /// OpenAPI export, so we call it optimistically and rely on the generic
    /// Serialize-bound to keep this module decoupled from the payload type.
    /// On non-2xx (including 404 while the endpoint doesn't yet exist) we
    /// return Err, and the caller logs rather than escalating.
    pub async fn report_session_metrics<T: serde::Serialize + ?Sized>(
        &self,
        session_id: &str,
        payload: &T,
    ) -> Result<(), String> {
        if self.base_url.is_empty() {
            return Err("WORKERS_API_URL not set".into());
        }
        let url = format!("{}/internal/sessions/{}/metrics", self.base_url, session_id);
        let resp = self
            .client
            .post(&url)
            .header("X-Internal-Secret", &self.internal_secret)
            .json(payload)
            .send()
            .await
            .map_err(|e| format!("workers metrics error: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!(
                "workers metrics {} → {}",
                session_id,
                resp.status()
            ));
        }
        Ok(())
    }

    pub async fn update_session_status(
        &self,
        session_id: &str,
        status: &str,
        live_session_id: Option<&str>,
    ) -> Result<(), String> {
        if self.base_url.is_empty() {
            return Err("WORKERS_API_URL not set".into());
        }
        let url = format!("{}/internal/sessions/{}", self.base_url, session_id);
        let resp = self
            .client
            .patch(&url)
            .header("X-Internal-Secret", &self.internal_secret)
            .json(&SessionStatusUpdate {
                status: status.to_string(),
                live_session_id: live_session_id.map(str::to_string),
            })
            .send()
            .await
            .map_err(|e| format!("workers status update error: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("workers status {} → {}", status, resp.status()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::core::contracts::workers::{SessionBundle, Stream};

    #[test]
    fn stream_requires_delay_ms_and_host_gain_per_openapi_contract() {
        let missing_fields = r#"{
            "id": "s1",
            "session_id": "S",
            "lang": "en",
            "platform": "youtube",
            "platform_broadcast_id": null,
            "platform_stream_id": null,
            "rtmp_url": "rtmp://a/b",
            "stream_key": "k",
            "status": "pending",
            "created_at": 1
        }"#;
        assert!(
            serde_json::from_str::<Stream>(missing_fields).is_err(),
            "generated Stream must reject payloads missing the delay_ms/host_gain the schema marks required"
        );
    }

    #[test]
    fn stream_roundtrips_with_explicit_delay_and_gain() {
        let json = r#"{
            "id": "s1",
            "session_id": "S",
            "lang": "ja",
            "platform": "custom",
            "platform_broadcast_id": null,
            "platform_stream_id": null,
            "rtmp_url": null,
            "stream_key": null,
            "status": "live",
            "delay_ms": 500,
            "host_gain": 1.0,
            "created_at": 42
        }"#;
        let row: Stream = serde_json::from_str(json).expect("valid payload");

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
                "platform_broadcast_id": null,
                "platform_stream_id": null,
                "rtmp_url": "rtmp://x/y",
                "stream_key": "k",
                "status": "pending",
                "delay_ms": 2000,
                "host_gain": 0.2,
                "created_at": 123
            }],
            "voice": {
                "id": "v",
                "user_id": "u",
                "elevenlabs_voice_id": "EL123",
                "name": "cloned",
                "created_at": 456
            }
        }"#;

        let bundle: SessionBundle = serde_json::from_str(json).expect("valid bundle");
        assert_eq!(bundle.streams.len(), 1);
        assert_eq!(bundle.streams[0].lang, "ja");
        let voice = bundle.voice.expect("voice present");
        assert_eq!(voice.elevenlabs_voice_id, "EL123");
    }
}
