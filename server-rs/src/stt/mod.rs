//! Gladia Solaria-1 real-time STT integration.
//!
//! Two-step connection: POST /v2/live to create session, then connect WebSocket.
//! Handles clause-boundary chunking, prosody analysis, and emotion classification.

pub mod types;
pub mod detectors;
pub mod prosody;

pub use types::*;
pub use detectors::*;
pub use prosody::*;
