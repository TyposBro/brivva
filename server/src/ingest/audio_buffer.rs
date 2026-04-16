use std::collections::VecDeque;

use crate::protocol::AudioFrame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioInsertOutcome {
    Inserted,
    DuplicateDropped,
    TooOldDropped,
}

#[derive(Debug, Default)]
pub struct AudioBuffer {
    frames: VecDeque<AudioFrame>,
    max_depth_ms: u64,
}

impl AudioBuffer {
    pub fn new(max_depth_ms: u64) -> Self {
        Self {
            frames: VecDeque::new(),
            max_depth_ms,
        }
    }

    pub fn insert(&mut self, frame: AudioFrame, play_cursor_ms: u64) -> AudioInsertOutcome {
        if frame.capture_ts_ms + frame.duration_ms as u64 <= play_cursor_ms {
            return AudioInsertOutcome::TooOldDropped;
        }
        if self.frames.iter().any(|existing| existing.seq == frame.seq) {
            return AudioInsertOutcome::DuplicateDropped;
        }

        let index = self
            .frames
            .iter()
            .position(|existing| {
                existing.capture_ts_ms > frame.capture_ts_ms
                    || (existing.capture_ts_ms == frame.capture_ts_ms && existing.seq > frame.seq)
            })
            .unwrap_or(self.frames.len());
        self.frames.insert(index, frame);
        self.prune_depth();
        AudioInsertOutcome::Inserted
    }

    pub fn pop_exact(&mut self, capture_ts_ms: u64) -> Option<AudioFrame> {
        let idx = self
            .frames
            .iter()
            .position(|frame| frame.capture_ts_ms == capture_ts_ms)?;
        self.frames.remove(idx)
    }

    pub fn prune_stale(&mut self, oldest_allowed_ts_ms: u64) -> usize {
        let before = self.frames.len();
        while self.frames.front().is_some_and(|frame| frame.capture_ts_ms < oldest_allowed_ts_ms) {
            self.frames.pop_front();
        }
        before - self.frames.len()
    }

    pub fn depth_ms(&self) -> u64 {
        match (self.frames.front(), self.frames.back()) {
            (Some(front), Some(back)) => {
                back.capture_ts_ms + back.duration_ms as u64 - front.capture_ts_ms
            }
            _ => 0,
        }
    }

    fn prune_depth(&mut self) {
        while self.depth_ms() > self.max_depth_ms {
            self.frames.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(seq: u64, capture_ts_ms: u64) -> AudioFrame {
        AudioFrame {
            seq,
            capture_ts_ms,
            duration_ms: 20,
            pcm: vec![0; 1764],
        }
    }

    #[test]
    fn inserts_in_timestamp_order() {
        let mut buffer = AudioBuffer::new(1_000);
        buffer.insert(frame(2, 40), 0);
        buffer.insert(frame(1, 20), 0);

        assert_eq!(buffer.pop_exact(20).unwrap().seq, 1);
        assert_eq!(buffer.pop_exact(40).unwrap().seq, 2);
    }

    #[test]
    fn drops_duplicates() {
        let mut buffer = AudioBuffer::new(1_000);
        assert_eq!(buffer.insert(frame(1, 20), 0), AudioInsertOutcome::Inserted);
        assert_eq!(buffer.insert(frame(1, 20), 0), AudioInsertOutcome::DuplicateDropped);
    }

    #[test]
    fn drops_too_old_frames() {
        let mut buffer = AudioBuffer::new(1_000);
        assert_eq!(buffer.insert(frame(1, 20), 100), AudioInsertOutcome::TooOldDropped);
    }
}
