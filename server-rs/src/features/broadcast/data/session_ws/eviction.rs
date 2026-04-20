use crate::features::broadcast::domain::LiveSession;

/// Returns every live_session_id whose `session_id` matches `target_session_id`,
/// excluding `incoming_live_session_id`. Pure scan over the dashmap so the
/// "is there a stale row?" decision can be unit-tested without spawning a WS.
pub(super) fn find_live_sessions_for(
    live_sessions: &dashmap::DashMap<String, LiveSession>,
    target_session_id: &str,
    incoming_live_session_id: &str,
) -> Vec<String> {
    live_sessions
        .iter()
        .filter(|entry| {
            entry.key() != incoming_live_session_id
                && entry
                    .value()
                    .session_id
                    .as_deref()
                    .is_some_and(|sid| sid == target_session_id)
        })
        .map(|entry| entry.key().clone())
        .collect()
}

/// Tear down any stale live_session rows tied to the same FE `session_id`
/// before a new WS upgrade installs its own row. Closes the old host_tx
/// (which lets the dangling send_task exit), stops every FFmpeg child via
/// the rtmp_manager, and removes the entry. Idempotent — safe to call when
/// no stale row exists.
pub(super) async fn evict_stale_live_sessions(
    live_sessions: &dashmap::DashMap<String, LiveSession>,
    target_session_id: &str,
    incoming_live_session_id: &str,
) {
    let stale_ids =
        find_live_sessions_for(live_sessions, target_session_id, incoming_live_session_id);
    for stale_id in stale_ids {
        let Some((_, mut stale)) = live_sessions.remove(&stale_id) else {
            continue;
        };
        // Drop host_tx explicitly so the prior connection's send_task exits
        // promptly — the receiver side returns None on the next recv().
        stale.host_tx = None;
        if let Some(manager) = stale.rtmp_manager.take() {
            let mut mgr = manager.lock().await;
            mgr.stop_all().await;
        }
        tracing::warn!(
            session_id = %target_session_id,
            stale_live_session_id = %stale_id,
            new_live_session_id = %incoming_live_session_id,
            "evicted stale live_session before binding new WS — prior connection lingered"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::broadcast::domain::{Lang, LiveSessions, PipelineConfig};
    use axum::extract::ws::Message;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn live_session_with_session_id(
        live_session_id: &str,
        session_id: Option<&str>,
    ) -> LiveSession {
        LiveSession::new(
            live_session_id.into(),
            Lang::En,
            session_id.map(|s| s.to_string()),
            Arc::new(PipelineConfig::default()),
        )
    }

    #[test]
    fn find_live_sessions_for_returns_empty_when_no_match_for_target_session_id() {
        let map = dashmap::DashMap::new();
        map.insert(
            "OLD001".into(),
            live_session_with_session_id("OLD001", Some("other-session")),
        );
        let stale = find_live_sessions_for(&map, "target-session", "NEW001");
        assert!(stale.is_empty());
    }

    #[test]
    fn find_live_sessions_for_excludes_the_incoming_live_session_id() {
        // The newly minted live_session has not been inserted yet, but a
        // future refactor that flips the order must never wipe its own row.
        let map = dashmap::DashMap::new();
        map.insert(
            "NEW001".into(),
            live_session_with_session_id("NEW001", Some("target-session")),
        );
        let stale = find_live_sessions_for(&map, "target-session", "NEW001");
        assert!(stale.is_empty());
    }

    #[test]
    fn find_live_sessions_for_collects_every_stale_row_for_one_session_id() {
        let map = dashmap::DashMap::new();
        map.insert(
            "OLD001".into(),
            live_session_with_session_id("OLD001", Some("target-session")),
        );
        map.insert(
            "OLD002".into(),
            live_session_with_session_id("OLD002", Some("target-session")),
        );
        map.insert(
            "OTHER1".into(),
            live_session_with_session_id("OTHER1", Some("unrelated")),
        );
        let mut stale = find_live_sessions_for(&map, "target-session", "NEW001");
        stale.sort();
        assert_eq!(stale, vec!["OLD001".to_string(), "OLD002".into()]);
    }

    #[test]
    fn find_live_sessions_for_ignores_rows_with_no_session_id() {
        // Quick-start sessions (no Workers session_id) must never be evicted
        // by a different session_id's reconnect.
        let map = dashmap::DashMap::new();
        map.insert(
            "QSTART".into(),
            live_session_with_session_id("QSTART", None),
        );
        let stale = find_live_sessions_for(&map, "target-session", "NEW001");
        assert!(stale.is_empty());
    }

    #[tokio::test]
    async fn evict_stale_live_sessions_removes_matching_row_and_drops_host_tx() {
        let live_sessions: LiveSessions = Arc::new(dashmap::DashMap::new());
        let mut stale = live_session_with_session_id("OLD001", Some("session-1"));
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        stale.host_tx = Some(tx);
        live_sessions.insert("OLD001".into(), stale);
        live_sessions.insert(
            "OTHER1".into(),
            live_session_with_session_id("OTHER1", Some("session-2")),
        );

        evict_stale_live_sessions(&live_sessions, "session-1", "NEW001").await;

        assert!(!live_sessions.contains_key("OLD001"));
        assert!(live_sessions.contains_key("OTHER1"));
        // Receiver returns None once the sender (host_tx) is dropped.
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn evict_stale_live_sessions_is_noop_when_nothing_matches() {
        let live_sessions: LiveSessions = Arc::new(dashmap::DashMap::new());
        live_sessions.insert(
            "OTHER1".into(),
            live_session_with_session_id("OTHER1", Some("session-2")),
        );
        evict_stale_live_sessions(&live_sessions, "session-1", "NEW001").await;
        assert_eq!(live_sessions.len(), 1);
        assert!(live_sessions.contains_key("OTHER1"));
    }
}
