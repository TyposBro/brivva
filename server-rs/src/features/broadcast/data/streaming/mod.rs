//! FFmpeg RTMP muxer: host video + translated audio → RTMP streams.
//!
//! Video is delayed by broadcast_delay seconds.
//! Audio plays immediately as TTS completes — no A/V sync scheduling.
//!
//! Using OS threads (not Tokio tasks) ensures timing precision isn't affected
//! by async runtime contention from STT/translation/TTS futures.

pub mod video_drain;
pub mod audio_drain;
pub mod decoder;
mod process;
mod types;
mod manager;
mod ffmpeg_spawn;
mod stream_lifecycle;
mod health_monitor;

// Re-export public API
pub use decoder::IncrementalMp3Decoder;
pub use process::{kill_orphan_ffmpeg, decode_mp3_to_pcm};
pub use types::{StreamingPcm, truncate_with_fadeout};
pub use manager::{RtmpManager, SharedRtmpManager, erase_rtmp_manager, downcast_rtmp_manager};
pub use health_monitor::spawn_health_monitor;

// Re-export crate-internal items used by submodules
pub(crate) use process::FFMPEG_BIN;
pub(crate) use types::{
    QueuedAudio,
    AUDIO_TICK, AUDIO_BYTES_PER_TICK,
    JITTER_WARN_THRESHOLD, JITTER_RECOVERY_THRESHOLD,
    MAX_RECOVERY_TICKS,
    JITTER_WARN_LOG_INTERVAL,
    MAX_AUDIO_QUEUE_DEPTH,
};
