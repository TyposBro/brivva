use std::time::Instant;

use serde::Serialize;

use crate::{
    ingest::{AudioBuffer, AudioInsertOutcome, VideoBuffer, VideoInsertOutcome},
    protocol::{AudioFrame, VideoChunk},
    scheduler::{AudioSink, Scheduler, VideoSink},
};

use super::state::SessionConfig;

#[derive(Debug, Clone, Serialize)]
pub struct SourceSessionSnapshot {
    pub delay_ms: u64,
    pub audio_buffer_depth_ms: u64,
    pub video_buffer_depth_ms: u64,
    pub audio_frames_played: u64,
    pub audio_frames_silence_filled: u64,
    pub audio_frames_late_dropped: u64,
    pub video_chunks_emitted: u64,
    pub video_chunks_late_dropped: u64,
    pub current_audio_play_ts_ms: u64,
    pub current_video_play_ts_ms: u64,
}

pub struct SourceSession {
    scheduler: Scheduler,
    config: SessionConfig,
}

impl SourceSession {
    pub fn new(now: Instant, config: SessionConfig) -> Self {
        Self {
            scheduler: Scheduler::new(now, config.delay_ms),
            config,
        }
    }

    pub fn reset_session_start(&mut self, now: Instant) {
        self.scheduler.reset_session_start(now);
    }

    pub fn push_audio(&mut self, frame: AudioFrame) -> AudioInsertOutcome {
        self.scheduler.initialize_audio_cursor(&frame);
        let play_cursor_ms = self.scheduler.metrics().current_audio_play_ts_ms;
        self.scheduler.audio_buffer.insert(frame, play_cursor_ms)
    }

    pub fn push_video(&mut self, chunk: VideoChunk) -> VideoInsertOutcome {
        let play_cursor_ms = self.scheduler.metrics().current_video_play_ts_ms;
        self.scheduler.video_buffer.insert(chunk, play_cursor_ms)
    }

    pub fn tick<A: AudioSink, V: VideoSink>(
        &mut self,
        now: Instant,
        audio_sink: &mut A,
        video_sink: &mut V,
    ) {
        self.scheduler.tick(now, audio_sink, video_sink);
    }

    pub fn audio_buffer(&self) -> &AudioBuffer {
        &self.scheduler.audio_buffer
    }

    pub fn video_buffer(&self) -> &VideoBuffer {
        &self.scheduler.video_buffer
    }

    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    pub fn metrics(&self) -> &crate::scheduler::SchedulerMetrics {
        self.scheduler.metrics()
    }

    pub fn snapshot(&self) -> SourceSessionSnapshot {
        let metrics = self.scheduler.metrics();
        SourceSessionSnapshot {
            delay_ms: self.config.delay_ms,
            audio_buffer_depth_ms: self.scheduler.audio_buffer.depth_ms(),
            video_buffer_depth_ms: self.scheduler.video_buffer.depth_ms(),
            audio_frames_played: metrics.audio_frames_played,
            audio_frames_silence_filled: metrics.audio_frames_silence_filled,
            audio_frames_late_dropped: metrics.audio_frames_late_dropped,
            video_chunks_emitted: metrics.video_chunks_emitted,
            video_chunks_late_dropped: metrics.video_chunks_late_dropped,
            current_audio_play_ts_ms: metrics.current_audio_play_ts_ms,
            current_video_play_ts_ms: metrics.current_video_play_ts_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::{
        protocol::{AudioFrame, ChunkKind, VideoChunk},
        scheduler::sink::test_sink::{RecordingAudioSink, RecordingVideoSink},
    };

    use super::*;

    #[test]
    fn source_session_emits_buffered_media() {
        let start = Instant::now();
        let mut session = SourceSession::new(start, SessionConfig { delay_ms: 1_000 });
        session.push_audio(AudioFrame {
            seq: 1,
            capture_ts_ms: 0,
            duration_ms: 20,
            pcm: vec![1; 1764],
        });
        session.push_video(VideoChunk {
            seq: 1,
            capture_ts_ms: 0,
            duration_ms: 33,
            is_keyframe: true,
            chunk_kind: ChunkKind::Init,
            bytes: vec![9, 8, 7],
        });

        let mut audio_sink = RecordingAudioSink::default();
        let mut video_sink = RecordingVideoSink::default();
        session.tick(start + Duration::from_millis(1_000), &mut audio_sink, &mut video_sink);

        assert_eq!(audio_sink.played.len(), 1);
        assert_eq!(video_sink.emitted.len(), 1);
    }
}
