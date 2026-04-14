use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrivvaError {
    #[error("STT: {0}")]
    Stt(String),
    #[error("Translation: {0}")]
    Translation(String),
    #[error("TTS: {0}")]
    Tts(String),
    #[error("FFmpeg: {0}")]
    Ffmpeg(String),
    #[error("Voice clone: {0}")]
    VoiceClone(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("WebSocket: {0}")]
    WebSocket(String),
    #[error("Dubbing: {0}")]
    Dubbing(String),
}

pub type Result<T> = std::result::Result<T, BrivvaError>;
