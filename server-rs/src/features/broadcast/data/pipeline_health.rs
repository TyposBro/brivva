//! Periodic task that sends PipelineHealth to the host every 5 seconds.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::features::broadcast::domain::{Sessions, ServerMsg};
use super::pipeline_helpers::to_ws;
use super::streaming::SharedRtmpManager;

const HEALTH_REPORT_INTERVAL_SECS: u64 = 5;

struct HealthReportDeps {
    session_id: String,
    sessions: Sessions,
    rtmp_manager: SharedRtmpManager,
    stop_flag: Arc<AtomicBool>,
}

struct PipelineHealthReporter {
    deps: HealthReportDeps,
}

impl PipelineHealthReporter {
    async fn run(self) {
        tracing::info!(
            "[HEALTH:{}] pipeline health reporter started (every {}s)",
            self.deps.session_id, HEALTH_REPORT_INTERVAL_SECS,
        );
        let mut interval = tokio::time::interval(
            Duration::from_secs(HEALTH_REPORT_INTERVAL_SECS),
        );
        loop {
            interval.tick().await;
            if self.deps.stop_flag.load(Ordering::Acquire) {
                tracing::info!(
                    "[HEALTH:{}] pipeline health reporter stopped",
                    self.deps.session_id,
                );
                break;
            }
            self.send_health_snapshot().await;
        }
    }

    async fn send_health_snapshot(&self) {
        let (queue_depth, drift_ms) = self.collect_stream_metrics().await;
        let stt_connected = self.is_session_active();
        let counters = self.read_counters();
        let msg = ServerMsg::PipelineHealth {
            stt_connected,
            queue_depth,
            drift_ms,
            dropped_chunks: counters.tts_failures + counters.tts_timeouts,
            tts_timeouts: counters.tts_timeouts,
            translate_errors: counters.translation_empty,
            avg_latency_ms: counters.avg_latency_ms,
            tts_failures: counters.tts_failures,
            stt_disconnects: counters.stt_disconnects,
        };
        self.send_to_host(&msg);
    }

    async fn collect_stream_metrics(&self) -> (HashMap<String, usize>, HashMap<String, u64>) {
        let mgr = self.deps.rtmp_manager.lock().await;
        (mgr.queue_depths(), mgr.drift_per_lang())
    }

    fn is_session_active(&self) -> bool {
        self.deps.sessions.get(&self.deps.session_id).is_some()
    }

    fn read_counters(&self) -> HealthCounterSnapshot {
        let session = match self.deps.sessions.get(&self.deps.session_id) {
            Some(s) => s,
            None => return HealthCounterSnapshot::default(),
        };
        HealthCounterSnapshot {
            tts_failures: session.pipeline_counters.tts_failures(),
            tts_timeouts: session.pipeline_counters.tts_timeouts(),
            stt_disconnects: session.pipeline_counters.stt_disconnects(),
            translation_empty: session.pipeline_counters.translation_empty(),
            avg_latency_ms: session.latency_tracker.rolling_average_ms(),
        }
    }

    fn send_to_host(&self, msg: &ServerMsg) {
        if let Some(session) = self.deps.sessions.get(&self.deps.session_id) {
            session.send_to_host(to_ws(msg));
        }
    }
}

#[derive(Default)]
struct HealthCounterSnapshot {
    tts_failures: u64,
    tts_timeouts: u64,
    stt_disconnects: u64,
    translation_empty: u64,
    avg_latency_ms: u64,
}

/// Spawn a background task that sends PipelineHealth to the host every 5 seconds.
/// Stops when stop_flag is set (session cleanup).
pub fn spawn_pipeline_health_reporter(
    session_id: String,
    sessions: Sessions,
    rtmp_manager: SharedRtmpManager,
    stop_flag: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    let deps = HealthReportDeps {
        session_id,
        sessions,
        rtmp_manager,
        stop_flag,
    };
    let reporter = PipelineHealthReporter { deps };
    tokio::spawn(reporter.run())
}
