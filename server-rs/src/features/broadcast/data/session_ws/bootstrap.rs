use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::core::contracts::workers::SessionBundle;
use crate::features::broadcast::data::metrics::spawn_metrics_reporter;
use crate::features::broadcast::data::workers_api::WorkersApi;
use crate::features::broadcast::domain::{Lang, LiveSession, LiveSessions, SessionMetrics};

use super::active_voice_refresh::spawn_active_voice_refresh;
use super::rtmp::{RtmpStartArgs, start_rtmp_streams};

pub(super) enum BootstrapOutcome {
    Continue,
    /// Abort silently (auth failure, session-owner mismatch — the FE was
    /// never going to succeed so we close without an explanatory frame).
    Abort,
    /// Abort with a user-visible message. The host receives an
    /// `{"type":"error","message":..}` frame before the socket closes so the
    /// FE renders a banner instead of hanging.
    AbortWithError(String),
}

pub(super) struct BootstrapArgs<'a> {
    pub workers_api: &'a Arc<WorkersApi>,
    pub sid: &'a str,
    pub user_id: &'a str,
    pub source_lang: &'a Lang,
    pub live_session: &'a mut LiveSession,
    pub live_session_id: &'a str,
    pub ffmpeg_monitor_stop: Arc<AtomicBool>,
    pub live_sessions: LiveSessions,
}

pub(super) async fn bootstrap_session(args: BootstrapArgs<'_>) -> BootstrapOutcome {
    let BootstrapArgs {
        workers_api,
        sid,
        user_id,
        source_lang,
        live_session,
        live_session_id,
        ffmpeg_monitor_stop,
        live_sessions,
    } = args;

    let bundle = match workers_api.fetch_session_bundle(sid).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(session_id = %sid, error = %e, "session bundle fetch failed");
            return BootstrapOutcome::Abort;
        }
    };

    if bundle.session.user_id != user_id {
        tracing::warn!(
            session_id = %sid,
            owner_user_id = %bundle.session.user_id,
            jwt_user_id = %user_id,
            "ws host upgrade rejected: session owner mismatch"
        );
        return BootstrapOutcome::Abort;
    }

    // Fail-fast on any stream missing RTMP ingest info. Previously this path
    // silently skipped the stream, which produced the "Waiting for
    // utterances..." ghost-session bug when Workers hadn't auto-filled a
    // YouTube row. Surface the error to the host instead so they can fix it.
    if let StreamPreflight::MissingRtmp { stream_id, lang } = preflight_streams(&bundle) {
        tracing::error!(
            session_id = %sid,
            stream_id = %stream_id,
            lang = %lang,
            "session aborted: stream has no RTMP ingest — platform broadcast was never created"
        );
        return BootstrapOutcome::AbortWithError(format!(
            "Stream {stream_id} ({lang}) has no RTMP URL. The destination's broadcast was never created — check the connection to the platform and try again."
        ));
    }

    if let Some(v) = bundle.voice.clone() {
        live_session.selected_voice_id = Some(v.elevenlabs_voice_id);
        // Populate enrollment_lang from the Voice row so the TTS dispatcher
        // can (a) detect cross-lingual mismatch and fall back to the default
        // voice, and (b) emit an explicit `language_code` in the
        // ElevenLabs body so `eleven_multilingual_v2` infers in the
        // enrollment language instead of silently defaulting to English —
        // the cause of the April 2026 Indian-accent regression. Unknown /
        // legacy codes that Lang::from_str rejects quietly stay None;
        // tts.rs already treats None as "let ElevenLabs decide".
        live_session.selected_voice_enrollment_lang =
            v.source_lang.as_deref().and_then(Lang::from_str);
    }
    live_session.voice_preset =
        crate::features::broadcast::domain::VoicePreset::from_wire(&bundle.session.voice_preset);

    let metrics = SessionMetrics::new();
    live_session.metrics = Some(metrics.clone());
    let (provider_tx, mut provider_rx) = tokio::sync::mpsc::unbounded_channel();
    live_session.provider_health_tx = Some(provider_tx);
    let provider_workers_api = workers_api.clone();
    let provider_sid = sid.to_string();
    let provider_live_session_id = live_session_id.to_string();
    let provider_stop = ffmpeg_monitor_stop.clone();
    let _provider_failure_reporter = tokio::spawn(async move {
        while !provider_stop.load(std::sync::atomic::Ordering::Acquire) {
            let Some(event) = provider_rx.recv().await else {
                break;
            };
            if event.billable && event.state == "live" {
                continue;
            }
            match provider_workers_api
                .report_provider_failure(&provider_sid, Some(&provider_live_session_id), &event)
                .await
            {
                Ok(()) => {}
                Err(error) => tracing::warn!(
                    session_id = %provider_sid,
                    provider = %event.provider,
                    state = %event.state,
                    reason = %event.reason,
                    error = %error,
                    "provider failure report failed"
                ),
            }
        }
    });

    start_rtmp_streams(RtmpStartArgs {
        bundle: &bundle,
        source_lang,
        live_session,
        sid,
        ffmpeg_monitor_stop: ffmpeg_monitor_stop.clone(),
        metrics: metrics.clone(),
    });
    spawn_live_status_update(
        workers_api.clone(),
        sid.to_string(),
        live_session_id.to_string(),
    );
    let _metrics_reporter = spawn_metrics_reporter(
        metrics,
        workers_api.clone(),
        Some(sid.to_string()),
        live_session_id.to_string(),
        ffmpeg_monitor_stop.clone(),
    );
    // Optional mid-session voice upsert watcher. It is useful only if a host
    // re-records their voice while already live. Keep it off by default during
    // live media work: the 5s poll adds local Workers load and is unnecessary
    // for the normal launch flow where voice setup happens before Go Live.
    if std::env::var("BRIVVA_ACTIVE_VOICE_REFRESH")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        let _voice_refresher = spawn_active_voice_refresh(
            workers_api.clone(),
            sid.to_string(),
            live_session_id.to_string(),
            live_sessions,
            ffmpeg_monitor_stop,
        );
    }
    BootstrapOutcome::Continue
}

/// Outcome of pre-flighting the stream list. `Ok(..)` means we can proceed;
/// `Err(..)` carries a user-visible message routed back to the FE as a host
/// `error:` text frame before the socket is closed.
pub enum StreamPreflight {
    Ok,
    MissingRtmp { stream_id: String, lang: String },
}

/// Inspects every stream in the bundle and rejects the session if any
/// non-passthrough row is missing RTMP ingest data. Passthrough streams
/// (`lang == PASS_LANG_CODE`) legitimately skip start_stream today — they
/// still need rtmp + key, so they are checked too. The previous "silently
/// continue" behavior here was the production bug: a YouTube row landed in
/// D1 with NULL rtmp_url/stream_key (no server-side auto-create) and the
/// session hung on "Waiting for utterances...".
pub fn preflight_streams(bundle: &SessionBundle) -> StreamPreflight {
    for s in &bundle.streams {
        if s.rtmp_url.as_deref().unwrap_or("").is_empty()
            || s.stream_key.as_deref().unwrap_or("").is_empty()
        {
            return StreamPreflight::MissingRtmp {
                stream_id: s.id.clone(),
                lang: s.lang.clone(),
            };
        }
    }
    StreamPreflight::Ok
}

fn spawn_live_status_update(workers_api: Arc<WorkersApi>, sid: String, live_session_id: String) {
    tokio::spawn(async move {
        if let Err(e) = workers_api
            .update_session_status(&sid, "live", Some(&live_session_id))
            .await
        {
            tracing::warn!(
                session_id = %sid,
                error = %e,
                "workers status=live update failed"
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::contracts::workers::{Session, Stream};

    fn stub_session() -> Session {
        Session {
            id: "sess-1".into(),
            user_id: "u-1".into(),
            voice_id: None,
            title: "t".into(),
            source_lang: "en".into(),
            target_langs: "[\"ja\"]".into(),
            status: "setup".into(),
            live_session_id: None,
            voice_preset: "female".into(),
            created_at: 0,
        }
    }

    fn stream_with(rtmp: Option<&str>, key: Option<&str>) -> Stream {
        Stream {
            id: "st-1".into(),
            session_id: "sess-1".into(),
            lang: "ja".into(),
            platform: "youtube".into(),
            platform_broadcast_id: None,
            platform_stream_id: None,
            stream_key: key.map(|s| s.to_string()),
            rtmp_url: rtmp.map(|s| s.to_string()),
            status: "ready".into(),
            delay_ms: 2000,
            host_gain: 0.2,
            created_at: 0,
            watch_url: None,
        }
    }

    #[test]
    fn preflight_streams_accepts_fully_populated_stream() {
        let bundle = SessionBundle {
            session: stub_session(),
            streams: vec![stream_with(
                Some("rtmp://a.rtmp.youtube.com/live2/"),
                Some("key-abc"),
            )],
            voice: None,
        };
        assert!(matches!(preflight_streams(&bundle), StreamPreflight::Ok));
    }

    #[test]
    fn preflight_streams_rejects_missing_rtmp_url_on_non_passthrough_stream() {
        let bundle = SessionBundle {
            session: stub_session(),
            streams: vec![stream_with(None, Some("key-abc"))],
            voice: None,
        };
        match preflight_streams(&bundle) {
            StreamPreflight::MissingRtmp { stream_id, lang } => {
                assert_eq!(stream_id, "st-1");
                assert_eq!(lang, "ja");
            }
            StreamPreflight::Ok => panic!("expected MissingRtmp"),
        }
    }

    #[test]
    fn preflight_streams_rejects_missing_stream_key() {
        let bundle = SessionBundle {
            session: stub_session(),
            streams: vec![stream_with(Some("rtmp://a.rtmp.youtube.com/live2/"), None)],
            voice: None,
        };
        assert!(matches!(
            preflight_streams(&bundle),
            StreamPreflight::MissingRtmp { .. }
        ));
    }

    #[test]
    fn preflight_streams_rejects_empty_string_rtmp_url() {
        // D1 can store `""` (not just NULL) — the pre-flight treats both the
        // same way so a legacy migration path can't slip an empty string past.
        let bundle = SessionBundle {
            session: stub_session(),
            streams: vec![stream_with(Some(""), Some("key"))],
            voice: None,
        };
        assert!(matches!(
            preflight_streams(&bundle),
            StreamPreflight::MissingRtmp { .. }
        ));
    }

    #[test]
    fn preflight_streams_accepts_empty_stream_bundle() {
        // A quick-start session with no destinations is still valid — the
        // host is testing STT without pushing RTMP anywhere. Pre-flight must
        // not trip on an empty streams vec.
        let bundle = SessionBundle {
            session: stub_session(),
            streams: vec![],
            voice: None,
        };
        assert!(matches!(preflight_streams(&bundle), StreamPreflight::Ok));
    }
}
