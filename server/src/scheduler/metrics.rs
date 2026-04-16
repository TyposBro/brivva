use serde::Serialize;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct SchedulerMetrics {
    pub audio_frames_played: u64,
    pub audio_frames_silence_filled: u64,
    pub audio_frames_late_dropped: u64,
    pub video_chunks_emitted: u64,
    pub video_chunks_late_dropped: u64,
    pub audio_buffer_depth_ms: u64,
    pub video_buffer_depth_ms: u64,
    pub current_audio_play_ts_ms: u64,
    pub current_video_play_ts_ms: u64,
}
