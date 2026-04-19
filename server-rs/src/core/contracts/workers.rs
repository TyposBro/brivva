// @generated — DO NOT EDIT.
//
// Rust bindings for the subset of the Workers OpenAPI schema that
// server-rs deserializes on the /internal/sessions/* endpoints.
//
// Regenerate with:  bun run --cwd contracts gen:rust
// Source of truth:  contracts/openapi/brivva-workers.json

#![allow(clippy::too_many_lines)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub voice_id: Option<String>,
    pub title: String,
    pub source_lang: String,
    pub target_langs: String,
    pub status: String,
    pub live_session_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Stream {
    pub id: String,
    pub session_id: String,
    pub lang: String,
    pub platform: String,
    pub platform_broadcast_id: Option<String>,
    pub platform_stream_id: Option<String>,
    pub stream_key: Option<String>,
    pub rtmp_url: Option<String>,
    pub status: String,
    pub delay_ms: u64,
    pub host_gain: f32,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Voice {
    pub id: String,
    pub user_id: String,
    pub elevenlabs_voice_id: String,
    pub name: String,
    pub source_lang: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionBundle {
    pub session: Session,
    pub streams: Vec<Stream>,
    pub voice: Option<Voice>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionStatusUpdate {
    pub status: String,
    #[serde(default)]
    pub live_session_id: Option<String>,
}
