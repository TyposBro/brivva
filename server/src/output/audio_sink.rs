use std::io::{self, Write};

use crate::{
    protocol::AudioFrame,
    scheduler::{audio::AUDIO_PCM_BYTES_PER_FRAME, AudioSink},
};

pub struct PcmAudioSink<W: Write> {
    writer: W,
}

impl<W: Write> PcmAudioSink<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: Write> AudioSink for PcmAudioSink<W> {
    fn write_audio_frame(&mut self, frame: AudioFrame) {
        let _ = self.writer.write_all(&frame.pcm);
    }

    fn write_silence_frame(&mut self, _capture_ts_ms: u64, _duration_ms: u32) {
        let silence = [0u8; AUDIO_PCM_BYTES_PER_FRAME];
        let _ = self.writer.write_all(&silence);
    }
}

pub fn write_pcm_frame<W: Write>(writer: &mut W, pcm: &[u8]) -> io::Result<()> {
    writer.write_all(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_audio_frame_bytes() {
        let mut sink = PcmAudioSink::new(Vec::<u8>::new());
        sink.write_audio_frame(AudioFrame {
            seq: 1,
            capture_ts_ms: 0,
            duration_ms: 20,
            pcm: vec![1, 2, 3, 4],
        });

        assert_eq!(sink.into_inner(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn writes_exact_silence_frame() {
        let mut sink = PcmAudioSink::new(Vec::<u8>::new());
        sink.write_silence_frame(0, 20);

        assert_eq!(sink.into_inner().len(), AUDIO_PCM_BYTES_PER_FRAME);
    }
}
