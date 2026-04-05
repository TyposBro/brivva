//! ElevenLabs TTS integration — WebSocket streaming + REST fallback.

pub mod config;
pub mod voice_settings;
pub(crate) mod orchestrator;
pub(crate) mod ws;
pub(crate) mod rest;

// Re-export public API to maintain existing import paths
pub use config::{StyleParams, TTS_API_KEY, DEFAULT_VOICE};
pub use orchestrator::{do_tts, TtsRequest};
pub use ws::do_tts_ws;
pub use rest::do_tts_rest;
pub use voice_settings::VoiceStyle;
