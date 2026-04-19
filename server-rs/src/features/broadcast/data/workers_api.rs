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

    /// PATCH billing metrics to Workers. Matches `app.patch(...)` in
    /// workers/src/orchestration/app.ts with merge-semantics — omitted
    /// fields leave prior values alone; per-lang output seconds shallow-merge.
    /// On non-2xx we return Err, and the caller logs rather than escalating.
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
            .patch(&url)
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
    use super::*;
    use crate::core::contracts::workers::{SessionBundle, Stream};
    use axum::{
        Json, Router,
        extract::{Path, State},
        http::StatusCode,
        routing::{get, patch},
    };
    use serde_json::{Value, json};
    use std::sync::Arc as StdArc;
    use tokio::task::JoinHandle;

    async fn spawn_mock(router: Router) -> (String, JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{}", addr), handle)
    }

    #[tokio::test]
    async fn fetch_session_bundle_errors_when_base_url_is_empty() {
        let api = WorkersApi::new("", "sec");
        let err = api
            .fetch_session_bundle("X")
            .await
            .expect_err("empty base url must error");
        assert!(err.contains("WORKERS_API_URL not set"));
    }

    #[tokio::test]
    async fn update_session_status_errors_when_base_url_is_empty() {
        let api = WorkersApi::new("", "sec");
        let err = api
            .update_session_status("X", "live", None)
            .await
            .expect_err("empty base url must error");
        assert!(err.contains("WORKERS_API_URL not set"));
    }

    #[tokio::test]
    async fn report_session_metrics_errors_when_base_url_is_empty() {
        let api = WorkersApi::new("", "sec");
        let err = api
            .report_session_metrics("X", &json!({}))
            .await
            .expect_err("empty base url must error");
        assert!(err.contains("WORKERS_API_URL not set"));
    }

    #[tokio::test]
    async fn fetch_session_bundle_errors_on_network_failure() {
        // Port 1 is reserved/refused on most systems — simulates connection failure.
        let api = WorkersApi::new("http://127.0.0.1:1", "sec");
        let err = api
            .fetch_session_bundle("X")
            .await
            .expect_err("refused connection must error");
        assert!(err.starts_with("workers fetch error"), "got {err}");
    }

    #[tokio::test]
    async fn fetch_session_bundle_errors_on_non_2xx() {
        async fn not_found(Path(_): Path<String>) -> StatusCode {
            StatusCode::NOT_FOUND
        }
        let app = Router::new().route("/internal/sessions/{id}", get(not_found));
        let (base, handle) = spawn_mock(app).await;

        let api = WorkersApi::new(&base, "sec");
        let err = api
            .fetch_session_bundle("X")
            .await
            .expect_err("404 must error");
        assert!(err.contains("→ 404"), "got {err}");
        handle.abort();
    }

    #[tokio::test]
    async fn fetch_session_bundle_errors_on_malformed_json() {
        async fn bad_json(Path(_): Path<String>) -> (StatusCode, String) {
            (StatusCode::OK, "not json".into())
        }
        let app = Router::new().route("/internal/sessions/{id}", get(bad_json));
        let (base, handle) = spawn_mock(app).await;

        let api = WorkersApi::new(&base, "sec");
        let err = api
            .fetch_session_bundle("X")
            .await
            .expect_err("bad json must error");
        assert!(err.contains("workers session decode"), "got {err}");
        handle.abort();
    }

    #[tokio::test]
    async fn fetch_session_bundle_decodes_minimal_valid_body() {
        async fn ok_json(Path(id): Path<String>) -> Json<Value> {
            Json(json!({
                "session": {
                    "id": id,
                    "user_id": "u",
                    "voice_id": null,
                    "title": "",
                    "source_lang": "en",
                    "target_langs": "[]",
                    "status": "created",
                    "live_session_id": null,
                    "created_at": 0
                },
                "streams": [],
                "voice": null
            }))
        }
        let app = Router::new().route("/internal/sessions/{id}", get(ok_json));
        let (base, handle) = spawn_mock(app).await;

        let api = WorkersApi::new(&base, "sec");
        let bundle = api.fetch_session_bundle("abc").await.expect("ok");
        assert_eq!(bundle.session.id, "abc");
        handle.abort();
    }

    #[tokio::test]
    async fn update_session_status_errors_on_5xx() {
        async fn boom() -> StatusCode {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        let app = Router::new().route("/internal/sessions/{id}", patch(boom));
        let (base, handle) = spawn_mock(app).await;

        let api = WorkersApi::new(&base, "sec");
        let err = api
            .update_session_status("X", "live", Some("LIVE01"))
            .await
            .expect_err("5xx must error");
        assert!(err.contains("→ 500"), "got {err}");
        handle.abort();
    }

    #[tokio::test]
    async fn update_session_status_network_error_is_wrapped() {
        let api = WorkersApi::new("http://127.0.0.1:1", "sec");
        let err = api
            .update_session_status("X", "live", None)
            .await
            .expect_err("refused connection must error");
        assert!(err.starts_with("workers status update error"), "got {err}");
    }

    #[tokio::test]
    async fn update_session_status_succeeds_on_2xx_and_sends_payload() {
        #[derive(Clone)]
        struct S(StdArc<tokio::sync::Mutex<Option<Value>>>);

        async fn capture(
            State(s): State<S>,
            Path(_): Path<String>,
            Json(body): Json<Value>,
        ) -> StatusCode {
            *s.0.lock().await = Some(body);
            StatusCode::OK
        }

        let shared = StdArc::new(tokio::sync::Mutex::new(None));
        let state = S(shared.clone());
        let app = Router::new()
            .route("/internal/sessions/{id}", patch(capture))
            .with_state(state);
        let (base, handle) = spawn_mock(app).await;

        let api = WorkersApi::new(&base, "sec");
        api.update_session_status("X", "live", Some("LIVE01"))
            .await
            .expect("ok");

        let body = shared.lock().await.clone().expect("captured");
        assert_eq!(body["status"].as_str().unwrap(), "live");
        assert_eq!(body["live_session_id"].as_str().unwrap(), "LIVE01");
        handle.abort();
    }

    #[tokio::test]
    async fn report_session_metrics_errors_on_404() {
        async fn not_found() -> StatusCode {
            StatusCode::NOT_FOUND
        }
        let app = Router::new().route("/internal/sessions/{id}/metrics", patch(not_found));
        let (base, handle) = spawn_mock(app).await;

        let api = WorkersApi::new(&base, "sec");
        let err = api
            .report_session_metrics("X", &json!({"k":1}))
            .await
            .expect_err("404 must error");
        assert!(err.contains("→ 404"), "got {err}");
        handle.abort();
    }

    #[tokio::test]
    async fn report_session_metrics_succeeds_on_2xx() {
        async fn ok() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new().route("/internal/sessions/{id}/metrics", patch(ok));
        let (base, handle) = spawn_mock(app).await;

        let api = WorkersApi::new(&base, "sec");
        api.report_session_metrics("X", &json!({"k":1}))
            .await
            .expect("ok");
        handle.abort();
    }

    #[tokio::test]
    async fn report_session_metrics_network_error_is_wrapped() {
        let api = WorkersApi::new("http://127.0.0.1:1", "sec");
        let err = api
            .report_session_metrics("X", &json!({}))
            .await
            .expect_err("refused connection must error");
        assert!(err.starts_with("workers metrics error"), "got {err}");
    }

    #[tokio::test]
    async fn workers_api_new_trims_trailing_slash_from_base_url() {
        let api = WorkersApi::new("http://example.com/", "sec");
        assert_eq!(api.base_url, "http://example.com");
    }

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
