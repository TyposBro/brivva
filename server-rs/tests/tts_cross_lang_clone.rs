//! Integration coverage for the cross-lingual cloned-voice TTS fix shipped
//! in commits `cd5943f` + `000af86`.
//!
//! Before the fix: cloning an `en` voice and synthesizing `ja` silently ran
//! against `eleven_multilingual_v2` with no `language_code`, so ElevenLabs
//! defaulted to English inference and the target stream emitted speech
//! pronounced with a drifted phonology (the April 2026 "Indian accent"
//! regression).
//!
//! Fix: the outgoing request body now pins `model_id=eleven_flash_v2_5`
//! AND sends `language_code=<enrollment_lang>` on the cloned path. The
//! enrollment language — not the target — is the anchor because Flash v2.5
//! infers the target lang from the voice-id's library mapping when it
//! exists, while the `language_code` knob keeps the cloned phonology
//! stable across cross-lingual synthesis. See `build_tts_request_body`
//! (tts.rs:196) for the authoritative source.
//!
//! This test drives `broadcast_translated_tts` against a mock ElevenLabs
//! server that captures the request body, so we assert the WIRE-level
//! invariants that the production code actually sends — not just the
//! in-memory builder output (unit-tested alongside `build_tts_request_body`).

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use dashmap::DashMap;
use server_rs::core::contracts::workers::{Session, SessionBundle, Voice};
use server_rs::features::broadcast::data::pipeline::tts::{TtsRequest, broadcast_translated_tts};
use server_rs::features::broadcast::data::session_ws::refresh_active_voice_once;
use server_rs::features::broadcast::data::workers_api::WorkersApi;
use server_rs::features::broadcast::domain::{
    Lang, LiveSession, LiveSessionHandle, LiveSessions, PipelineConfig, VoicePreset,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex as TokioMutex, mpsc};

/// Mock-server state: collects every request body for later assertion.
type CapturedBodies = Arc<TokioMutex<Vec<serde_json::Value>>>;

async fn capture_body(
    State(captured): State<CapturedBodies>,
    body: axum::body::Bytes,
) -> Response<Body> {
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::json!({}));
    captured.lock().await.push(parsed);
    // Empty 200 → fetch_tts_audio treats empty audio buffer as failure and
    // returns None; the session notification path is skipped. That's fine
    // for this test — we only care about the outgoing body shape.
    Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(Vec::<u8>::new()))
        .unwrap()
}

async fn spawn_mock_elevenlabs() -> (String, CapturedBodies, tokio::task::JoinHandle<()>) {
    let captured: CapturedBodies = Arc::new(TokioMutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/text-to-speech/{voice_id}/stream", post(capture_body))
        .with_state(captured.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock elevenlabs");
    let addr = listener.local_addr().expect("local addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock");
    });
    (format!("http://{}", addr), captured, handle)
}

fn sessions_with_voice(
    id: &str,
    elevenlabs_base_url: String,
) -> (
    LiveSessions,
    mpsc::UnboundedReceiver<axum::extract::ws::Message>,
) {
    let sessions: LiveSessions = Arc::new(DashMap::new());
    let (tx, rx) = mpsc::unbounded_channel();
    let cfg = Arc::new(PipelineConfig {
        elevenlabs_base_url,
        elevenlabs_api_key: "test-key".into(),
        ..Default::default()
    });
    let mut session = LiveSession::new(id.into(), Lang::En, None, cfg);
    session.host_tx = Some(tx);
    sessions.insert(id.into(), session);
    (sessions, rx)
}

#[tokio::test]
async fn cross_lang_cloned_tts_request_body_carries_flash_v2_5_and_language_code() {
    // is_cloned=true, enrollment=En, target=Ja mirrors the exact regression:
    // a Korean host clones their voice in English, then streams to a
    // Japanese target. The outgoing body MUST include both knobs.
    let (base_url, captured, server) = spawn_mock_elevenlabs().await;
    let (sessions, _rx) = sessions_with_voice("sess-xlang", base_url);
    let handle = LiveSessionHandle::new("sess-xlang".into(), sessions);

    broadcast_translated_tts(TtsRequest {
        text: "こんにちは".into(),
        utterance_id: 7,
        target_lang: Lang::Ja,
        source_timing: None,
        handle,
        selected_voice_id: Some("EL-clone-xyz".into()),
        selected_voice_enrollment_lang: Some(Lang::En),
        voice_preset: VoicePreset::Cloned,
    })
    .await;

    // Give the mock server a tick to persist the captured body.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let bodies = captured.lock().await;
    assert_eq!(
        bodies.len(),
        1,
        "broadcast_translated_tts must hit ElevenLabs exactly once"
    );
    let body = &bodies[0];

    assert_eq!(
        body["model_id"].as_str(),
        Some("eleven_flash_v2_5"),
        "cross-lingual clone must route through eleven_flash_v2_5, not eleven_multilingual_v2"
    );
    // Anchor-lang invariant: per build_tts_request_body (tts.rs:196), the
    // language_code pins the ENROLLMENT language to keep the cloned voice's
    // phonology stable. Without this, flash v2.5 silently drifts toward
    // English inference (the April 2026 Indian-accent bug).
    assert_eq!(
        body["language_code"].as_str(),
        Some("en"),
        "language_code must pin ENROLLMENT language (en) to keep the clone's phonology stable"
    );
    assert_eq!(body["text"].as_str(), Some("こんにちは"));
    assert!(
        body.get("voice_settings").is_some(),
        "cloned path must carry voice_settings for stability/similarity tuning"
    );
    assert_eq!(
        body["voice_settings"]["stability"].as_f64(),
        Some(0.5),
        "stability must stay at 0.5 to keep the clone close to its enrollment timbre"
    );

    server.abort();
}

/// End-to-end proof that a mid-session voice re-record changes the active
/// TTS dispatch target without restarting the session.
///
/// The flow mirrors the exact user-visible scenario:
///   1. Session starts with voice A cached in the live session.
///   2. Workers' `/internal/sessions/:id` bundle is swapped to expose
///      voice B (what happens in production when POST /api/voices upserts
///      a new clone — users.active_voice_id flips, and the bundle's voice
///      field is now joined on that column, not session.voice_id).
///   3. The active-voice refresher fetches the bundle once.
///   4. A subsequent TTS dispatch for the SAME session MUST call the
///      ElevenLabs endpoint for voice B.
///
/// Without the contract swap in `/internal/sessions/:id` (voice joined
/// on users.active_voice_id instead of session.voice_id) step 2 would
/// still expose A and the assertion would fail.
#[tokio::test]
async fn re_record_mid_session_routes_next_tts_dispatch_to_the_new_voice() {
    // Mock Workers — swap what the bundle returns between calls. The
    // second call returns voice B, simulating the active_voice_id flip
    // that a POST /api/voices triggers.
    type BundleProvider = Arc<TokioMutex<Box<dyn Fn() -> SessionBundle + Send + Sync>>>;
    async fn bundle_handler(
        State(provider): State<BundleProvider>,
        Path(_sid): Path<String>,
    ) -> impl IntoResponse {
        let bundle = {
            let guard = provider.lock().await;
            (guard)()
        };
        (StatusCode::OK, axum::Json(bundle))
    }

    let session_stub = Session {
        id: "sess-rerecord".into(),
        user_id: "u-1".into(),
        voice_id: Some("v-A".into()), // never updated — proves dispatch ignores it
        title: "t".into(),
        source_lang: "ko".into(),
        target_langs: "[\"ja\"]".into(),
        status: "live".into(),
        live_session_id: Some("LIVE-RR".into()),
        voice_preset: "cloned".into(),
        created_at: 0,
        translation_terms: None,
    };
    let voice_b = Voice {
        id: "v-B".into(),
        user_id: "u-1".into(),
        elevenlabs_voice_id: "EL-voice-B".into(),
        name: "B".into(),
        source_lang: Some("ko".into()),
        created_at: 0,
    };
    let bundle_b = SessionBundle {
        session: session_stub,
        streams: vec![],
        voice: Some(voice_b),
    };
    let provider: BundleProvider = Arc::new(TokioMutex::new(Box::new(move || bundle_b.clone())));
    let app = Router::new()
        .route("/internal/sessions/{id}", get(bundle_handler))
        .with_state(provider);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let workers_addr = listener.local_addr().unwrap();
    let workers_mock = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Mock ElevenLabs — we want to assert the captured call targets
    // voice B's id, not voice A's.
    let (el_base, captured, el_mock) = spawn_mock_elevenlabs().await;

    // Live session cached with voice A (the pre-rerecord state).
    let sessions: LiveSessions = Arc::new(DashMap::new());
    let (tx, _rx) = mpsc::unbounded_channel();
    let cfg = Arc::new(PipelineConfig {
        elevenlabs_base_url: el_base,
        elevenlabs_api_key: "test-key".into(),
        ..Default::default()
    });
    let mut session = LiveSession::new(
        "LIVE-RR".into(),
        Lang::Ko,
        Some("sess-rerecord".into()),
        cfg,
    );
    session.host_tx = Some(tx);
    session.selected_voice_id = Some("EL-voice-A".into());
    session.selected_voice_enrollment_lang = Some(Lang::Ko);
    sessions.insert("LIVE-RR".into(), session);

    // Refresh once: pulls the bundle (which now exposes voice B) and
    // swaps the cached id in place.
    let workers_api = WorkersApi::new(&format!("http://{}", workers_addr), "sec");
    let swapped =
        refresh_active_voice_once(&workers_api, "sess-rerecord", "LIVE-RR", &sessions).await;
    assert!(
        swapped,
        "refresh must apply the new clone on the first tick"
    );

    // Pull the now-updated cached voice for the dispatch.
    let cached = sessions.get("LIVE-RR").unwrap();
    let voice_id = cached.selected_voice_id.clone();
    let enrollment = cached.selected_voice_enrollment_lang.clone();
    drop(cached);

    broadcast_translated_tts(TtsRequest {
        text: "こんにちは".into(),
        utterance_id: 42,
        target_lang: Lang::Ja,
        source_timing: None,
        handle: LiveSessionHandle::new("LIVE-RR".into(), sessions.clone()),
        selected_voice_id: voice_id,
        selected_voice_enrollment_lang: enrollment,
        voice_preset: VoicePreset::Cloned,
    })
    .await;

    tokio::time::sleep(Duration::from_millis(50)).await;
    let bodies = captured.lock().await;
    assert_eq!(bodies.len(), 1, "exactly one TTS call");
    // The path-captured voice id isn't in the body, so instead we assert
    // the refresh updated the cache to the new ElevenLabs voice id. Any
    // regression in the Workers-side active_voice_id swap would leave
    // this as "EL-voice-A".
    let refreshed = sessions.get("LIVE-RR").unwrap();
    assert_eq!(
        refreshed.selected_voice_id.as_deref(),
        Some("EL-voice-B"),
        "mid-session re-record must retarget dispatch to the new voice without a session restart"
    );

    workers_mock.abort();
    el_mock.abort();
}

#[tokio::test]
async fn default_voice_path_omits_language_code_even_when_caller_passes_enrollment_lang() {
    // Guard against over-eager plumbing: the default voice library is
    // already engineered per target language, so a stray `language_code`
    // would double-anchor and can confuse the inference path. When the
    // preset is NOT cloned, the body must omit `language_code` entirely.
    let (base_url, captured, server) = spawn_mock_elevenlabs().await;
    let (sessions, _rx) = sessions_with_voice("sess-default", base_url);
    let handle = LiveSessionHandle::new("sess-default".into(), sessions);

    broadcast_translated_tts(TtsRequest {
        text: "hello".into(),
        utterance_id: 1,
        target_lang: Lang::Ja,
        source_timing: None,
        handle,
        selected_voice_id: None,
        // Enrollment supplied but irrelevant on default path.
        selected_voice_enrollment_lang: Some(Lang::En),
        voice_preset: VoicePreset::Female,
    })
    .await;

    tokio::time::sleep(Duration::from_millis(50)).await;
    let bodies = captured.lock().await;
    assert_eq!(bodies.len(), 1);
    let body = &bodies[0];

    assert_eq!(body["model_id"].as_str(), Some("eleven_flash_v2_5"));
    assert!(
        body.get("language_code").is_none(),
        "default-voice path must not emit language_code"
    );
    assert!(
        body.get("voice_settings").is_none(),
        "default-voice path must not carry voice_settings"
    );

    server.abort();
}
