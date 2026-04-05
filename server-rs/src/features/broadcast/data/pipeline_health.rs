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
        let queue_depth = self.collect_queue_depths().await;
        let stt_connected = self.is_session_active();
        let msg = ServerMsg::PipelineHealth {
            stt_connected,
            queue_depth,
            dropped_chunks: 0,
            tts_timeouts: 0,
            translate_errors: 0,
        };
        self.send_to_host(&msg);
    }

    async fn collect_queue_depths(&self) -> HashMap<String, usize> {
        let mgr = self.deps.rtmp_manager.lock().await;
        mgr.queue_depths()
    }

    fn is_session_active(&self) -> bool {
        self.deps.sessions.get(&self.deps.session_id).is_some()
    }

    fn send_to_host(&self, msg: &ServerMsg) {
        if let Some(session) = self.deps.sessions.get(&self.deps.session_id) {
            session.send_to_host(to_ws(msg));
        }
    }
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

