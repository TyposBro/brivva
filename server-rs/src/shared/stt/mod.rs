//! Soniox v4 real-time STT integration.

pub mod types;
pub mod config;
pub mod prosody;

pub(crate) mod reconnect;
pub(super) mod state;
pub(super) mod connection;
pub(super) mod handler;
pub(super) mod final_handler;
pub(super) mod interim_handler;

pub use types::*;
pub use prosody::*;
pub use reconnect::{start_stt, SttStartRequest};
