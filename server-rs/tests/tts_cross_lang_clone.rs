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
    Router, body::Body, extract::State, http::StatusCode, response::Response, routing::post,
};
use dashmap::DashMap;
use server_rs::features::broadcast::data::pipeline::tts::{TtsRequest, broadcast_translated_tts};
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
