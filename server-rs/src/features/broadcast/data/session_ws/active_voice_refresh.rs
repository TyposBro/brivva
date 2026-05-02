//! Mid-session active-voice refresh.
//!
//! The one-voice-per-user invariant means `POST /api/voices` can upsert a
//! new clone while a session is live. Workers swaps the owner's
//! `active_voice_id`, but Fargate has already cached
//! `live_session.selected_voice_id` at bootstrap. Without this refresher the
//! stale clone keeps dispatching until the host closes + reopens the WS.
//!
//! This module:
//!   1. Re-fetches the `/internal/sessions/:id` bundle (which Workers
//!      computes from `users.active_voice_id`, NOT `session.voice_id`).
//!   2. If the bundle's voice differs from the live session's cached
//!      voice, updates the cached voice id + enrollment_lang in place.
//!
//! The next TTS dispatch then picks up the new voice — no session restart.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::features::broadcast::data::workers_api::WorkersApi;
use crate::features::broadcast::domain::{Lang, LiveSessions};

/// Default cadence between refreshes. Small enough that a re-record
/// propagates quickly, large enough to keep Workers HTTP load negligible.
pub(super) const ACTIVE_VOICE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// One-shot refresh. Public so integration tests + the periodic spawn
/// can share the same logic. Returns `true` iff the cached voice was
/// actually swapped on this tick.
pub async fn refresh_active_voice_once(
    workers_api: &WorkersApi,
    sid: &str,
    live_session_id: &str,
    live_sessions: &LiveSessions,
) -> bool {
    let bundle = match workers_api.fetch_session_bundle(sid).await {
        Ok(b) => b,
        Err(e) => {
            // Transient bundle failure is noisy but not fatal — the next
            // tick will try again. Emit so operators can grep.
            tracing::warn!(
                session_id = %sid,
                error = %e,
                "active-voice refresh: bundle fetch failed — retaining cached voice"
            );
            return false;
        }
    };
    let Some(voice) = bundle.voice else {
        // Owner has no active clone right now (voice deleted, or never
        // had one). Don't clobber the cache — the TTS dispatcher already
        // falls back to the per-lang default when selected_voice_id is
        // None, and a mid-session delete is rare enough that a silent
        // retain is preferable to a hard swap.
        tracing::info!(
            session_id = %sid,
            "active-voice refresh: bundle has no voice — cache unchanged"
        );
        return false;
    };
    let Some(mut entry) = live_sessions.get_mut(live_session_id) else {
        // Session torn down between the HTTP round-trip and here. No-op.
        tracing::info!(
            session_id = %sid,
            live_session_id = %live_session_id,
            "active-voice refresh: live session gone — skipping update"
        );
        return false;
    };
    let current = entry.selected_voice_id.as_deref();
    if current == Some(voice.elevenlabs_voice_id.as_str()) {
        return false;
    }
    tracing::info!(
        session_id = %sid,
        live_session_id = %live_session_id,
        old_voice_id = ?current,
        new_voice_id = %voice.elevenlabs_voice_id,
        "active-voice refresh: applying new clone mid-session"
    );
    entry.selected_voice_id = Some(voice.elevenlabs_voice_id.clone());
    entry.selected_voice_enrollment_lang = voice.source_lang.as_deref().and_then(Lang::from_str);
    true
}

/// Spawn the periodic refresher. Stops when `stop_flag` is set (shares
/// the lifecycle with the ffmpeg monitor + metrics reporter).
pub(super) fn spawn_active_voice_refresh(
    workers_api: Arc<WorkersApi>,
    sid: String,
    live_session_id: String,
    live_sessions: LiveSessions,
    stop_flag: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    spawn_active_voice_refresh_with_interval(
        workers_api,
        sid,
        live_session_id,
        live_sessions,
        stop_flag,
        ACTIVE_VOICE_REFRESH_INTERVAL,
    )
}

/// Internal variant for tests that need to drive the cadence without
/// waiting 5s per tick.
pub(super) fn spawn_active_voice_refresh_with_interval(
    workers_api: Arc<WorkersApi>,
    sid: String,
    live_session_id: String,
    live_sessions: LiveSessions,
    stop_flag: Arc<AtomicBool>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Skip the immediate first tick — LiveSession may not be in the
        // DashMap yet (bootstrap_session returns before the outer insert).
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if stop_flag.load(Ordering::Acquire) {
                break;
            }
            let _ = refresh_active_voice_once(
                workers_api.as_ref(),
                &sid,
                &live_session_id,
                &live_sessions,
            )
            .await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::contracts::workers::{Session, SessionBundle, Voice};
    use crate::features::broadcast::domain::{LiveSession, PipelineConfig};
    use axum::{
        Router,
        extract::{Path, State},
        http::StatusCode,
        response::IntoResponse,
        routing::get,
    };
    use dashmap::DashMap;
    use std::sync::Mutex;
    use tokio::task::JoinHandle;

    type BundleProvider = Arc<Mutex<Box<dyn Fn() -> Option<SessionBundle> + Send + Sync>>>;

    async fn spawn_bundle_mock(provider: BundleProvider) -> (String, JoinHandle<()>) {
        async fn handler(
            State(state): State<BundleProvider>,
            Path(_sid): Path<String>,
        ) -> impl IntoResponse {
            let maybe = state.lock().unwrap()();
            match maybe {
                Some(bundle) => (StatusCode::OK, axum::Json(bundle)).into_response(),
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
        let app = Router::new()
            .route("/internal/sessions/{id}", get(handler))
            .with_state(provider);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let h = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{}", addr), h)
    }

    fn make_session() -> Session {
        Session {
            id: "sess-1".into(),
            user_id: "u-1".into(),
            voice_id: Some("v-A".into()),
            title: "t".into(),
            source_lang: "ko".into(),
            target_langs: "[\"ja\"]".into(),
            status: "live".into(),
            live_session_id: Some("LIVE-1".into()),
            voice_preset: "cloned".into(),
            created_at: 0,
        }
    }

    fn make_voice(id: &str, el: &str, lang: Option<&str>) -> Voice {
        Voice {
            id: id.into(),
            user_id: "u-1".into(),
            elevenlabs_voice_id: el.into(),
            name: "v".into(),
            source_lang: lang.map(str::to_string),
            created_at: 0,
        }
    }

    fn seed_live_session(voice_id: Option<String>, enrollment: Option<Lang>) -> LiveSessions {
        let mut live = LiveSession::new(
            "LIVE-1".into(),
            Lang::Ko,
            Some("sess-1".into()),
            Arc::new(PipelineConfig {
                soniox_api_key: "".into(),
                soniox_ws_url: "".into(),
                elevenlabs_api_key: "".into(),
                elevenlabs_base_url: "".into(),
                force_default_voice: false,
                force_rtmp_not_rtmps: false,
                v2_output_controls: false,
                v2_render_graph: false,
                v2_shared_decode: false,
                v2_gpu_workers: false,
                v2_encoded_fanout: false,
                video_encoder: Default::default(),
            }),
        );
        live.selected_voice_id = voice_id;
        live.selected_voice_enrollment_lang = enrollment;
        let sessions: LiveSessions = Arc::new(DashMap::new());
        sessions.insert("LIVE-1".into(), live);
        sessions
    }

    #[tokio::test]
    async fn refresh_once_applies_new_clone_when_active_voice_swapped_mid_session() {
        // Re-record scenario: live session was bootstrapped with voice A;
        // Workers now reports voice B is active. Refresh must swap the
        // cached clone in place so next TTS dispatch picks up B.
        let bundle_b = SessionBundle {
            session: make_session(),
            streams: vec![],
            voice: Some(make_voice("v-B", "EL-voice-B", Some("ko"))),
        };
        let provider: BundleProvider =
            Arc::new(Mutex::new(Box::new(move || Some(bundle_b.clone()))));
        let (base, _h) = spawn_bundle_mock(provider).await;

        let sessions = seed_live_session(Some("EL-voice-A".into()), Some(Lang::Ko));
        let api = WorkersApi::new(&base, "sec");

        let swapped = refresh_active_voice_once(&api, "sess-1", "LIVE-1", &sessions).await;
        assert!(swapped, "expected refresh to apply the new voice");

        let entry = sessions.get("LIVE-1").expect("live session present");
        assert_eq!(entry.selected_voice_id.as_deref(), Some("EL-voice-B"));
        assert_eq!(entry.selected_voice_enrollment_lang, Some(Lang::Ko));
    }

    #[tokio::test]
    async fn refresh_once_is_noop_when_cached_voice_already_matches() {
        let bundle = SessionBundle {
            session: make_session(),
            streams: vec![],
            voice: Some(make_voice("v-A", "EL-voice-A", Some("ko"))),
        };
        let provider: BundleProvider = Arc::new(Mutex::new(Box::new(move || Some(bundle.clone()))));
        let (base, _h) = spawn_bundle_mock(provider).await;

        let sessions = seed_live_session(Some("EL-voice-A".into()), Some(Lang::Ko));
        let api = WorkersApi::new(&base, "sec");

        let swapped = refresh_active_voice_once(&api, "sess-1", "LIVE-1", &sessions).await;
        assert!(!swapped, "already-current voice must not trigger a swap");
    }

    #[tokio::test]
    async fn refresh_once_retains_cache_when_bundle_voice_is_none() {
        // Mid-session delete edge: the user deleted their clone but the
        // live session is still running. Prior cached voice stays (TTS
        // dispatcher already falls back to per-lang defaults when
        // selected_voice_id is None — we just don't clobber).
        let bundle = SessionBundle {
            session: make_session(),
            streams: vec![],
            voice: None,
        };
        let provider: BundleProvider = Arc::new(Mutex::new(Box::new(move || Some(bundle.clone()))));
        let (base, _h) = spawn_bundle_mock(provider).await;

        let sessions = seed_live_session(Some("EL-voice-A".into()), Some(Lang::Ko));
        let api = WorkersApi::new(&base, "sec");

        let swapped = refresh_active_voice_once(&api, "sess-1", "LIVE-1", &sessions).await;
        assert!(!swapped);
        let entry = sessions.get("LIVE-1").unwrap();
        assert_eq!(entry.selected_voice_id.as_deref(), Some("EL-voice-A"));
    }

    #[tokio::test]
    async fn refresh_once_is_noop_when_live_session_torn_down() {
        let bundle = SessionBundle {
            session: make_session(),
            streams: vec![],
            voice: Some(make_voice("v-B", "EL-voice-B", Some("ko"))),
        };
        let provider: BundleProvider = Arc::new(Mutex::new(Box::new(move || Some(bundle.clone()))));
        let (base, _h) = spawn_bundle_mock(provider).await;

        let sessions: LiveSessions = Arc::new(DashMap::new()); // empty
        let api = WorkersApi::new(&base, "sec");

        let swapped = refresh_active_voice_once(&api, "sess-1", "LIVE-1", &sessions).await;
        assert!(!swapped);
    }

    #[tokio::test]
    async fn spawn_ticks_on_interval_and_stops_on_flag() {
        let bundle_b = SessionBundle {
            session: make_session(),
            streams: vec![],
            voice: Some(make_voice("v-B", "EL-voice-B", Some("ko"))),
        };
        let provider: BundleProvider =
            Arc::new(Mutex::new(Box::new(move || Some(bundle_b.clone()))));
        let (base, _server) = spawn_bundle_mock(provider).await;

        let sessions = seed_live_session(Some("EL-voice-A".into()), Some(Lang::Ko));
        let api = Arc::new(WorkersApi::new(&base, "sec"));
        let stop = Arc::new(AtomicBool::new(false));
        let task = spawn_active_voice_refresh_with_interval(
            api,
            "sess-1".into(),
            "LIVE-1".into(),
            sessions.clone(),
            stop.clone(),
            Duration::from_millis(20),
        );

        // Wait long enough for at least two ticks (first is skipped).
        tokio::time::sleep(Duration::from_millis(120)).await;
        stop.store(true, Ordering::Release);
        // Give the loop one more tick to observe the stop flag.
        tokio::time::sleep(Duration::from_millis(60)).await;
        task.abort();

        let entry = sessions.get("LIVE-1").unwrap();
        assert_eq!(entry.selected_voice_id.as_deref(), Some("EL-voice-B"));
    }
}
