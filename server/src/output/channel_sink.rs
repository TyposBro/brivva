use std::io::Write;

use crate::{
    protocol::{AudioFrame, VideoChunk},
    scheduler::{audio::AUDIO_PCM_BYTES_PER_FRAME, AudioSink, VideoSink},
};

pub enum AudioSinkCommand {
    Frame(Vec<u8>),
    Silence,
}

pub struct ChannelAudioSink {
    tx: std::sync::mpsc::Sender<AudioSinkCommand>,
}

impl ChannelAudioSink {
    pub fn new(tx: std::sync::mpsc::Sender<AudioSinkCommand>) -> Self {
        Self { tx }
    }
}

impl AudioSink for ChannelAudioSink {
    fn write_audio_frame(&mut self, frame: AudioFrame) {
        let _ = self.tx.send(AudioSinkCommand::Frame(frame.pcm));
    }

    fn write_silence_frame(&mut self, _capture_ts_ms: u64, _duration_ms: u32) {
        let _ = self.tx.send(AudioSinkCommand::Silence);
    }
}

pub struct ChannelVideoSink {
    tx: std::sync::mpsc::Sender<Vec<u8>>,
}

impl ChannelVideoSink {
    pub fn new(tx: std::sync::mpsc::Sender<Vec<u8>>) -> Self {
        Self { tx }
    }
}

impl VideoSink for ChannelVideoSink {
    fn write_video_chunk(&mut self, chunk: VideoChunk) {
        let _ = self.tx.send(chunk.bytes);
    }
}

/// Runs on a dedicated std::thread. Performs blocking writes to FFmpeg
/// audio FIFO. Exits when all senders drop or on write error.
pub fn run_audio_writer<W: Write>(
    rx: std::sync::mpsc::Receiver<AudioSinkCommand>,
    mut writer: W,
) {
    let silence = [0u8; AUDIO_PCM_BYTES_PER_FRAME];
    let mut frames = 0u64;
    while let Ok(cmd) = rx.recv() {
        let result = match cmd {
            AudioSinkCommand::Frame(pcm) => writer.write_all(&pcm),
            AudioSinkCommand::Silence => writer.write_all(&silence),
        };
        if let Err(e) = result {
            eprintln!("[audio-writer] write error after {frames} frames: {e}");
            break;
        }
        frames += 1;
    }
    eprintln!("[audio-writer] exited after {frames} frames");
}

/// Runs on a dedicated std::thread. Performs blocking writes to FFmpeg
/// video stdin. Exits when all senders drop or on write error.
pub fn run_video_writer<W: Write>(
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    mut writer: W,
) {
    let mut chunks = 0u64;
    while let Ok(bytes) = rx.recv() {
        if let Err(e) = writer.write_all(&bytes) {
            eprintln!("[video-writer] write error after {chunks} chunks: {e}");
            break;
        }
        chunks += 1;
    }
    eprintln!("[video-writer] exited after {chunks} chunks");
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
        assert!(matches!(cmd, AudioSinkCommand::Frame(pcm) if pcm == vec![1, 2, 3]));
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
        assert_eq!(cmd, vec![7, 8, 9]);
    }

    #[test]
    fn audio_writer_processes_commands() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut buf = Vec::new();

        tx.send(AudioSinkCommand::Frame(vec![1, 2])).unwrap();
        tx.send(AudioSinkCommand::Silence).unwrap();
        drop(tx);

        run_audio_writer(rx, &mut buf);

        assert_eq!(buf[..2], [1, 2]);
        assert_eq!(buf.len(), 2 + AUDIO_PCM_BYTES_PER_FRAME);
    }

    #[test]
    fn video_writer_processes_commands() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut buf = Vec::new();

        tx.send(vec![3, 4]).unwrap();
        tx.send(vec![5, 6]).unwrap();
        drop(tx);

        run_video_writer(rx, &mut buf);

        assert_eq!(buf, vec![3, 4, 5, 6]);
    }
}
