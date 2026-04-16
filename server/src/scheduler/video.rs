use crate::ingest::VideoBuffer;

use super::{clock::SchedulerClock, metrics::SchedulerMetrics, sink::VideoSink};

pub const VIDEO_LATE_DROP_THRESHOLD_MS: u64 = 100;

pub fn emit_due_video<S: VideoSink>(
    clock: &SchedulerClock,
    now: std::time::Instant,
    video_buffer: &mut VideoBuffer,
    sink: &mut S,
    metrics: &mut SchedulerMetrics,
) {
    let play_cursor_ms = clock.play_cursor_ms(now);
    for chunk in video_buffer.pop_due(play_cursor_ms) {
        if play_cursor_ms > chunk.capture_ts_ms.saturating_add(VIDEO_LATE_DROP_THRESHOLD_MS) {
            metrics.video_chunks_late_dropped += 1;
            continue;
        }
        metrics.current_video_play_ts_ms = chunk.capture_ts_ms;
        metrics.video_chunks_emitted += 1;
        sink.write_video_chunk(chunk);
    }
    metrics.video_buffer_depth_ms = video_buffer.depth_ms();
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::{
        ingest::{VideoBuffer, VideoInsertOutcome},
        protocol::{ChunkKind, VideoChunk},
        scheduler::sink::test_sink::RecordingVideoSink,
    };

    use super::*;

    fn chunk(seq: u64, capture_ts_ms: u64) -> VideoChunk {
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
    fn emits_due_video() {
        let start = Instant::now();
        let clock = SchedulerClock::new(start, 1_000);
        let mut buffer = VideoBuffer::new(2_000);
        assert_eq!(buffer.insert(chunk(1, 0), 0), VideoInsertOutcome::Inserted);
        let mut sink = RecordingVideoSink::default();
        let mut metrics = SchedulerMetrics::default();

        emit_due_video(
            &clock,
            start + Duration::from_millis(1_000),
            &mut buffer,
            &mut sink,
            &mut metrics,
        );

        assert_eq!(sink.emitted.len(), 1);
        assert_eq!(metrics.video_chunks_emitted, 1);
    }

    #[test]
    fn drops_very_late_video() {
        let start = Instant::now();
        let clock = SchedulerClock::new(start, 1_000);
        let mut buffer = VideoBuffer::new(2_000);
        assert_eq!(buffer.insert(chunk(1, 0), 0), VideoInsertOutcome::Inserted);
        let mut sink = RecordingVideoSink::default();
        let mut metrics = SchedulerMetrics::default();

        emit_due_video(
            &clock,
            start + Duration::from_millis(1_250),
            &mut buffer,
            &mut sink,
            &mut metrics,
        );

        assert!(sink.emitted.is_empty());
        assert_eq!(metrics.video_chunks_late_dropped, 1);
    }
}
