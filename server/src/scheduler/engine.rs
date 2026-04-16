use std::time::Instant;

use crate::{
    ingest::{AudioBuffer, VideoBuffer},
    protocol::AudioFrame,
};

use super::{
    audio::{emit_due_audio, AUDIO_FRAME_DURATION_MS},
    clock::SchedulerClock,
    metrics::SchedulerMetrics,
    sink::{AudioSink, VideoSink},
    video::emit_due_video,
};

pub struct Scheduler {
    clock: SchedulerClock,
    pub audio_buffer: AudioBuffer,
    pub video_buffer: VideoBuffer,
    next_audio_play_ts_ms: Option<u64>,
    metrics: SchedulerMetrics,
}

impl Scheduler {
    pub fn new(server_session_start: Instant, configured_delay_ms: u64) -> Self {
        Self {
            clock: SchedulerClock::new(server_session_start, configured_delay_ms),
            audio_buffer: AudioBuffer::new(10_000),
            video_buffer: VideoBuffer::new(10_000),
            next_audio_play_ts_ms: None,
            metrics: SchedulerMetrics::default(),
        }
    }

    pub fn initialize_audio_cursor(&mut self, first_frame: &AudioFrame) {
        if self.next_audio_play_ts_ms.is_none() {
            self.next_audio_play_ts_ms = Some(first_frame.capture_ts_ms);
            self.metrics.current_audio_play_ts_ms =
                first_frame.capture_ts_ms.saturating_sub(AUDIO_FRAME_DURATION_MS);
        }
    }

    pub fn tick<A: AudioSink, V: VideoSink>(
        &mut self,
        now: Instant,
        audio_sink: &mut A,
        video_sink: &mut V,
    ) {
        emit_due_audio(
            &self.clock,
            now,
            &mut self.audio_buffer,
            &mut self.next_audio_play_ts_ms,
            audio_sink,
            &mut self.metrics,
        );
        emit_due_video(
            &self.clock,
            now,
            &mut self.video_buffer,
            video_sink,
            &mut self.metrics,
        );
    }

    pub fn reset_session_start(&mut self, now: Instant) {
        self.clock.reset_session_start(now);
        self.next_audio_play_ts_ms = None;
        self.metrics.current_audio_play_ts_ms = 0;
    }

    pub fn metrics(&self) -> &SchedulerMetrics {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::{
        ingest::{AudioInsertOutcome, VideoInsertOutcome},
        protocol::{AudioFrame, ChunkKind, VideoChunk},
        scheduler::sink::test_sink::{RecordingAudioSink, RecordingVideoSink},
    };

    use super::*;

    fn audio_frame(seq: u64, capture_ts_ms: u64) -> AudioFrame {
        AudioFrame {
            seq,
            capture_ts_ms,
            duration_ms: 20,
            pcm: vec![1; 1764],
        }
    }

    fn video_chunk(seq: u64, capture_ts_ms: u64) -> VideoChunk {
        VideoChunk {
            seq,
            capture_ts_ms,
            duration_ms: 33,
            is_keyframe: seq == 1,
            chunk_kind: ChunkKind::Media,
            bytes: vec![1, 2, 3],
        }
    }

    #[test]
    fn reset_session_start_resets_audio_cursor() {
        let start = Instant::now();
        let mut scheduler = Scheduler::new(start, 1_000);
        let audio = audio_frame(1, 0);
        scheduler.initialize_audio_cursor(&audio);
        assert!(scheduler.next_audio_play_ts_ms.is_some());
        scheduler.reset_session_start(start + Duration::from_millis(50));
        assert!(scheduler.next_audio_play_ts_ms.is_none());
    }

    #[test]
    fn drives_audio_and_video_together() {
        let start = Instant::now();
        let mut scheduler = Scheduler::new(start, 1_000);
        let audio = audio_frame(1, 0);
        scheduler.initialize_audio_cursor(&audio);
        assert_eq!(
            scheduler.audio_buffer.insert(audio, 0),
            AudioInsertOutcome::Inserted
        );
        assert_eq!(
            scheduler.video_buffer.insert(video_chunk(1, 0), 0),
            VideoInsertOutcome::Inserted
        );

        let mut audio_sink = RecordingAudioSink::default();
        let mut video_sink = RecordingVideoSink::default();

        scheduler.tick(start + Duration::from_millis(1_000), &mut audio_sink, &mut video_sink);

        assert_eq!(audio_sink.played.len(), 1);
        assert_eq!(video_sink.emitted.len(), 1);
    }
}
