//! ElevenLabs Dubbing API — create dubs, poll status, download audio.

mod types;
mod client;

pub use types::DubbingStatus;
pub use client::{create_dubbing, poll_status, download_audio, CreateDubbingResult};
