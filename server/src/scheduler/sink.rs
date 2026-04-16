use crate::protocol::{AudioFrame, VideoChunk};

pub trait AudioSink {
    fn write_audio_frame(&mut self, frame: AudioFrame);
    fn write_silence_frame(&mut self, capture_ts_ms: u64, duration_ms: u32);
}

pub trait VideoSink {
    fn write_video_chunk(&mut self, chunk: VideoChunk);
}

#[derive(Default)]
pub struct NoopAudioSink;

impl AudioSink for NoopAudioSink {
    fn write_audio_frame(&mut self, _frame: AudioFrame) {}

    fn write_silence_frame(&mut self, _capture_ts_ms: u64, _duration_ms: u32) {}
}

#[derive(Default)]
pub struct NoopVideoSink;

impl VideoSink for NoopVideoSink {
    fn write_video_chunk(&mut self, _chunk: VideoChunk) {}
}

pub mod test_sink {
    use crate::protocol::{AudioFrame, VideoChunk};

    use super::{AudioSink, VideoSink};

    #[derive(Default)]
    pub struct RecordingAudioSink {
        pub played: Vec<AudioFrame>,
        pub silence: Vec<(u64, u32)>,
    }

    impl AudioSink for RecordingAudioSink {
        fn write_audio_frame(&mut self, frame: AudioFrame) {
            self.played.push(frame);
        }

        fn write_silence_frame(&mut self, capture_ts_ms: u64, duration_ms: u32) {
            self.silence.push((capture_ts_ms, duration_ms));
        }
    }

    #[derive(Default)]
    pub struct RecordingVideoSink {
        pub emitted: Vec<VideoChunk>,
    }

    impl VideoSink for RecordingVideoSink {
        fn write_video_chunk(&mut self, chunk: VideoChunk) {
            self.emitted.push(chunk);
        }
    }
}
