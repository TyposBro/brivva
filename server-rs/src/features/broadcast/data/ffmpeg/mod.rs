//! FFmpeg RTMP muxer: per-stream delayed WebRTC video mixed with translated TTS.
//!
//! Model: every RTMP output stream has an independent `delay_ms`. Host audio
//! and depacketized H.264 Annex-B video are fanned out to every stream's delay
//! buffer the instant they arrive. Each stream's drain threads emit media only
//! after it has aged by the stream's configured delay, mixing in any queued TTS
//! PCM at emit time. Video is copied by FFmpeg; the server no longer JPEG-
//! decodes or re-encodes frames, and no localhost UDP hop can drop RTP.
//!
//! Why this is simpler than the previous utterance-timestamp scheduler:
//! - No global playback clock; each output owns its delay buffer.
//! - No per-utterance truncation / fade-out / alignment.
//! - Source-language streams are just `is_source = true` → skip the TTS mix.
//! - Different target languages can have different delays (tuned to their
//!   expected STT+translate+TTS latency) — server enforces a translated
//!   minimum, D1 persists stream hints, Fargate reads via the session bundle.

mod args;
mod drain;
mod mixer;
mod orphan;
mod self_check;

use args::{FfmpegProgressAlert, FfmpegProgressMonitor, parse_tee_slave_muxer_index};
pub use args::{
    VideoInputCodec, VideoProfile, VideoProfileCaps, drain_stderr_lines, redact_rtmp_secrets,
};
pub use orphan::{decode_mp3_to_pcm, kill_orphan_ffmpeg};
pub use self_check::{log_startup_runtime_self_check, run_runtime_self_check};

use args::build_ffmpeg_args_with_profile;
use drain::{AudioDrainCtx, VideoDrainCtx, audio_drain_loop, video_drain_loop};

use crate::features::broadcast::domain::SessionMetrics;
use crate::features::broadcast::domain::VideoEncoderKind;
use crate::features::broadcast::domain::output_health::{
    OutputDegradationLabel, OutputHealthSnapshot, OutputHealthState, OutputId,
};
use crate::features::broadcast::domain::render_graph::{
    RenderGraphNodeState, RenderGraphOutputNode,
};

use std::collections::{HashMap, VecDeque};
use std::io::BufReader;
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

// ── Timing + format constants ─────────────────────────────

/// Cap the host audio delay buffer per stream at ~20 s of samples.
/// (Prevents unbounded growth if the drain thread falls behind.)
const HOST_AUDIO_CAP_BYTES: usize = 20 * 88_200;
/// Cap depacketized H.264 chunks per stream. Dropping old chunks can still
/// create visible gaps, but this path no longer uses localhost UDP, so overflow
/// should only happen if FFmpeg/RTMP is truly slower than real time.
const HOST_VIDEO_H264_CAP_CHUNKS: usize = 120_000;
const PCM_BYTES_PER_SECOND: usize = 88_200;
/// Live-commerce translated audio should not silently drift by tens of
/// seconds. Keep a firm default cap and shed whole TTS segments if forced;
/// never chop raw PCM from the middle of a sentence.
const DEFAULT_TTS_QUEUE_CAP_MS: u64 = 60_000;
const MAX_TTS_QUEUE_CAP_MS: u64 = 120_000;
/// Max FFmpeg restart attempts per stream.
const MAX_FFMPEG_RESTARTS: u32 = 3;
/// Delay between FFmpeg restart attempts.
const FFMPEG_RESTART_DELAY: Duration = Duration::from_secs(2);
/// Force an FFmpeg restart if no bytes have been written to the child for
/// this long. Grip regional endpoints drop silently after ~30 min; the
/// stream appears fine (FFmpeg hasn't exited) but audio/video stops flowing.
/// Pre-emptively killing the child surfaces it to `detect_crashed`, which
/// then restarts the stream with the existing delay buffers preserved.
const IDLE_RESTART_THRESHOLD: Duration = Duration::from_secs(25);

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn align_pcm_s16le_len(len: usize) -> usize {
    len - (len % 2)
}

fn truncate_pcm_s16le(mut pcm: Vec<u8>) -> Vec<u8> {
    pcm.truncate(align_pcm_s16le_len(pcm.len()));
    pcm
}

// ── Per-stream shared state ───────────────────────────────

/// Timestamped chunk of host media. The tuple is (received_at, bytes). A chunk
/// becomes eligible for emission at `received_at + stream.delay`.
type TimedChunk = (Instant, Arc<[u8]>);

#[derive(Clone, Debug)]
pub(crate) struct TtsSegment {
    pub utterance_id: u64,
    pub sentence_id: u32,
    pub lang: String,
    pub text: String,
    pub pcm: Arc<[u8]>,
    pub estimated_source_duration_ms: Option<u64>,
    pub tts_duration_ms: u64,
    pub expansion_ratio_milli: Option<u32>,
    pub policy: String,
}

impl TtsSegment {
    #[cfg(test)]
    pub(crate) fn new(
        utterance_id: u64,
        sentence_id: u32,
        lang: String,
        text: String,
        pcm: Vec<u8>,
    ) -> Self {
        Self::with_metadata(utterance_id, sentence_id, lang, text, pcm, None, "normal")
    }

    pub(crate) fn with_metadata(
        utterance_id: u64,
        sentence_id: u32,
        lang: String,
        text: String,
        pcm: Vec<u8>,
        estimated_source_duration_ms: Option<u64>,
        policy: impl Into<String>,
    ) -> Self {
        let pcm = truncate_pcm_s16le(pcm);
        let tts_duration_ms =
            (pcm.len() as u64).saturating_mul(1_000) / PCM_BYTES_PER_SECOND as u64;
        let expansion_ratio_milli = estimated_source_duration_ms
            .filter(|source_ms| *source_ms > 0)
            .map(|source_ms| {
                tts_duration_ms
                    .saturating_mul(1_000)
                    .saturating_div(source_ms)
                    .min(u32::MAX as u64) as u32
            });
        Self {
            utterance_id,
            sentence_id,
            lang,
            text,
            pcm: Arc::from(pcm),
            estimated_source_duration_ms,
            tts_duration_ms,
            expansion_ratio_milli,
            policy: policy.into(),
        }
    }

    pub(crate) fn byte_len(&self) -> usize {
        self.pcm.len()
    }

    pub(crate) fn duration_ms(&self) -> u64 {
        self.tts_duration_ms
    }
}

pub(crate) fn tts_queue_bytes(queue: &VecDeque<TtsSegment>) -> usize {
    queue.iter().map(TtsSegment::byte_len).sum()
}

fn tts_queue_cap_bytes_from_env() -> usize {
    let millis = std::env::var("BRIVVA_TTS_QUEUE_CAP_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TTS_QUEUE_CAP_MS)
        .clamp(1_000, MAX_TTS_QUEUE_CAP_MS);
    millis.saturating_mul(PCM_BYTES_PER_SECOND as u64) as usize / 1_000
}

pub(crate) struct StreamBuffers {
    audio: Arc<StdMutex<VecDeque<TimedChunk>>>,
    video: Arc<StdMutex<VecDeque<TimedChunk>>>,
    tts: Arc<StdMutex<VecDeque<TtsSegment>>>,
}

impl StreamBuffers {
    fn new() -> Self {
        Self {
            audio: Arc::new(StdMutex::new(VecDeque::new())),
            video: Arc::new(StdMutex::new(VecDeque::new())),
            tts: Arc::new(StdMutex::new(VecDeque::new())),
        }
    }
}

struct RtmpStream {
    child: std::process::Child,
    video_handle: Option<thread::JoinHandle<()>>,
    audio_handle: Option<thread::JoinHandle<()>>,
    audio_fifo: String,
    subtitle_textfile: String,

    lang: String,
    rtmp_urls: Vec<String>,
    destinations: Vec<RtmpDestination>,
    delay: Duration,
    is_source: bool,
    host_gain: f32,
    /// User explicitly selected "Passthrough (source)" for this destination.
    /// The pipeline skips STT+translate+TTS and re-broadcasts host audio raw.
    /// Implementation-wise this is a superset of `is_source=true` — we keep
    /// it as a dedicated flag so traces + future bifurcations (e.g. per-
    /// stream billing exemption) can tell "user picked passthrough" apart
    /// from "stream's lang coincidentally equals source_lang".
    passthrough: bool,
    output_id: Option<OutputId>,
    destination_platform: String,
    output_controls_enabled: bool,
    render_graph_node: Option<RenderGraphOutputNode>,
    buffers: StreamBuffers,
    stop_flag: Arc<AtomicBool>,
    restart_count: u32,
    /// Unix-ms wall clock of the most recent successful FFmpeg-stdin write
    /// from either drain thread. The health monitor uses it to detect silent
    /// output stalls that don't crash FFmpeg (see `IDLE_RESTART_THRESHOLD`).
    ///
    /// **Sentinel `0`** means "no drain write has happened yet" — the stream
    /// is still warming up (host hasn't pushed a frame/sample, or ffmpeg's
    /// output init is still mid-connection). `kill_idle_streams` treats
    /// `0` as "not idle, skip" so a slow-starting host doesn't burn the
    /// first `MAX_FFMPEG_RESTARTS` attempt on a false-positive idle kill.
    last_write_ms: Arc<AtomicI64>,
    video_input_codec: VideoInputCodec,
}

#[derive(Clone, Debug)]
pub struct RtmpDestination {
    pub platform: String,
    pub url: String,
}

impl RtmpDestination {
    pub fn new(platform: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            url: url.into(),
        }
    }
}

pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
    video_profile: VideoProfile,
    video_profile_caps: VideoProfileCaps,
    /// Optional billing counters. `None` in unit tests / paths that don't
    /// care about metrics; `Some` when the session wires one via
    /// `set_metrics`. Cloned into each drain thread so increments stay
    /// lock-free.
    metrics: Option<Arc<SessionMetrics>>,
    output_health_tx: Option<mpsc::UnboundedSender<OutputHealthSnapshot>>,
    video_encoder: VideoEncoderKind,
    video_input_codec: VideoInputCodec,
}

type CrashedStreamSnapshot = (
    String,
    String,
    Vec<String>,
    u64,
    bool,
    f32,
    bool,
    Option<OutputId>,
    String,
    bool,
    Option<RenderGraphOutputNode>,
    u32,
    Vec<RtmpDestination>,
    StreamBuffers,
);

struct RestartStreamArgs {
    id: String,
    lang: String,
    rtmp_urls: Vec<String>,
    delay_ms: u64,
    is_source: bool,
    host_gain: f32,
    passthrough: bool,
    output_id: Option<OutputId>,
    destination_platform: String,
    output_controls_enabled: bool,
    render_graph_node: Option<RenderGraphOutputNode>,
    prev_count: u32,
    destinations: Vec<RtmpDestination>,
    buffers: StreamBuffers,
}

struct StreamSpawnArgs {
    stream_id: String,
    lang: String,
    rtmp_urls: Vec<String>,
    delay_ms: u64,
    is_source: bool,
    host_gain: f32,
    passthrough: bool,
    output_id: Option<OutputId>,
    destination_platform: String,
    destinations: Vec<RtmpDestination>,
    output_controls_enabled: bool,
    render_graph_node: Option<RenderGraphOutputNode>,
    existing_buffers: Option<StreamBuffers>,
    video_input_codec: VideoInputCodec,
}

/// Public signature for `RtmpManager::start_stream`. Grouping the per-stream
/// inputs into a struct keeps the call site readable as the number of flags
/// grows (passthrough joined `is_source` + `host_gain`) and stays inside the
/// §3.3 arg budget.
pub struct StartStreamArgs<'a> {
    pub stream_id: &'a str,
    pub lang: &'a str,
    pub rtmp_url: &'a str,
    pub delay_ms: u64,
    pub is_source: bool,
    pub host_gain: f32,
    pub output_id: Option<OutputId>,
    pub destination_platform: &'a str,
    pub output_controls_enabled: bool,
    /// Optional V2 Phase 4 adapter metadata. When present, logs node lifecycle
    /// around the existing per-output FFmpeg process only.
    pub render_graph_node: Option<RenderGraphOutputNode>,
    /// See `RtmpStream::passthrough`. Fargate skips STT/translate/TTS entirely
    /// for passthrough streams — host audio re-broadcast at gain 1.0, no
    /// caption overlay.
    pub passthrough: bool,
}

pub struct StartStreamGroupArgs<'a> {
    pub stream_id: &'a str,
    pub lang: &'a str,
    pub rtmp_urls: Vec<String>,
    pub destinations: Vec<RtmpDestination>,
    pub delay_ms: u64,
    pub is_source: bool,
    pub host_gain: f32,
    pub output_id: Option<OutputId>,
    pub destination_platform: &'a str,
    pub output_controls_enabled: bool,
    pub render_graph_node: Option<RenderGraphOutputNode>,
    pub passthrough: bool,
}

fn emit_output_health(
    tx: Option<&mpsc::UnboundedSender<OutputHealthSnapshot>>,
    enabled: bool,
    output_id: &Option<OutputId>,
    stream_id: &str,
    lang: &str,
    destination_platform: &str,
    state: OutputHealthState,
    restart_count: u32,
    degradation: Option<OutputDegradationLabel>,
    message: Option<String>,
) {
    if !enabled {
        return;
    }
    let Some(output_id) = output_id else {
        return;
    };
    let mut snapshot = OutputHealthSnapshot::new(
        output_id.clone(),
        stream_id,
        lang,
        destination_platform,
        state,
    )
    .with_restart_count(restart_count);
    snapshot.degradation = degradation;
    snapshot.message = message;
    tracing::info!(
        event = snapshot.event_name(),
        output_id = %snapshot.output_id,
        stream_id = %snapshot.stream_id,
        lang = %snapshot.lang,
        destination_platform = %snapshot.destination_platform,
        state = ?snapshot.state,
        restart_count = snapshot.restart_count,
        degradation = ?snapshot.degradation,
        message = ?snapshot.message,
        snapshot = ?snapshot,
        "v2 output health event"
    );
    if let Some(tx) = tx {
        let _ = tx.send(snapshot);
    }
}

fn emit_render_graph_adapter_event(
    node: &Option<RenderGraphOutputNode>,
    state: RenderGraphNodeState,
    event: &'static str,
    message: Option<String>,
) {
    let Some(node) = node else {
        return;
    };
    tracing::info!(
        event,
        graph_id = %node.graph_id.as_str(),
        node_id = %node.node_id.as_str(),
        output_id = %node.output_id,
        stream_id = %node.stream_id,
        node_kind = ?node.kind,
        node_state = ?state,
        message = ?message,
        "v2 render graph adapter event"
    );
}

impl Default for RtmpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RtmpManager {
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            video_profile: VideoProfile::default(),
            video_profile_caps: VideoProfileCaps::default(),
            metrics: None,
            output_health_tx: None,
            video_encoder: VideoEncoderKind::X264,
            video_input_codec: VideoInputCodec::H264AnnexB,
        }
    }

    pub fn set_video_encoder(&mut self, encoder: VideoEncoderKind) {
        self.video_encoder = encoder;
    }

    pub fn set_video_profile_caps(&mut self, caps: VideoProfileCaps) {
        self.video_profile_caps = caps;
    }

    pub fn set_capture_video_profile(&mut self, width: u32, height: u32, fps: u32) -> VideoProfile {
        let caps = self.effective_video_profile_caps();
        let profile = VideoProfile::from_capture_with_caps(width, height, fps, caps);
        self.set_video_profile(profile);
        profile
    }

    pub fn set_observed_video_fps(&mut self, fps: u32) -> VideoProfile {
        let current = self.video_profile;
        let caps = self.effective_video_profile_caps();
        let profile =
            VideoProfile::from_capture_with_caps(current.max_width, current.max_height, fps, caps);
        self.set_video_profile(profile);
        profile
    }

    pub fn set_observed_video_dimensions(&mut self, width: u32, height: u32) -> VideoProfile {
        let current = self.video_profile;
        let caps = self.effective_video_profile_caps();
        let profile = VideoProfile::from_capture_with_caps(width, height, current.input_fps, caps);
        self.set_video_profile(profile);
        profile
    }

    fn effective_video_profile_caps(&self) -> VideoProfileCaps {
        if self.video_encoder == VideoEncoderKind::X264 && self.video_profile_caps.max_fps > 30 {
            tracing::warn!(
                requested_max_fps = self.video_profile_caps.max_fps,
                effective_max_fps = 30,
                video_encoder = self.video_encoder.codec_name(),
                "x264 live encode capped to 30fps; use BRIVVA_VIDEO_ENCODER=nvenc for high-fps output"
            );
            return VideoProfileCaps {
                max_fps: 30,
                ..self.video_profile_caps
            };
        }
        self.video_profile_caps
    }

    pub fn set_video_profile(&mut self, profile: VideoProfile) {
        if self.video_profile == profile {
            return;
        }
        tracing::info!(
            input_fps = profile.input_fps,
            output_fps = profile.output_fps,
            max_width = profile.max_width,
            max_height = profile.max_height,
            bitrate_kbps = profile.bitrate_kbps,
            "ffmpeg video profile updated for host camera"
        );
        self.video_profile = profile;
        let active_stream_count = self
            .streams
            .values()
            .filter(|stream| !stream.stop_flag.load(Ordering::Acquire))
            .count();
        if active_stream_count > 0 {
            // Do not kill live FFmpeg children for observed WebRTC profile
            // corrections. The browser can report/estimate FPS after streams
            // have already started; killing here looks like an FFmpeg crash,
            // burns restart budget, and restarts mid-GOP without SPS/PPS.
            // Existing children keep their launch profile; future restarts use
            // the corrected profile.
            tracing::info!(
                active_stream_count,
                "ffmpeg video profile update deferred for active streams"
            );
        }
    }

    /// Attach a billing-metrics sink. Must be called before `start_stream`
    /// so per-stream drain threads receive the counter refs at spawn time.
    pub fn set_metrics(&mut self, metrics: Arc<SessionMetrics>) {
        self.metrics = Some(metrics);
    }

    pub fn set_output_health_tx(&mut self, tx: mpsc::UnboundedSender<OutputHealthSnapshot>) {
        self.output_health_tx = Some(tx);
    }

    /// Start an FFmpeg RTMP process with dedicated video + audio drain threads.
    ///
    /// `host_gain` is the multiplier applied to the delayed host audio before
    /// mixing with translated TTS (1.0 for source streams, typically 0.2 for
    /// ducked target streams). Video arrives separately as encoded WebRTC RTP
    /// and is copied to RTMP by FFmpeg.
    pub fn start_stream(&mut self, args: StartStreamArgs<'_>) -> Result<(), String> {
        self.start_stream_group(StartStreamGroupArgs {
            stream_id: args.stream_id,
            lang: args.lang,
            rtmp_urls: vec![args.rtmp_url.to_string()],
            destinations: vec![RtmpDestination::new(
                args.destination_platform,
                args.rtmp_url,
            )],
            delay_ms: args.delay_ms,
            is_source: args.is_source,
            host_gain: args.host_gain,
            output_id: args.output_id,
            destination_platform: args.destination_platform,
            output_controls_enabled: args.output_controls_enabled,
            render_graph_node: args.render_graph_node,
            passthrough: args.passthrough,
        })
    }

    pub fn start_stream_group(&mut self, args: StartStreamGroupArgs<'_>) -> Result<(), String> {
        emit_output_health(
            self.output_health_tx.as_ref(),
            args.output_controls_enabled,
            &args.output_id,
            args.stream_id,
            args.lang,
            args.destination_platform,
            OutputHealthState::Starting,
            0,
            None,
            None,
        );
        emit_render_graph_adapter_event(
            &args.render_graph_node,
            RenderGraphNodeState::Starting,
            "render_graph.node.start",
            None,
        );
        if let Err(e) = self.spawn_stream_inner(StreamSpawnArgs {
            stream_id: args.stream_id.to_string(),
            lang: args.lang.to_string(),
            rtmp_urls: args.rtmp_urls.clone(),
            destinations: args.destinations.clone(),
            delay_ms: args.delay_ms,
            is_source: args.is_source,
            host_gain: args.host_gain,
            passthrough: args.passthrough,
            output_id: args.output_id.clone(),
            destination_platform: args.destination_platform.to_string(),
            output_controls_enabled: args.output_controls_enabled,
            render_graph_node: args.render_graph_node.clone(),
            existing_buffers: None,
            video_input_codec: self.video_input_codec,
        }) {
            emit_output_health(
                self.output_health_tx.as_ref(),
                args.output_controls_enabled,
                &args.output_id,
                args.stream_id,
                args.lang,
                args.destination_platform,
                OutputHealthState::Failed,
                0,
                Some(OutputDegradationLabel::RtmpPublishError),
                Some(e.clone()),
            );
            emit_render_graph_adapter_event(
                &args.render_graph_node,
                RenderGraphNodeState::Failed,
                "render_graph.node.failure",
                Some(e.clone()),
            );
            return Err(e);
        }
        emit_output_health(
            self.output_health_tx.as_ref(),
            args.output_controls_enabled,
            &args.output_id,
            args.stream_id,
            args.lang,
            args.destination_platform,
            OutputHealthState::Publishing,
            0,
            None,
            None,
        );
        if let Some(stream) = self.streams.get_mut(args.stream_id)
            && let Some(node) = stream.render_graph_node.as_mut()
        {
            node.transition(RenderGraphNodeState::Live);
        }
        emit_render_graph_adapter_event(
            &args.render_graph_node,
            RenderGraphNodeState::Live,
            "render_graph.node.live",
            None,
        );
        tracing::info!(
            stream_id = %args.stream_id,
            lang = %args.lang,
            rtmp_urls = ?args.rtmp_urls.iter().map(|url| redact_rtmp_secrets(url)).collect::<Vec<_>>(),
            destinations = ?args.destinations.iter().map(|dest| serde_json::json!({
                "platform": dest.platform,
                "url": redact_rtmp_secrets(&dest.url),
            })).collect::<Vec<_>>(),
            rtmp_destination_count = args.rtmp_urls.len(),
            delay_ms = args.delay_ms,
            is_source = args.is_source,
            host_gain = args.host_gain,
            passthrough = args.passthrough,
            "ffmpeg rtmp stream group started"
        );
        Ok(())
    }

    /// Push one depacketized Annex-B H.264 chunk into every stream's delay
    /// buffer. FFmpeg receives these bytes through stdin, avoiding the old
    /// localhost UDP RTP bridge that dropped 4K bursts and corrupted frames.
    pub fn push_video_h264(&self, chunk: &[u8]) {
        self.push_video_h264_at(chunk, Instant::now());
    }

    /// Push H.264 using the source media clock, not wall-clock arrival. WebRTC
    /// packets can arrive in bursts; using arrival time makes RTMP speed up and
    /// then buffer. RTP timestamps preserve the browser capture cadence.
    pub fn push_video_h264_at(&self, chunk: &[u8], captured_at: Instant) {
        if chunk.is_empty() {
            return;
        }
        let shared_chunk: Arc<[u8]> = Arc::from(chunk);
        for stream in self.streams.values() {
            if stream.video_input_codec != VideoInputCodec::H264AnnexB {
                continue;
            }
            let mut buf = stream.buffers.video.lock().unwrap();
            buf.push_back((captured_at, shared_chunk.clone()));
            let mut dropped = 0usize;
            while buf.len() > HOST_VIDEO_H264_CAP_CHUNKS {
                buf.pop_front();
                dropped += 1;
            }
            if dropped > 0 {
                tracing::warn!(
                    destination_platform = %stream.destination_platform,
                    lang = %stream.lang,
                    video_buffer_cap_chunks_dropped = dropped,
                    cap_chunks = HOST_VIDEO_H264_CAP_CHUNKS,
                    "video h264 queue cap dropped oldest chunks"
                );
            }
        }
    }

    pub fn push_video_vp8_ivf_frame_at(&self, frame: &[u8], captured_at: Instant) {
        if frame.is_empty() {
            return;
        }
        let shared_chunk: Arc<[u8]> = Arc::from(frame);
        for stream in self.streams.values() {
            if stream.video_input_codec != VideoInputCodec::Vp8Ivf {
                continue;
            }
            let mut buf = stream.buffers.video.lock().unwrap();
            buf.push_back((captured_at, shared_chunk.clone()));
            let mut dropped = 0usize;
            while buf.len() > HOST_VIDEO_H264_CAP_CHUNKS {
                buf.pop_front();
                dropped += 1;
            }
            if dropped > 0 {
                tracing::warn!(
                    destination_platform = %stream.destination_platform,
                    lang = %stream.lang,
                    video_buffer_cap_chunks_dropped = dropped,
                    cap_chunks = HOST_VIDEO_H264_CAP_CHUNKS,
                    "video vp8 ivf queue cap dropped oldest chunks"
                );
            }
        }
    }

    fn stop_stream_for_restart(&mut self, id: &str) -> Option<RestartStreamArgs> {
        let mut old = self.streams.remove(id)?;
        old.stop_flag.store(true, Ordering::Release);
        let _ = old.child.kill();
        let _ = old.child.wait();
        let _ = std::fs::remove_file(&old.audio_fifo);
        let _ = std::fs::remove_file(&old.subtitle_textfile);
        Some(RestartStreamArgs {
            id: id.to_string(),
            lang: old.lang,
            rtmp_urls: old.rtmp_urls,
            delay_ms: old.delay.as_millis() as u64,
            is_source: old.is_source,
            host_gain: old.host_gain,
            passthrough: old.passthrough,
            output_id: old.output_id,
            destination_platform: old.destination_platform,
            output_controls_enabled: old.output_controls_enabled,
            render_graph_node: old.render_graph_node,
            prev_count: old.restart_count,
            destinations: old.destinations,
            buffers: old.buffers,
        })
    }

    pub fn switch_video_input_codec(&mut self, codec: VideoInputCodec) {
        if self.video_input_codec == codec {
            return;
        }
        self.video_input_codec = codec;
        let to_restart: Vec<String> = self
            .streams
            .iter()
            .filter_map(|(id, stream)| (stream.video_input_codec != codec).then_some(id.clone()))
            .collect();
        for id in to_restart {
            let Some(old) = self.stop_stream_for_restart(&id) else {
                continue;
            };
            tracing::info!(stream_id = %id, video_input_codec = codec.log_label(), "switching ffmpeg video input codec");
            self.restart_stream_with_codec(old, codec);
        }
    }

    /// Push raw host PCM (s16le 44.1 kHz mono) into every stream's delay buffer.
    pub fn push_host_audio(&self, pcm: &[u8]) {
        self.push_host_audio_at(pcm, Instant::now());
    }

    /// Push raw host PCM using the source media clock, not wall-clock arrival.
    /// Timestamped WebSocket audio can arrive with jitter; using the browser
    /// sample clock keeps RTMP delay buffers paced by capture cadence.
    pub fn push_host_audio_at(&self, pcm: &[u8], captured_at: Instant) {
        let even_len = align_pcm_s16le_len(pcm.len());
        if even_len == 0 {
            return;
        }
        let shared_pcm: Arc<[u8]> = Arc::from(&pcm[..even_len]);
        for stream in self.streams.values() {
            let mut buf = stream.buffers.audio.lock().unwrap();
            buf.push_back((captured_at, shared_pcm.clone()));
            // Cap total bytes across queued chunks.
            let mut total: usize = buf.iter().map(|(_, b)| b.len()).sum();
            while total > HOST_AUDIO_CAP_BYTES {
                if let Some((_, front)) = buf.pop_front() {
                    total = total.saturating_sub(front.len());
                } else {
                    break;
                }
            }
        }
    }

    /// Append translated PCM for a specific language to that stream's TTS queue.
    /// Compatibility wrapper for tests and older call sites that do not carry
    /// utterance metadata.
    #[cfg(test)]
    pub fn push_tts(&self, lang: &str, pcm: Vec<u8>) {
        self.push_tts_segment(TtsSegment::new(0, 0, lang.to_string(), String::new(), pcm));
    }

    /// Append a translated TTS segment to matching target streams.
    ///
    /// Both `is_source` and `passthrough` streams are skipped: neither carries
    /// a translated track, so enqueueing PCM would leak memory up to the cap
    /// and never play back.
    pub(crate) fn push_tts_segment(&self, segment: TtsSegment) {
        if segment.byte_len() == 0 {
            return;
        }
        if let Some(m) = &self.metrics {
            m.record_tts_pcm(&segment.lang, segment.byte_len() as u64);
        }
        for stream in self.streams.values() {
            if stream.lang == segment.lang && !stream.is_source && !stream.passthrough {
                let mut q = stream.buffers.tts.lock().unwrap();
                q.push_back(segment.clone());
                let cap_bytes = tts_queue_cap_bytes_from_env();
                while tts_queue_bytes(&q) > cap_bytes {
                    let Some(dropped) = q.pop_front() else {
                        break;
                    };
                    tracing::warn!(
                        lang = %dropped.lang,
                        utterance_id = dropped.utterance_id,
                        sentence_id = dropped.sentence_id,
                        dropped_bytes = dropped.byte_len(),
                        dropped_duration_ms = dropped.duration_ms(),
                        estimated_source_duration_ms = dropped.estimated_source_duration_ms,
                        expansion_ratio_milli = dropped.expansion_ratio_milli,
                        policy = %dropped.policy,
                        text_chars = dropped.text.chars().count(),
                        cap_bytes,
                        "tts segment queue overflow — whole segment dropped"
                    );
                }
            }
        }
    }

    pub(crate) fn tts_backlog_ms(&self, lang: &str) -> u64 {
        self.streams
            .values()
            .filter(|stream| stream.lang == lang && !stream.is_source && !stream.passthrough)
            .map(|stream| {
                let q = stream.buffers.tts.lock().unwrap();
                (tts_queue_bytes(&q).saturating_mul(1_000) / PCM_BYTES_PER_SECOND) as u64
            })
            .max()
            .unwrap_or(0)
    }

    /// Update burned subtitle text for every target stream in `lang`.
    /// FFmpeg's drawtext filter reads this file with `reload=1`, so the
    /// output process stays alive while subtitles change per utterance.
    pub fn push_subtitle(&self, lang: &str, text: &str) {
        for stream in self.streams.values() {
            if stream.lang == lang && !stream.is_source && !stream.passthrough {
                if let Err(error) = std::fs::write(&stream.subtitle_textfile, text) {
                    tracing::warn!(
                        lang = %lang,
                        path = %stream.subtitle_textfile,
                        error = %error,
                        "subtitle textfile update failed"
                    );
                }
            }
        }
    }

    /// Kill FFmpeg children that have gone silent (no drain writes in
    /// `IDLE_RESTART_THRESHOLD`). We only send the kill here; `detect_crashed`
    /// on the next monitor tick sees the exit and runs the normal restart
    /// path with buffers reused. Deliberately does not set `stop_flag` so
    /// the stream is treated as a crash, not an intentional shutdown.
    pub(crate) fn kill_idle_streams(&mut self) {
        let now = now_unix_ms();
        let threshold_ms = IDLE_RESTART_THRESHOLD.as_millis() as i64;
        for (id, stream) in &mut self.streams {
            if stream.stop_flag.load(Ordering::Acquire) {
                continue;
            }
            let last = stream.last_write_ms.load(Ordering::Acquire);
            // Sentinel: drain has not written yet. Stream is warming up
            // (host hasn't pushed first frame, or ffmpeg's still opening
            // the output). Counting this as "idle" would kill every
            // session that takes >25s to produce its first sample — the
            // common case on browser hosts with mic/cam permission
            // prompts. See `RtmpStream::last_write_ms`.
            if last == 0 {
                continue;
            }
            let idle_ms = now.saturating_sub(last);
            if idle_ms < threshold_ms {
                continue;
            }
            tracing::warn!(
                stream_id = %id,
                lang = %stream.lang,
                idle_ms,
                threshold_ms,
                "ffmpeg idle beyond threshold, killing to trigger restart"
            );
            emit_output_health(
                self.output_health_tx.as_ref(),
                stream.output_controls_enabled,
                &stream.output_id,
                id,
                &stream.lang,
                &stream.destination_platform,
                OutputHealthState::Degraded,
                stream.restart_count,
                Some(OutputDegradationLabel::IdleNoWrites),
                Some(format!("idle_ms={idle_ms} threshold_ms={threshold_ms}")),
            );
            match stream.child.kill() {
                Ok(()) => {
                    // Reset the timestamp so we don't try to kill a second
                    // time before `detect_crashed` picks up the exit.
                    stream.last_write_ms.store(now, Ordering::Release);
                }
                Err(e) => tracing::error!(
                    stream_id = %id,
                    error = %e,
                    "ffmpeg idle-kill failed"
                ),
            }
        }
    }

    /// Scan for crashed FFmpeg processes and surface the restart context.
    pub(crate) fn detect_crashed(&mut self) -> Vec<CrashedStreamSnapshot> {
        let mut to_restart = Vec::new();

        for (id, stream) in &mut self.streams {
            match stream.child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(-1);
                    let signal = status.signal();
                    let core_dumped = status.core_dumped();
                    if stream.stop_flag.load(Ordering::Acquire) {
                        continue;
                    }
                    tracing::warn!(
                        stream_id = %id,
                        lang = %stream.lang,
                        exit_code = code,
                        exit_signal = signal,
                        core_dumped,
                        restart_count = stream.restart_count,
                        "ffmpeg rtmp process crashed, scheduling restart"
                    );
                    emit_output_health(
                        self.output_health_tx.as_ref(),
                        stream.output_controls_enabled,
                        &stream.output_id,
                        id,
                        &stream.lang,
                        &stream.destination_platform,
                        OutputHealthState::Degraded,
                        stream.restart_count,
                        Some(OutputDegradationLabel::FfmpegCrash),
                        Some(match signal {
                            Some(signal) => format!("ffmpeg signal={signal} exit_code={code}"),
                            None => format!("ffmpeg exit_code={code}"),
                        }),
                    );
                    if stream.restart_count >= MAX_FFMPEG_RESTARTS {
                        tracing::error!(
                            stream_id = %id,
                            lang = %stream.lang,
                            attempts = MAX_FFMPEG_RESTARTS,
                            "ffmpeg rtmp giving up after max restart attempts"
                        );
                        emit_output_health(
                            self.output_health_tx.as_ref(),
                            stream.output_controls_enabled,
                            &stream.output_id,
                            id,
                            &stream.lang,
                            &stream.destination_platform,
                            OutputHealthState::Failed,
                            stream.restart_count,
                            Some(OutputDegradationLabel::RestartLimitReached),
                            Some("max ffmpeg restart attempts reached".to_string()),
                        );
                        stream.stop_flag.store(true, Ordering::Release);
                        continue;
                    }
                    to_restart.push(id.clone());
                }
                Ok(None) => {}
                Err(e) => tracing::error!(
                    stream_id = %id,
                    error = %e,
                    "ffmpeg status check failed"
                ),
            }
        }

        let mut result = Vec::new();
        for id in to_restart {
            if let Some(mut old) = self.streams.remove(&id) {
                old.stop_flag.store(true, Ordering::Release);
                let _ = old.child.kill();
                let _ = old.child.wait();
                let _ = std::fs::remove_file(&old.audio_fifo);
                let _ = std::fs::remove_file(&old.subtitle_textfile);

                result.push((
                    id,
                    old.lang,
                    old.rtmp_urls,
                    old.delay.as_millis() as u64,
                    old.is_source,
                    old.host_gain,
                    old.passthrough,
                    old.output_id,
                    old.destination_platform,
                    old.output_controls_enabled,
                    old.render_graph_node,
                    old.restart_count,
                    old.destinations,
                    old.buffers,
                ));
            }
        }
        result
    }

    /// Restart a stream after a crash, reusing its buffers so queued host
    /// media + TTS survive the FFmpeg restart.
    fn restart_stream(&mut self, args: RestartStreamArgs) {
        self.restart_stream_with_codec(args, self.video_input_codec);
    }

    fn restart_stream_with_codec(
        &mut self,
        args: RestartStreamArgs,
        video_input_codec: VideoInputCodec,
    ) {
        emit_output_health(
            self.output_health_tx.as_ref(),
            args.output_controls_enabled,
            &args.output_id,
            &args.id,
            &args.lang,
            &args.destination_platform,
            OutputHealthState::Restarting,
            args.prev_count,
            Some(OutputDegradationLabel::FfmpegCrash),
            None,
        );
        emit_render_graph_adapter_event(
            &args.render_graph_node,
            RenderGraphNodeState::Restarting,
            "render_graph.node.start",
            Some("ffmpeg restart".to_string()),
        );
        match self.spawn_stream_inner(StreamSpawnArgs {
            stream_id: args.id.clone(),
            lang: args.lang.clone(),
            rtmp_urls: args.rtmp_urls.clone(),
            destinations: args.destinations.clone(),
            delay_ms: args.delay_ms,
            is_source: args.is_source,
            host_gain: args.host_gain,
            passthrough: args.passthrough,
            output_id: args.output_id.clone(),
            destination_platform: args.destination_platform.clone(),
            output_controls_enabled: args.output_controls_enabled,
            render_graph_node: args.render_graph_node.clone(),
            existing_buffers: Some(args.buffers),
            video_input_codec,
        }) {
            Ok(()) => {
                if let Some(stream) = self.streams.get_mut(&args.id) {
                    stream.restart_count = args.prev_count + 1;
                    if let Some(node) = stream.render_graph_node.as_mut() {
                        node.transition(RenderGraphNodeState::Live);
                    }
                }
                emit_output_health(
                    self.output_health_tx.as_ref(),
                    args.output_controls_enabled,
                    &args.output_id,
                    &args.id,
                    &args.lang,
                    &args.destination_platform,
                    OutputHealthState::Publishing,
                    args.prev_count + 1,
                    None,
                    None,
                );
                emit_render_graph_adapter_event(
                    &args.render_graph_node,
                    RenderGraphNodeState::Live,
                    "render_graph.node.live",
                    Some("ffmpeg restarted".to_string()),
                );
                tracing::info!(
                    stream_id = %args.id,
                    lang = %args.lang,
                    attempt = args.prev_count + 1,
                    max_attempts = MAX_FFMPEG_RESTARTS,
                    "ffmpeg rtmp restarted"
                );
            }
            Err(e) => {
                emit_output_health(
                    self.output_health_tx.as_ref(),
                    args.output_controls_enabled,
                    &args.output_id,
                    &args.id,
                    &args.lang,
                    &args.destination_platform,
                    OutputHealthState::Failed,
                    args.prev_count,
                    Some(OutputDegradationLabel::RtmpPublishError),
                    Some(e.clone()),
                );
                emit_render_graph_adapter_event(
                    &args.render_graph_node,
                    RenderGraphNodeState::Failed,
                    "render_graph.node.failure",
                    Some(e.clone()),
                );
                tracing::error!(
                    stream_id = %args.id,
                    lang = %args.lang,
                    error = %e,
                    "ffmpeg rtmp restart failed"
                );
            }
        }
    }

    fn spawn_stream_inner(&mut self, args: StreamSpawnArgs) -> Result<(), String> {
        let audio_fifo = format!("/tmp/brivva_audio_{}", args.stream_id);
        let subtitle_textfile = format!("/tmp/brivva_subtitle_{}.txt", args.stream_id);

        let _ = std::fs::remove_file(&audio_fifo);
        let _ = std::fs::write(&subtitle_textfile, "");
        std::process::Command::new("mkfifo")
            .arg(&audio_fifo)
            .output()
            .map_err(|e| format!("mkfifo failed: {}", e))?;

        let encoder = self.video_encoder;
        let video_profile = self
            .video_profile
            .for_destination_platform(&args.destination_platform);
        let ffmpeg_args = build_ffmpeg_args_with_profile(
            &audio_fifo,
            Some(&subtitle_textfile),
            &args.rtmp_urls,
            video_profile,
            encoder,
            args.video_input_codec,
        );
        tracing::info!(
            stream_id = %args.stream_id,
            lang = %args.lang,
            destination_platform = %args.destination_platform,
            ffmpeg_args = ?ffmpeg_args.iter().map(|arg| redact_rtmp_secrets(arg)).collect::<Vec<_>>(),
            video_encoder = encoder.codec_name(),
            input_fps = video_profile.input_fps,
            output_fps = video_profile.output_fps,
            max_width = video_profile.max_width,
            max_height = video_profile.max_height,
            bitrate_kbps = video_profile.bitrate_kbps,
            maxrate_kbps = video_profile.maxrate_kbps,
            keyframe_interval_frames = video_profile.keyframe_interval_frames,
            pad_to_canvas = video_profile.pad_to_canvas,
            video_input_codec = args.video_input_codec.log_label(),
            "ffmpeg spawn: WebRTC video pipe re-encode enabled"
        );

        let mut child = std::process::Command::new("ffmpeg")
            .args(&ffmpeg_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;

        // Drain ffmpeg's stderr line-by-line so its diagnostics land in our
        // tracing pipeline. Without this reader the piped stderr fills up
        // (~64 KB kernel buffer) and eventually blocks ffmpeg's log writes,
        // but more importantly we were flying blind on every RTMP failure:
        // Grip drops, ffmpeg exits, the restart loop spins, and no log line
        // ever told us why. The thread exits naturally when ffmpeg closes
        // stderr on process exit (BufRead::lines yields None at EOF).
        if let Some(stderr) = child.stderr.take() {
            let sid_for_log = args.stream_id.clone();
            let lang_for_log = args.lang.clone();
            let output_id_for_log = args.output_id.clone();
            let platform_for_log = args.destination_platform.clone();
            let destinations_for_log = args.destinations.clone();
            let output_controls_for_log = args.output_controls_enabled;
            let output_health_tx_for_log = self.output_health_tx.clone();
            let thread_name = format!("stderr-drain-{}", args.stream_id);
            if let Err(e) = thread::Builder::new().name(thread_name).spawn(move || {
                let mut progress = FfmpegProgressMonitor::default();
                drain_stderr_lines(BufReader::new(stderr), |line| {
                    for alert in progress.ingest_line(&line) {
                        match alert {
                            FfmpegProgressAlert::SlowEncode {
                                speed,
                                consecutive_ticks,
                            } => {
                                let snapshot = progress.snapshot();
                                tracing::warn!(
                                    stream_id = %sid_for_log,
                                    lang = %lang_for_log,
                                    speed,
                                    consecutive_ticks,
                                    fps = snapshot.fps,
                                    dup_frames = snapshot.dup_frames,
                                    drop_frames = snapshot.drop_frames,
                                    "ffmpeg encode below realtime"
                                );
                                emit_output_health(
                                    output_health_tx_for_log.as_ref(),
                                    output_controls_for_log,
                                    &output_id_for_log,
                                    &sid_for_log,
                                    &lang_for_log,
                                    &platform_for_log,
                                    OutputHealthState::Degraded,
                                    0,
                                    Some(OutputDegradationLabel::SlowEncode),
                                    Some(format!(
                                        "speed={speed} consecutive_ticks={consecutive_ticks}"
                                    )),
                                );
                            }
                            FfmpegProgressAlert::DroppedFrames { total, delta } => {
                                let snapshot = progress.snapshot();
                                tracing::warn!(
                                    stream_id = %sid_for_log,
                                    lang = %lang_for_log,
                                    total_drop_frames = total,
                                    delta_drop_frames = delta,
                                    speed = snapshot.speed,
                                    fps = snapshot.fps,
                                    "ffmpeg output dropped frames"
                                );
                                emit_output_health(
                                    output_health_tx_for_log.as_ref(),
                                    output_controls_for_log,
                                    &output_id_for_log,
                                    &sid_for_log,
                                    &lang_for_log,
                                    &platform_for_log,
                                    OutputHealthState::Degraded,
                                    0,
                                    Some(OutputDegradationLabel::DroppedFrames),
                                    Some(format!(
                                        "total_drop_frames={total} delta_drop_frames={delta}"
                                    )),
                                );
                            }
                        }
                    }
                    if let Some(index) = parse_tee_slave_muxer_index(&line)
                        && let Some(destination) = destinations_for_log.get(index)
                    {
                        tracing::warn!(
                            stream_id = %sid_for_log,
                            lang = %lang_for_log,
                            destination_index = index,
                            destination_platform = %destination.platform,
                            destination_url = %redact_rtmp_secrets(&destination.url),
                            "ffmpeg tee destination reported failure"
                        );
                    }
                    tracing::warn!(
                        stream_id = %sid_for_log,
                        lang = %lang_for_log,
                        "ffmpeg stderr: {line}"
                    );
                });
            }) {
                tracing::error!(
                    stream_id = %args.stream_id,
                    error = %e,
                    "ffmpeg stderr drain thread spawn failed"
                );
            }
        }

        let buffers = args.existing_buffers.unwrap_or_else(StreamBuffers::new);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let delay = Duration::from_millis(args.delay_ms);
        // Seed last_write to 0 — the "not yet written" sentinel read by
        // `kill_idle_streams`. A freshly-spawned stream is not idle until
        // the drain has produced at least one successful write; before
        // 2026-04-22 we seeded to `now_unix_ms()` instead, which meant the
        // idle detector fired at +25s for any session where the host
        // (browser mic/cam permission dialog, OBS warm-up) took longer
        // than 25s to push its first byte. That falsely burned one of
        // `MAX_FFMPEG_RESTARTS` on every session.
        let last_write_ms = Arc::new(AtomicI64::new(0));

        let video_stdin = child
            .stdin
            .take()
            .ok_or_else(|| "FFmpeg video stdin unavailable".to_string())?;

        // Video drain
        let v_buf = buffers.video.clone();
        let v_stop = stop_flag.clone();
        let v_sid = args.stream_id.clone();
        let v_last = last_write_ms.clone();
        let v_metrics = self.metrics.clone();
        let video_handle = thread::Builder::new()
            .name(format!("video-drain-{}", args.stream_id))
            .spawn(move || {
                video_drain_loop(VideoDrainCtx {
                    stream_id: v_sid,
                    video_buf: v_buf,
                    video_stdin,
                    input_codec: args.video_input_codec,
                    delay,
                    stop: v_stop,
                    last_write_ms: v_last,
                    metrics: v_metrics,
                })
            })
            .map_err(|e| format!("Video thread spawn failed: {}", e))?;

        // Audio drain
        let audio_ctx = AudioDrainCtx {
            stream_id: args.stream_id.clone(),
            host_buf: buffers.audio.clone(),
            tts_queue: buffers.tts.clone(),
            fifo_path: audio_fifo.clone(),
            delay,
            is_source: args.is_source,
            host_gain: args.host_gain,
            lang: args.lang.clone(),
            destination_platform: args.destination_platform.clone(),
            passthrough: args.passthrough,
            stop: stop_flag.clone(),
            last_write_ms: last_write_ms.clone(),
            metrics: self.metrics.clone(),
        };
        let audio_handle = thread::Builder::new()
            .name(format!("audio-drain-{}", args.stream_id))
            .spawn(move || audio_drain_loop(audio_ctx))
            .map_err(|e| format!("Audio thread spawn failed: {}", e))?;

        self.streams.insert(
            args.stream_id.clone(),
            RtmpStream {
                child,
                video_handle: Some(video_handle),
                audio_handle: Some(audio_handle),
                audio_fifo,
                subtitle_textfile,
                lang: args.lang,
                rtmp_urls: args.rtmp_urls,
                destinations: args.destinations,
                delay,
                is_source: args.is_source,
                host_gain: args.host_gain,
                passthrough: args.passthrough,
                output_id: args.output_id,
                destination_platform: args.destination_platform,
                output_controls_enabled: args.output_controls_enabled,
                render_graph_node: args.render_graph_node,
                buffers,
                stop_flag,
                restart_count: 0,
                last_write_ms,
                video_input_codec: args.video_input_codec,
            },
        );

        Ok(())
    }

    pub async fn stop_all(&mut self) {
        for (id, mut stream) in self.streams.drain() {
            stream.stop_flag.store(true, Ordering::Release);
            emit_output_health(
                self.output_health_tx.as_ref(),
                stream.output_controls_enabled,
                &stream.output_id,
                &id,
                &stream.lang,
                &stream.destination_platform,
                OutputHealthState::Stopped,
                stream.restart_count,
                None,
                None,
            );
            if let Some(node) = stream.render_graph_node.as_mut() {
                node.transition(RenderGraphNodeState::Stopped);
            }
            emit_render_graph_adapter_event(
                &stream.render_graph_node,
                RenderGraphNodeState::Stopped,
                "render_graph.node.stop",
                None,
            );
            match stream.child.kill() {
                Ok(_) => {
                    let _ = stream.child.wait();
                    tracing::info!(stream_id = %id, "ffmpeg rtmp process killed (stop_all)");
                }
                Err(e) => tracing::error!(stream_id = %id, error = %e, "ffmpeg kill failed"),
            }
            // Drain threads exit on the next tick (≤20 ms) once they observe
            // stop_flag or hit EPIPE; 500 ms covers a worst-case scheduler
            // gap. Longer than that we let go without blocking session teardown.
            let join_timeout = Duration::from_millis(500);
            for (label, handle) in [
                ("video", stream.video_handle.take()),
                ("audio", stream.audio_handle.take()),
            ] {
                if let Some(h) = handle {
                    let id_clone = id.clone();
                    let result = tokio::time::timeout(join_timeout, async {
                        tokio::task::spawn_blocking(move || h.join()).await
                    })
                    .await;
                    match result {
                        Ok(Ok(Ok(()))) => {}
                        Ok(Ok(Err(_))) => {
                            eprintln!("[FFMPEG:{}] {} thread panicked", id_clone, label)
                        }
                        Ok(Err(_)) => {
                            eprintln!("[FFMPEG:{}] {} thread join cancelled", id_clone, label)
                        }
                        Err(_) => tracing::debug!(
                            stream_id = %id_clone,
                            thread = label,
                            "drain thread join timed out, will exit on next tick"
                        ),
                    }
                }
            }
            let _ = std::fs::remove_file(&stream.audio_fifo);
            let _ = std::fs::remove_file(&stream.subtitle_textfile);
        }
    }
}

pub type SharedRtmpManager = Arc<tokio::sync::Mutex<RtmpManager>>;

pub fn spawn_health_monitor(
    manager: SharedRtmpManager,
    stop_flag: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            if stop_flag.load(Ordering::Acquire) {
                break;
            }
            let crashed = {
                let mut mgr = manager.lock().await;
                mgr.kill_idle_streams();
                mgr.detect_crashed()
            };
            for (
                id,
                lang,
                rtmp_urls,
                delay_ms,
                is_source,
                host_gain,
                passthrough,
                output_id,
                destination_platform,
                output_controls_enabled,
                render_graph_node,
                prev_count,
                destinations,
                buffers,
            ) in crashed
            {
                tokio::time::sleep(FFMPEG_RESTART_DELAY).await;
                if stop_flag.load(Ordering::Acquire) {
                    break;
                }
                let mut mgr = manager.lock().await;
                mgr.restart_stream(RestartStreamArgs {
                    id,
                    lang,
                    rtmp_urls,
                    delay_ms,
                    is_source,
                    host_gain,
                    passthrough,
                    output_id,
                    destination_platform,
                    output_controls_enabled,
                    render_graph_node,
                    prev_count,
                    destinations,
                    buffers,
                });
            }
        }
    })
}

#[cfg(test)]
mod tests;
