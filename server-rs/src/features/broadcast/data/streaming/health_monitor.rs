//! Background health checker that detects crashed FFmpeg processes and restarts them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::features::broadcast::domain::{Sessions, ServerMsg};
use crate::features::broadcast::data::pipeline_helpers::to_ws;
use super::manager::{SharedRtmpManager, StreamConfig, RestartState};
use super::types::{HEALTH_CHECK_INTERVAL_SECS, FFMPEG_RESTART_DELAY};

struct HealthMonitor {
    manager: SharedRtmpManager,
    stop_flag: Arc<AtomicBool>,
    sessions: Sessions,
    session_id: String,
}

impl HealthMonitor {
    async fn run(self) {
        tracing::info!("[HEALTH] FFmpeg health monitor started (check every {}s)", HEALTH_CHECK_INTERVAL_SECS);
        let mut interval = tokio::time::interval(Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS));
        let mut check_count: u64 = 0;
        loop {
            interval.tick().await;
            if self.stop_flag.load(Ordering::Acquire) {
                tracing::info!("[HEALTH] stop signal received, exiting");
                break;
            }
            check_count += 1;
            self.detect_and_restart_crashes(&mut check_count).await;
        }
    }

    async fn detect_and_restart_crashes(&self, check_count: &mut u64) {
        let (crashed, exhausted) = {
            let mut mgr = self.manager.lock().await;
            let crashed = mgr.detect_crashed();
            let exhausted = mgr.drain_exhausted_streams();
            (crashed, exhausted)
        };
        if !crashed.is_empty() {
            tracing::warn!("[HEALTH] check #{}: {} crashed stream(s) detected", check_count, crashed.len());
        }
        for (config, state) in crashed {
            self.restart_one(config, state).await;
        }
        for (lang, restart_count) in exhausted {
            self.warn_host_exhausted(&lang, restart_count);
        }
    }

    async fn restart_one(
        &self,
        config: StreamConfig,
        state: RestartState,
    ) {
        tokio::time::sleep(FFMPEG_RESTART_DELAY).await;
        if self.stop_flag.load(Ordering::Acquire) {
            return;
        }
        let mut mgr = self.manager.lock().await;
        mgr.restart_stream(&config, state);
    }

    fn warn_host_exhausted(&self, lang: &str, restart_count: u32) {
        tracing::error!(
            "[HEALTH] stream lang={} permanently failed after {} restarts",
            lang, restart_count
        );
        if let Some(session) = self.sessions.get(&self.session_id) {
            session.send_to_host(to_ws(&ServerMsg::PipelineWarning {
                kind: "ffmpeg_restart_exhausted".to_string(),
                lang: lang.to_string(),
                detail: format!(
                    "RTMP stream for {} died after {} restart attempts",
                    lang, restart_count
                ),
                utterance_id: 0,
            }));
        }
    }
}

/// Spawn a background task that periodically checks for crashed FFmpeg processes
/// and restarts them. Runs every N seconds. Stops when stop_flag is set.
/// Sends PipelineWarning to the host when a stream exhausts all restart attempts.
pub fn spawn_health_monitor(
    manager: SharedRtmpManager,
    stop_flag: Arc<AtomicBool>,
    sessions: Sessions,
    session_id: String,
) -> tokio::task::JoinHandle<()> {
    let monitor = HealthMonitor { manager, stop_flag, sessions, session_id };
    tokio::spawn(monitor.run())
}
