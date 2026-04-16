use std::collections::VecDeque;

use crate::protocol::VideoChunk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoInsertOutcome {
    Inserted,
    DuplicateDropped,
    TooOldDropped,
}

#[derive(Debug, Default)]
pub struct VideoBuffer {
    chunks: VecDeque<VideoChunk>,
    max_depth_ms: u64,
}

impl VideoBuffer {
    pub fn new(max_depth_ms: u64) -> Self {
        Self {
            chunks: VecDeque::new(),
            max_depth_ms,
        }
    }

    pub fn insert(&mut self, chunk: VideoChunk, play_cursor_ms: u64) -> VideoInsertOutcome {
        if chunk.capture_ts_ms + chunk.duration_ms as u64 <= play_cursor_ms {
            return VideoInsertOutcome::TooOldDropped;
        }
        if self.chunks.iter().any(|existing| existing.seq == chunk.seq) {
            return VideoInsertOutcome::DuplicateDropped;
        }

        let index = self
            .chunks
            .iter()
            .position(|existing| {
                existing.capture_ts_ms > chunk.capture_ts_ms
                    || (existing.capture_ts_ms == chunk.capture_ts_ms && existing.seq > chunk.seq)
            })
            .unwrap_or(self.chunks.len());
        self.chunks.insert(index, chunk);
        self.prune_depth();
        VideoInsertOutcome::Inserted
    }

    pub fn pop_due(&mut self, play_cursor_ms: u64) -> Vec<VideoChunk> {
        let mut out = Vec::new();
        while self
            .chunks
            .front()
            .is_some_and(|chunk| chunk.capture_ts_ms <= play_cursor_ms)
        {
            if let Some(chunk) = self.chunks.pop_front() {
                out.push(chunk);
            }
        }
        out
    }

    pub fn prune_stale(&mut self, oldest_allowed_ts_ms: u64) -> usize {
        let before = self.chunks.len();
        while self
            .chunks
            .front()
            .is_some_and(|chunk| chunk.capture_ts_ms + (chunk.duration_ms as u64) < oldest_allowed_ts_ms)
        {
            self.chunks.pop_front();
        }
        before - self.chunks.len()
    }

    pub fn depth_ms(&self) -> u64 {
        match (self.chunks.front(), self.chunks.back()) {
            (Some(front), Some(back)) => {
                back.capture_ts_ms + back.duration_ms as u64 - front.capture_ts_ms
            }
            _ => 0,
        }
    }

    fn prune_depth(&mut self) {
        while self.depth_ms() > self.max_depth_ms {
            self.chunks.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::ChunkKind;

    use super::*;

    fn chunk(seq: u64, capture_ts_ms: u64, is_keyframe: bool) -> VideoChunk {
        VideoChunk {
            seq,
            capture_ts_ms,
            duration_ms: 33,
            is_keyframe,
            chunk_kind: if is_keyframe { ChunkKind::Init } else { ChunkKind::Media },
            bytes: vec![1, 2, 3],
        }
    }

    #[test]
    fn inserts_in_timestamp_order() {
        let mut buffer = VideoBuffer::new(1_000);
        buffer.insert(chunk(2, 66, false), 0);
        buffer.insert(chunk(1, 33, true), 0);

        let due = buffer.pop_due(66);
        assert_eq!(due[0].seq, 1);
        assert_eq!(due[1].seq, 2);
    }

    #[test]
    fn drops_duplicates() {
        let mut buffer = VideoBuffer::new(1_000);
        assert_eq!(buffer.insert(chunk(1, 33, true), 0), VideoInsertOutcome::Inserted);
        assert_eq!(
            buffer.insert(chunk(1, 33, true), 0),
            VideoInsertOutcome::DuplicateDropped
        );
    }

    #[test]
    fn drops_too_old_chunks() {
        let mut buffer = VideoBuffer::new(1_000);
        assert_eq!(
            buffer.insert(chunk(1, 33, true), 100),
            VideoInsertOutcome::TooOldDropped
        );
    }
}
