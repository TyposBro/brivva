//! FFmpeg RTMP muxer: host video + translated audio → RTMP streams.
//!
//! Implements the "Fixed-Delay Jitter Buffer" pattern for video-audio sync:
//!
//! 1. Video frames are buffered with capture timestamps in a shared ring buffer
//! 2. A dedicated OS thread drains video at exactly 30fps (33.33ms ticks)
//! 3. A separate dedicated OS thread drains audio at 20ms ticks
//! 4. Both threads read from a shared delayed clock: `Instant::now() - D`
//! 5. TTS audio is queued and released when the delayed clock reaches utterance_start
//! 6. Between utterances, exact silence padding maintains cumulative sample count
//! 7. TTS calls have a hard timeout at D-500ms; missed utterances become silence
//!
//! Using OS threads (not Tokio tasks) ensures timing precision isn't affected
//! by async runtime contention from STT/translation/TTS futures.

pub mod video_drain;
pub mod audio_drain;
pub mod decoder;
mod process;
mod types;
mod manager;

// Re-export public API
pub use decoder::IncrementalMp3Decoder;
pub use process::{kill_orphan_ffmpeg, decode_mp3_to_pcm};
pub use types::{StreamingPcm, truncate_with_fadeout};
pub use manager::{RtmpManager, SharedRtmpManager, spawn_health_monitor};

// Re-export crate-internal items used by submodules (audio_drain, video_drain, decoder)
pub(crate) use process::FFMPEG_BIN;
pub(crate) use types::{
    QueuedAudio,
    AUDIO_TICK, AUDIO_BYTES_PER_TICK,
    JITTER_WARN_THRESHOLD, JITTER_RECOVERY_THRESHOLD,
    MAX_RECOVERY_TICKS,
    DRIFT_CHECK_INTERVAL_TICKS, DRIFT_WARN_THRESHOLD_MS,
    JITTER_WARN_LOG_INTERVAL,
};
