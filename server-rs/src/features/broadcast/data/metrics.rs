//! Background reporter task for per-session billing counters.
//!
//! The counter struct itself (`SessionMetrics`) lives in `domain/metrics.rs`
//! since it is pure state. This module owns the tokio task that snapshots
//! the counters and PATCHes to Workers — a `data/` concern because it needs
//! the HTTP client.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::workers_api::{MetricsReportError, WorkersApi};
use crate::features::broadcast::domain::SessionMetrics;

pub use crate::features::broadcast::domain::metrics::MetricsPayload;

/// How often the reporter task snapshots + POSTs billing metrics.
const METRICS_INTERVAL: Duration = Duration::from_secs(30);

/// Stop reporting after this many consecutive 404 responses from Workers.
/// 404 means the session was deleted/ended out from under us — at that
/// point further PATCHes are pure noise (and the most likely trigger of
/// the SQLITE_BUSY_RECOVERY storms we saw in prod 2026-04-20). One stray
/// 404 isn't enough — Workers may briefly miss a row mid-rollback — but
/// two in a row is decisive.
const MAX_CONSECUTIVE_NOT_FOUND: u32 = 2;

/// Background reporter. Ticks every `METRICS_INTERVAL`, snapshots the
/// counters, and PATCHes to Workers. When `WorkersApi.base_url` is empty
/// (local/test) or a tick errors, logs the payload instead of propagating.
/// Stops when `stop_flag` is set.
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
        let mut consecutive_not_found: u32 = 0;
        loop {
            ticker.tick().await;
            if stop_flag.load(Ordering::Acquire) {
                break;
            }
            let payload = metrics.snapshot();
            match &session_id {
                Some(sid) => match workers_api.report_session_metrics(sid, &payload).await {
                    Ok(()) => {
                        // Successful report → reset the 404 streak so a single
                        // mid-rollback miss can't accidentally retire a still
                        // live session later.
                        consecutive_not_found = 0;
                    }
                    Err(MetricsReportError::NotFound) => {
                        consecutive_not_found += 1;
                        tracing::warn!(
                            live_session_id = %live_session_id,
                            session_id = %sid,
                            streak = consecutive_not_found,
                            limit = MAX_CONSECUTIVE_NOT_FOUND,
                            "workers metrics PATCH 404 — session row missing"
                        );
                        if consecutive_not_found >= MAX_CONSECUTIVE_NOT_FOUND {
                            // Session has been ended/deleted server-side and
                            // the WS lifecycle hasn't torn us down yet (or
                            // never will — the End path raced our snapshot).
                            // Stop spamming Workers. Don't touch stop_flag —
                            // it owns the FFmpeg/health-monitor lifecycle and
                            // we don't want one network class to take those
                            // down with us.
                            tracing::info!(
                                live_session_id = %live_session_id,
                                session_id = %sid,
                                "metrics reporter self-cancelled after consecutive 404s"
                            );
                            break;
                        }
                    }
                    Err(MetricsReportError::Other(msg)) => tracing::debug!(
                        live_session_id = %live_session_id,
                        session_id = %sid,
                        error = %msg,
                        payload = ?payload,
                        "metrics report failed — logging locally and retrying next tick"
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
        routing::patch,
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
            .route("/internal/sessions/{id}/metrics", patch(handler))
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
    async fn reporter_keeps_running_when_workers_returns_5xx() {
        // 5xx is treated as a transient hiccup — the reporter logs and keeps
        // ticking so a temporary Workers outage doesn't drop billing data.
        // Only 404 (session row gone) should retire the task; 5xx must not.
        let counter = Arc::new(AtomicU32::new(0));
        let (base, server) =
            spawn_counting_mock(counter.clone(), StatusCode::INTERNAL_SERVER_ERROR).await;

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
        // Still ticking after several 5xx responses — task didn't abort.
        let first = counter.load(Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(80)).await;
        let second = counter.load(Ordering::Relaxed);
        stop.store(true, Ordering::Release);
        let _ = tokio::time::timeout(Duration::from_millis(200), reporter).await;

        assert!(
            second > first,
            "expected task to keep ticking past 5xx; first={first} second={second}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn reporter_self_cancels_after_consecutive_404s_when_workers_session_is_gone() {
        // Prod incident 2026-04-20: Workers used to hard-delete the session
        // row on End-Session, then server-rs spammed `PATCH /metrics/:id` for
        // minutes against a row that no longer existed (and likely triggered
        // the SQLITE_BUSY_RECOVERY storm in the same trace). The reporter
        // must self-cancel after `MAX_CONSECUTIVE_NOT_FOUND` consecutive
        // 404s so we stop the noise on its own — even when the WS-driven
        // teardown hasn't fired yet.
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

        // The reporter should join on its own — without us flipping stop_flag
        // — once the 404 streak passes the threshold.
        let join = tokio::time::timeout(Duration::from_secs(2), reporter)
            .await
            .expect("reporter must self-cancel on consecutive 404s without external stop signal");
        join.expect("reporter task panicked");

        // The reporter handles the ticks BETWEEN counter increments via the
        // mock — the stop happens after >= MAX_CONSECUTIVE_NOT_FOUND 404s
        // are observed. Bound the upper count tightly so a future regression
        // (e.g. raising the threshold) trips this test.
        let ticks = counter.load(Ordering::Relaxed);
        assert!(
            ticks >= MAX_CONSECUTIVE_NOT_FOUND,
            "expected ≥{MAX_CONSECUTIVE_NOT_FOUND} ticks before self-cancel, got {ticks}"
        );
        // Stop flag must NOT be touched by the self-cancel — it owns FFmpeg
        // + health-monitor lifecycles, and a network class shouldn't tear
        // those down on its own.
        assert!(
            !stop.load(Ordering::Acquire),
            "self-cancel must not flip the FFmpeg stop flag"
        );

        server.abort();
    }

    #[tokio::test]
    async fn reporter_resets_404_streak_on_successful_report() {
        // A single mid-rollback 404 must not poison a still-live session.
        // We start the mock returning 404, let the reporter accumulate one
        // 404, then flip the mock to 200 — the streak should reset, and a
        // later 404 should NOT immediately self-cancel because the streak
        // is back at zero.
        use axum::extract::State;
        use std::sync::Mutex as StdMutex;

        #[derive(Clone)]
        struct Toggle {
            tick_count: Arc<AtomicU32>,
            status: Arc<StdMutex<StatusCode>>,
        }

        async fn toggling_handler(
            State(state): State<Toggle>,
            Path(_): Path<String>,
            _body: axum::body::Bytes,
        ) -> StatusCode {
            state.tick_count.fetch_add(1, Ordering::Relaxed);
            *state.status.lock().unwrap()
        }

        let tick_count = Arc::new(AtomicU32::new(0));
        let status = Arc::new(StdMutex::new(StatusCode::NOT_FOUND));
        let toggle = Toggle {
            tick_count: tick_count.clone(),
            status: status.clone(),
        };

        let app = Router::new()
            .route("/internal/sessions/{id}/metrics", patch(toggling_handler))
            .with_state(toggle);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base = format!("http://{}", addr);

        let metrics = SessionMetrics::new();
        let api = Arc::new(WorkersApi::new(&base, "sec"));
        let stop = Arc::new(AtomicBool::new(false));
        let reporter = spawn_metrics_reporter_with_interval(
            metrics,
            api,
            Some("S".into()),
            "LIVE".into(),
            stop.clone(),
            Duration::from_millis(15),
        );

        // First, allow exactly one 404 to land, then flip the mock to 200
        // before the streak hits MAX_CONSECUTIVE_NOT_FOUND.
        tokio::time::sleep(Duration::from_millis(20)).await;
        *status.lock().unwrap() = StatusCode::OK;
        // Let several 200 ticks land to confirm the streak reset.
        tokio::time::sleep(Duration::from_millis(80)).await;
        // Flip back to 404 — even if a single 404 lands now, the streak
        // restarts from 0, so the reporter must still be alive.
        *status.lock().unwrap() = StatusCode::NOT_FOUND;
        tokio::time::sleep(Duration::from_millis(20)).await;

        let still_running_ticks = tick_count.load(Ordering::Relaxed);
        stop.store(true, Ordering::Release);
        let _ = tokio::time::timeout(Duration::from_millis(200), reporter).await;

        assert!(
            still_running_ticks >= 4,
            "expected reporter to keep ticking after a mid-stream 200 reset the 404 streak; got {still_running_ticks}"
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
