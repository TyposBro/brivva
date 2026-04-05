//! ElevenLabs TTS integration — WebSocket streaming + REST fallback.

pub mod config;
pub mod voice_settings;
pub(crate) mod orchestrator;
pub(crate) mod ws;
pub(crate) mod rest;

// Re-export public API to maintain existing import paths
pub use crate::core::types::StyleParams;
pub use config::{TTS_API_KEY, DEFAULT_VOICE};
pub use orchestrator::{do_tts, TtsRequest};
pub use ws::do_tts_ws;
pub use rest::do_tts_rest;
pub use voice_settings::VoiceStyle;

/// All parameters for a single TTS synthesis call.
pub struct SynthesisRequest<'a> {
    pub text: &'a str,
    pub voice_id: &'a str,
    pub lang: &'a str,
    pub voice_settings: &'a serde_json::Value,
    pub max_bytes: usize,
    pub streaming: Option<&'a crate::streaming::StreamingPcm>,
    pub model_id: &'a str,
}

/// Abstraction over TTS providers.
/// Implementations: ElevenLabs WebSocket streaming + REST fallback.
pub trait Synthesizer: Send + Sync {
    fn synthesize(
        &self,
        req: &SynthesisRequest<'_>,
    ) -> impl std::future::Future<Output = Result<usize, String>> + Send;
}
