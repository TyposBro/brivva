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
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(METRICS_INTERVAL);
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
