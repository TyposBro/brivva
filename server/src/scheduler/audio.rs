use crate::{
    ingest::AudioBuffer,
    protocol::AudioFrame,
};

use super::{
    clock::SchedulerClock,
    metrics::SchedulerMetrics,
    sink::AudioSink,
};

pub const AUDIO_FRAME_DURATION_MS: u64 = 20;
pub const AUDIO_LATE_DROP_THRESHOLD_MS: u64 = 40;
pub const AUDIO_PCM_BYTES_PER_FRAME: usize = 1764;

pub fn emit_due_audio<S: AudioSink>(
    clock: &SchedulerClock,
    now: std::time::Instant,
    audio_buffer: &mut AudioBuffer,
    next_audio_play_ts_ms: &mut Option<u64>,
    sink: &mut S,
    metrics: &mut SchedulerMetrics,
) {
    if next_audio_play_ts_ms.is_none() {
        *next_audio_play_ts_ms = audio_buffer
            .pop_exact(0)
            .map(|frame| {
                let ts = frame.capture_ts_ms;
                sink.write_audio_frame(frame);
                metrics.audio_frames_played += 1;
                metrics.current_audio_play_ts_ms = ts;
                ts + AUDIO_FRAME_DURATION_MS
            });
    }

    while let Some(target_ts_ms) = *next_audio_play_ts_ms {
        if clock.play_deadline(target_ts_ms) > now {
            break;
        }

        if let Some(frame) = audio_buffer.pop_exact(target_ts_ms) {
            metrics.audio_frames_played += 1;
            metrics.current_audio_play_ts_ms = frame.capture_ts_ms;
            sink.write_audio_frame(frame);
        } else {
            sink.write_silence_frame(target_ts_ms, AUDIO_FRAME_DURATION_MS as u32);
            metrics.audio_frames_silence_filled += 1;
            metrics.current_audio_play_ts_ms = target_ts_ms;
        }

        let stale_before = target_ts_ms.saturating_sub(AUDIO_LATE_DROP_THRESHOLD_MS);
        metrics.audio_frames_late_dropped += audio_buffer.prune_stale(stale_before) as u64;
        *next_audio_play_ts_ms = Some(target_ts_ms + AUDIO_FRAME_DURATION_MS);
    }

    metrics.audio_buffer_depth_ms = audio_buffer.depth_ms();
}

pub fn silence_frame(capture_ts_ms: u64) -> AudioFrame {
    AudioFrame {
        seq: 0,
        capture_ts_ms,
        duration_ms: AUDIO_FRAME_DURATION_MS as u32,
        pcm: vec![0; AUDIO_PCM_BYTES_PER_FRAME],
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::{ingest::AudioInsertOutcome, scheduler::sink::test_sink::RecordingAudioSink};

    use super::*;

    fn frame(seq: u64, capture_ts_ms: u64) -> AudioFrame {
        AudioFrame {
            seq,
            capture_ts_ms,
            duration_ms: AUDIO_FRAME_DURATION_MS as u32,
            pcm: vec![1; AUDIO_PCM_BYTES_PER_FRAME],
        }
    }

    #[test]
    fn emits_exact_frame_when_due() {
        let start = Instant::now();
        let clock = SchedulerClock::new(start, 1_000);
        let mut buffer = AudioBuffer::new(2_000);
        assert_eq!(buffer.insert(frame(1, 0), 0), AudioInsertOutcome::Inserted);
        assert_eq!(buffer.insert(frame(2, 20), 0), AudioInsertOutcome::Inserted);
        let mut sink = RecordingAudioSink::default();
        let mut metrics = SchedulerMetrics::default();
        let mut next = Some(0);

        emit_due_audio(
            &clock,
            start + Duration::from_millis(1_020),
            &mut buffer,
            &mut next,
            &mut sink,
            &mut metrics,
        );

        assert_eq!(sink.played.len(), 2);
        assert_eq!(metrics.audio_frames_played, 2);
        assert_eq!(next, Some(40));
    }

    #[test]
    fn fills_silence_when_frame_missing() {
        let start = Instant::now();
        let clock = SchedulerClock::new(start, 1_000);
        let mut buffer = AudioBuffer::new(2_000);
        let mut sink = RecordingAudioSink::default();
        let mut metrics = SchedulerMetrics::default();
        let mut next = Some(0);

        emit_due_audio(
            &clock,
            start + Duration::from_millis(1_000),
            &mut buffer,
            &mut next,
            &mut sink,
            &mut metrics,
        );

        assert_eq!(sink.silence, vec![(0, 20)]);
        assert_eq!(metrics.audio_frames_silence_filled, 1);
        assert_eq!(next, Some(20));
    }
}
