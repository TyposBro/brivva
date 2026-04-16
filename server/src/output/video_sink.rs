use std::io::Write;

use crate::{protocol::VideoChunk, scheduler::VideoSink};

pub struct ChunkVideoSink<W: Write> {
    writer: W,
}

impl<W: Write> ChunkVideoSink<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: Write> VideoSink for ChunkVideoSink<W> {
    fn write_video_chunk(&mut self, chunk: VideoChunk) {
        let _ = self.writer.write_all(&chunk.bytes);
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::ChunkKind;

    use super::*;

    #[test]
    fn writes_video_chunk_bytes() {
        let mut sink = ChunkVideoSink::new(Vec::<u8>::new());
        sink.write_video_chunk(VideoChunk {
            seq: 1,
            capture_ts_ms: 0,
            duration_ms: 33,
            is_keyframe: true,
            chunk_kind: ChunkKind::Init,
            bytes: vec![7, 8, 9],
        });

        assert_eq!(sink.into_inner(), vec![7, 8, 9]);
    }
}
