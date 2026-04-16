use std::time::Instant;

use crate::{
    ingest::{AudioBuffer, AudioInsertOutcome, VideoBuffer, VideoInsertOutcome},
    protocol::{AudioFrame, VideoChunk},
    scheduler::{AudioSink, Scheduler, VideoSink},
};

use super::state::SessionConfig;

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
