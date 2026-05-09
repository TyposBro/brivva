use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoIngestKind {
    None,
    WebRtc,
    WebCodecsWs,
}

impl Default for VideoIngestKind {
    fn default() -> Self {
        Self::None
    }
}

impl VideoIngestKind {
    pub fn wire_label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::WebRtc => "webrtc",
            Self::WebCodecsWs => "webcodecs_ws",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodedVideoCodec {
    H264AnnexB,
    Vp8Ivf,
}

#[derive(Debug, Clone)]
pub struct EncodedVideoChunk {
    pub codec: EncodedVideoCodec,
    pub bytes: Vec<u8>,
    pub is_keyframe: bool,
    pub media_pts_us: u64,
    pub duration_us: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub sequence: u64,
}

#[derive(Debug, Clone)]
pub struct WebCodecsVideoIngestState {
    pub codec: EncodedVideoCodec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
    pub keyframe_interval_ms: u32,
    pub started_at: Instant,
    pub base_capture_time_us: Option<u64>,
    pub base_instant: Option<Instant>,
    pub next_sequence: Option<u64>,
    pub frames_received: u64,
    pub bytes_received: u64,
    pub drops: u64,
    pub first_keyframe_seen: bool,
    pub first_keyframe_logged: bool,
    pub last_media_pts_us: Option<u64>,
    pub ivf_frame_index: u64,
    pub wrote_ivf_header: bool,
}

impl WebCodecsVideoIngestState {
    pub fn new(
        codec: EncodedVideoCodec,
        width: u32,
        height: u32,
        fps: u32,
        bitrate_bps: u32,
        keyframe_interval_ms: u32,
    ) -> Self {
        Self {
            codec,
            width,
            height,
            fps,
            bitrate_bps,
            keyframe_interval_ms,
            started_at: Instant::now(),
            base_capture_time_us: None,
            base_instant: None,
            next_sequence: None,
            frames_received: 0,
            bytes_received: 0,
            drops: 0,
            first_keyframe_seen: false,
            first_keyframe_logged: false,
            last_media_pts_us: None,
            ivf_frame_index: 0,
            wrote_ivf_header: false,
        }
    }

    pub fn captured_at(&mut self, capture_time_us: u64) -> Instant {
        let base_capture = *self.base_capture_time_us.get_or_insert(capture_time_us);
        let base_instant = *self.base_instant.get_or_insert_with(Instant::now);
        let delta_us = capture_time_us.saturating_sub(base_capture);
        base_instant + std::time::Duration::from_micros(delta_us)
    }
}
