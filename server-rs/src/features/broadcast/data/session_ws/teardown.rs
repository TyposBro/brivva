use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::features::broadcast::data::workers_api::WorkersApi;
use crate::features::broadcast::domain::LiveSessions;

pub(super) struct TeardownArgs<'a> {
    pub live_sessions: &'a LiveSessions,
    pub live_session_id: &'a str,
    pub workers_api: &'a Arc<WorkersApi>,
    pub ffmpeg_monitor_stop: Arc<AtomicBool>,
}

pub(super) async fn teardown_session(args: TeardownArgs<'_>) {
    let TeardownArgs {
        live_sessions,
        live_session_id,
        workers_api,
        ffmpeg_monitor_stop,
    } = args;
    ffmpeg_monitor_stop.store(true, Ordering::Release);
    let Some((_, live_session)) = live_sessions.remove(live_session_id) else {
        return;
    };
    if let Some(manager) = live_session.rtmp_manager {
        let mut mgr = manager.lock().await;
        mgr.stop_all().await;
    }
    if let Some(sid) = live_session.session_id {
        let workers_for_end = workers_api.clone();
        tokio::spawn(async move {
            if let Err(e) = workers_for_end
                .update_session_status(&sid, "ended", None)
                .await
            {
                tracing::warn!(
                    session_id = %sid,
                    error = %e,
                    "workers status=ended update failed"
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::broadcast::domain::{Lang, LiveSession, PipelineConfig};

    #[tokio::test]
    async fn teardown_session_is_noop_when_live_session_id_missing() {
        let live_sessions: LiveSessions = Arc::new(dashmap::DashMap::new());
        let workers_api = Arc::new(WorkersApi::new("", ""));
        let stop = Arc::new(AtomicBool::new(false));
        teardown_session(TeardownArgs {
            live_sessions: &live_sessions,
            live_session_id: "MISSING",
            workers_api: &workers_api,
            ffmpeg_monitor_stop: stop.clone(),
        })
        .await;
        assert!(
            stop.load(Ordering::Acquire),
            "stop flag still set on noop path"
        );
    }

    #[tokio::test]
    async fn teardown_session_removes_live_session_and_sets_stop_flag() {
        let live_sessions: LiveSessions = Arc::new(dashmap::DashMap::new());
        live_sessions.insert(
            "LIVE01".into(),
            LiveSession::new(
                "LIVE01".into(),
                Lang::En,
                None,
                Arc::new(PipelineConfig::default()),
            ),
        );
        let workers_api = Arc::new(WorkersApi::new("", ""));
        let stop = Arc::new(AtomicBool::new(false));
        teardown_session(TeardownArgs {
            live_sessions: &live_sessions,
            live_session_id: "LIVE01",
            workers_api: &workers_api,
            ffmpeg_monitor_stop: stop.clone(),
        })
        .await;
        assert!(live_sessions.is_empty());
        assert!(stop.load(Ordering::Acquire));
    }
}
