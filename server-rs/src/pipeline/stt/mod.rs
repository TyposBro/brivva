//! STT pipeline: Gladia connection, message processing, and transcript handling.

mod state;
mod connection;
mod message_handler;
mod final_handler;
mod interim_handler;
mod audio_forwarder;
mod reconnect_loop;

pub use reconnect_loop::start_stt;
