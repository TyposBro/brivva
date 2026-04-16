use std::io::Write;

use crate::{
    protocol::{AudioFrame, VideoChunk},
    scheduler::{audio::AUDIO_PCM_BYTES_PER_FRAME, AudioSink, VideoSink},
};

pub enum SinkCommand {
    AudioFrame(Vec<u8>),
    AudioSilence,
    VideoChunk(Vec<u8>),
}

pub struct ChannelAudioSink {
    tx: std::sync::mpsc::Sender<SinkCommand>,
}

impl ChannelAudioSink {
    pub fn new(tx: std::sync::mpsc::Sender<SinkCommand>) -> Self {
        Self { tx }
    }
}

impl AudioSink for ChannelAudioSink {
    fn write_audio_frame(&mut self, frame: AudioFrame) {
        let _ = self.tx.send(SinkCommand::AudioFrame(frame.pcm));
    }

    fn write_silence_frame(&mut self, _capture_ts_ms: u64, _duration_ms: u32) {
        let _ = self.tx.send(SinkCommand::AudioSilence);
    }
}

pub struct ChannelVideoSink {
    tx: std::sync::mpsc::Sender<SinkCommand>,
}

impl ChannelVideoSink {
    pub fn new(tx: std::sync::mpsc::Sender<SinkCommand>) -> Self {
        Self { tx }
    }
}

impl VideoSink for ChannelVideoSink {
    fn write_video_chunk(&mut self, chunk: VideoChunk) {
        let _ = self.tx.send(SinkCommand::VideoChunk(chunk.bytes));
    }
}

/// Runs on a dedicated std::thread. Receives sink commands via channel
/// and performs blocking writes to FFmpeg audio FIFO and video stdin.
/// Exits when all senders drop (channel closes).
pub fn run_sink_writer<A: Write, V: Write>(
    rx: std::sync::mpsc::Receiver<SinkCommand>,
    mut audio_writer: A,
    mut video_writer: V,
) {
    let silence = [0u8; AUDIO_PCM_BYTES_PER_FRAME];
    while let Ok(cmd) = rx.recv() {
        match cmd {
            SinkCommand::AudioFrame(pcm) => {
                let _ = audio_writer.write_all(&pcm);
            }
            SinkCommand::AudioSilence => {
                let _ = audio_writer.write_all(&silence);
            }
            SinkCommand::VideoChunk(bytes) => {
                let _ = video_writer.write_all(&bytes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_audio_sink_sends_frame() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut sink = ChannelAudioSink::new(tx);
        sink.write_audio_frame(AudioFrame {
            seq: 1,
            capture_ts_ms: 0,
            duration_ms: 20,
            pcm: vec![1, 2, 3],
        });
        drop(sink);
        let cmd = rx.recv().unwrap();
        assert!(matches!(cmd, SinkCommand::AudioFrame(pcm) if pcm == vec![1, 2, 3]));
    }

    #[test]
    fn channel_video_sink_sends_chunk() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut sink = ChannelVideoSink::new(tx);
        sink.write_video_chunk(VideoChunk {
            seq: 1,
            capture_ts_ms: 0,
            duration_ms: 33,
            is_keyframe: true,
            chunk_kind: crate::protocol::ChunkKind::Init,
            bytes: vec![7, 8, 9],
        });
        drop(sink);
        let cmd = rx.recv().unwrap();
        assert!(matches!(cmd, SinkCommand::VideoChunk(bytes) if bytes == vec![7, 8, 9]));
    }

    #[test]
    fn writer_thread_processes_commands() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut audio_buf = Vec::new();
        let mut video_buf = Vec::new();

        tx.send(SinkCommand::AudioFrame(vec![1, 2])).unwrap();
        tx.send(SinkCommand::VideoChunk(vec![3, 4])).unwrap();
        tx.send(SinkCommand::AudioSilence).unwrap();
        drop(tx);

        run_sink_writer(rx, &mut audio_buf, &mut video_buf);

        assert_eq!(audio_buf[..2], [1, 2]);
        assert_eq!(video_buf, vec![3, 4]);
        assert_eq!(audio_buf.len(), 2 + AUDIO_PCM_BYTES_PER_FRAME);
    }
}
