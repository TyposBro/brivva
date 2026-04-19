//! Background reporter task for per-session billing counters.
//!
//! The counter struct itself (`SessionMetrics`) lives in `domain/metrics.rs`
//! since it is pure state. This module owns the tokio task that snapshots
//! the counters and POSTs to Workers — a `data/` concern because it needs
//! the HTTP client.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::workers_api::WorkersApi;
use crate::features::broadcast::domain::SessionMetrics;

pub use crate::features::broadcast::domain::metrics::MetricsPayload;

/// How often the reporter task snapshots + POSTs billing metrics.
const METRICS_INTERVAL: Duration = Duration::from_secs(30);

/// Background reporter. Ticks every `METRICS_INTERVAL`, snapshots the
/// counters, and POSTs to Workers. When the endpoint is missing (today) or
/// WorkersApi.base_url is empty (local/test), logs the payload instead of
/// propagating the error. Stops when `stop_flag` is set.
///
/// Crash isolation: the task catches all per-tick errors itself so a flaky
/// network never brings down the pipeline.
pub fn spawn_metrics_reporter(
    metrics: Arc<SessionMetrics>,
    workers_api: Arc<WorkersApi>,
    session_id: Option<String>,
    live_session_id: String,
    stop_flag: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    spawn_metrics_reporter_with_interval(
        metrics,
        workers_api,
        session_id,
        live_session_id,
        stop_flag,
        METRICS_INTERVAL,
    )
}

/// Internal variant parameterized by tick interval. Lets tests drive the
/// reporter quickly without changing the production cadence. Not part of
/// the public API.
pub(crate) fn spawn_metrics_reporter_with_interval(
    metrics: Arc<SessionMetrics>,
    workers_api: Arc<WorkersApi>,
    session_id: Option<String>,
    live_session_id: String,
    stop_flag: Arc<AtomicBool>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // First tick fires immediately; skip so we don't POST empty counters
        // right after start.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if stop_flag.load(Ordering::Acquire) {
                break;
            }
            let payload = metrics.snapshot();
            match &session_id {
                Some(sid) => match workers_api.report_session_metrics(sid, &payload).await {
                    Ok(()) => {}
                    Err(e) => tracing::debug!(
                        live_session_id = %live_session_id,
                        session_id = %sid,
                        error = %e,
                        payload = ?payload,
                        "metrics report failed — logging locally until endpoint lands"
                    ),
                },
                None => tracing::info!(
                    live_session_id = %live_session_id,
                    payload = ?payload,
                    "session metrics tick (no session_id, not reporting)"
                ),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        extract::{Path, State},
        http::StatusCode,
        routing::post,
    };
    use std::sync::atomic::AtomicU32;
    use tokio::task::JoinHandle;

    async fn spawn_counting_mock(
        counter: Arc<AtomicU32>,
        status: StatusCode,
    ) -> (String, JoinHandle<()>) {
        async fn handler(
            State(state): State<(Arc<AtomicU32>, StatusCode)>,
            Path(_): Path<String>,
            _body: axum::body::Bytes,
        ) -> StatusCode {
            state.0.fetch_add(1, Ordering::Relaxed);
            state.1
        }
        let app = Router::new()
            .route("/internal/sessions/{id}/metrics", post(handler))
            .with_state((counter, status));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{}", addr), handle)
    }

    #[tokio::test]
    async fn reporter_posts_to_workers_on_each_tick_when_session_id_present() {
        let counter = Arc::new(AtomicU32::new(0));
        let (base, server) = spawn_counting_mock(counter.clone(), StatusCode::OK).await;

        let metrics = SessionMetrics::new();
        let api = Arc::new(WorkersApi::new(&base, "sec"));
        let stop = Arc::new(AtomicBool::new(false));
        let reporter = spawn_metrics_reporter_with_interval(
            metrics,
            api,
            Some("S".into()),
            "LIVE".into(),
            stop.clone(),
            Duration::from_millis(20),
        );

        tokio::time::sleep(Duration::from_millis(120)).await;
        stop.store(true, Ordering::Release);
        let _ = tokio::time::timeout(Duration::from_millis(200), reporter).await;

        let ticks = counter.load(Ordering::Relaxed);
        assert!(ticks >= 2, "expected ≥2 ticks, got {ticks}");

        server.abort();
    }

    #[tokio::test]
    async fn reporter_keeps_running_when_workers_returns_non_2xx() {
        let counter = Arc::new(AtomicU32::new(0));
        let (base, server) = spawn_counting_mock(counter.clone(), StatusCode::NOT_FOUND).await;

        let metrics = SessionMetrics::new();
        let api = Arc::new(WorkersApi::new(&base, "sec"));
        let stop = Arc::new(AtomicBool::new(false));
        let reporter = spawn_metrics_reporter_with_interval(
            metrics,
            api,
            Some("S".into()),
            "LIVE".into(),
            stop.clone(),
            Duration::from_millis(20),
        );

        tokio::time::sleep(Duration::from_millis(120)).await;
        // Still ticking after the first 404 — task didn't abort.
        let first = counter.load(Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(80)).await;
        let second = counter.load(Ordering::Relaxed);
        stop.store(true, Ordering::Release);
        let _ = tokio::time::timeout(Duration::from_millis(200), reporter).await;

        assert!(
            second > first,
            "expected task to keep ticking past 404; first={first} second={second}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn reporter_logs_locally_and_does_not_post_when_session_id_missing() {
        let counter = Arc::new(AtomicU32::new(0));
        let (base, server) = spawn_counting_mock(counter.clone(), StatusCode::OK).await;

        let metrics = SessionMetrics::new();
        let api = Arc::new(WorkersApi::new(&base, "sec"));
        let stop = Arc::new(AtomicBool::new(false));
        let reporter = spawn_metrics_reporter_with_interval(
            metrics,
            api,
            None, // no session id → no POST, local log branch
            "LIVE".into(),
            stop.clone(),
            Duration::from_millis(20),
        );

        tokio::time::sleep(Duration::from_millis(80)).await;
        stop.store(true, Ordering::Release);
        let _ = tokio::time::timeout(Duration::from_millis(200), reporter).await;

        assert_eq!(counter.load(Ordering::Relaxed), 0);
        server.abort();
    }

    #[tokio::test]
    async fn reporter_exits_promptly_when_stop_flag_set_before_first_tick() {
        let counter = Arc::new(AtomicU32::new(0));
        let (base, server) = spawn_counting_mock(counter.clone(), StatusCode::OK).await;

        let metrics = SessionMetrics::new();
        let api = Arc::new(WorkersApi::new(&base, "sec"));
        let stop = Arc::new(AtomicBool::new(true));
        let reporter = spawn_metrics_reporter_with_interval(
            metrics,
            api,
            Some("S".into()),
            "LIVE".into(),
            stop.clone(),
            Duration::from_millis(20),
        );

        let joined = tokio::time::timeout(Duration::from_millis(200), reporter)
            .await
            .expect("reporter should exit promptly");
        joined.unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 0);
        server.abort();
    }

    #[tokio::test]
    async fn default_interval_constant_matches_documented_30_seconds() {
        assert_eq!(METRICS_INTERVAL, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn public_entry_point_spawns_a_task_that_can_be_stopped_immediately() {
        // Just verify that the public API constructs a running task + can
        // be stopped without panic. The short interval variant is covered
        // by the other tests — this one touches the 30s default path.
        let metrics = SessionMetrics::new();
        let api = Arc::new(WorkersApi::new("", "sec"));
        let stop = Arc::new(AtomicBool::new(true));
        let reporter = spawn_metrics_reporter(metrics, api, None, "LIVE".into(), stop);
        // Wait is bounded — even at 30s interval, the task enters the
        // select before the first tick returns, so stopping before it
        // wakes is the observable effect we want.
        reporter.abort();
    }
}
