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

/// Abstraction over TTS providers.
/// Implementations: ElevenLabs WebSocket streaming + REST fallback.
pub trait Synthesizer: Send + Sync {
    fn synthesize(
        &self,
        text: &str,
        voice_id: &str,
        lang: &str,
        voice_settings: &serde_json::Value,
        max_bytes: usize,
        streaming: Option<&crate::ffmpeg::StreamingPcm>,
        model_id: &str,
    ) -> impl std::future::Future<Output = Result<usize, String>> + Send;
}
