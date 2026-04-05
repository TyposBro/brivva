//! Gladia Solaria-1 real-time STT integration.

pub mod types;
pub mod config;
pub mod markers;
pub mod detectors;
pub mod prosody;

pub(crate) mod reconnect;
pub(super) mod state;
pub(super) mod connection;
pub(super) mod handler;
pub(super) mod final_handler;
pub(super) mod interim_handler;
pub(super) mod audio_forwarder;

pub use types::*;
pub use detectors::*;
pub use prosody::*;
pub use reconnect::{start_stt, SttStartRequest};

use std::sync::LazyLock;

pub(crate) static STT_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("STT_API_KEY").unwrap_or_default()
});
