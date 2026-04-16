#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    SessionInit = 0x01,
    AudioFrame = 0x02,
    VideoChunk = 0x03,
    StreamEnd = 0x04,
    Ping = 0x05,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    Init,
    Media,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInit {
    pub version: u16,
    pub flags: u16,
    pub audio_sample_rate: u32,
    pub audio_channels: u16,
    pub audio_frame_duration_ms: u16,
    pub video_timescale: u16,
    pub session_start_unix_ms: u64,
    pub metadata_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFrame {
    pub seq: u64,
    pub capture_ts_ms: u64,
    pub duration_ms: u32,
    pub pcm: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoChunk {
    pub seq: u64,
    pub capture_ts_ms: u64,
    pub duration_ms: u32,
    pub is_keyframe: bool,
    pub chunk_kind: ChunkKind,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamEnd {
    pub reason_code: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ping {
    pub client_time_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    SessionInit(SessionInit),
    AudioFrame(AudioFrame),
    VideoChunk(VideoChunk),
    StreamEnd(StreamEnd),
    Ping(Ping),
}
