//! ElevenLabs TTS integration — WebSocket streaming + REST fallback.

pub mod config;
pub mod voice_settings;
pub(crate) mod dashscope_ws;
pub(crate) mod orchestrator;
pub(crate) mod ws;
pub(crate) mod rest;
pub(crate) mod warmup;

// Re-export public API to maintain existing import paths
pub use crate::core::types::StyleParams;
pub use orchestrator::{do_tts, TtsEnv, TtsRequest};
pub use ws::do_tts_ws;
pub use rest::do_tts_rest;
pub use voice_settings::VoiceStyle;
pub use warmup::warm_up_tts_ws;
pub use dashscope_ws::{do_tts_dashscope, DASHSCOPE_TTS_WS_URL, DASHSCOPE_TTS_MODEL_VC};

/// All parameters for a single TTS synthesis call.
pub struct SynthesisRequest<'a> {
    pub text: &'a str,
    pub voice_id: &'a str,
    pub lang: &'a str,
    pub voice_settings: &'a serde_json::Value,
    pub max_bytes: usize,
    pub streaming: Option<&'a crate::features::broadcast::data::streaming::StreamingPcm>,
    pub model_id: &'a str,
    pub api_key: &'a str,
    /// True when voice_id is a user-cloned voice. Skips language_code in the
    /// ElevenLabs WS URL to avoid accent mixing (Korean clone + language_code=en
    /// produces Indian-accented English).
    pub is_cloned_voice: bool,
}

/// Abstraction over TTS providers.
/// Implementations: ElevenLabs WebSocket streaming + REST fallback.
pub trait Synthesizer: Send + Sync {
    fn synthesize(
        &self,
        req: &SynthesisRequest<'_>,
    ) -> impl std::future::Future<Output = Result<usize, String>> + Send;
}
