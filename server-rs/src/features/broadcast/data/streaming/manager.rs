//! RtmpManager: manages all FFmpeg RTMP streams for a session.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::core::config::BYTES_PER_SEC;
use super::types::{
    StreamingPcm, QueuedAudio,
    MAX_VIDEO_CHUNKS, DEFAULT_DELAY_MS, MAX_FFMPEG_RESTARTS,
    VIDEO_CRF, VIDEO_MAX_BITRATE, VIDEO_BUFSIZE, VIDEO_GOP_SIZE,
    AUDIO_BITRATE, AUDIO_CHANNELS_OUT,
};
use super::{video_drain, audio_drain};
use super::ffmpeg_spawn::{create_audio_fifo, spawn_ffmpeg_process};
use super::stream_lifecycle::{
    RtmpStream, check_stream_health, cleanup_single_stream,
    kill_ffmpeg_process, join_drain_threads,
};

// ── Parameter structs ────────────────────────────────────

/// Identifies a stream: id + language + RTMP destination.
pub(crate) struct StreamConfig {
    pub(crate) stream_id: String,
    pub(crate) lang: String,
    pub(crate) rtmp_url: String,
}

/// State carried across a restart: previous attempt count + reusable audio queue.
pub(crate) struct RestartState {
    pub(crate) prev_count: u32,
    pub(crate) audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
}

/// Newly-spawned FFmpeg process and its drain thread handles.
struct NewStream {
    child: std::process::Child,
    video_handle: thread::JoinHandle<()>,
    audio_handle: thread::JoinHandle<()>,
    audio_fifo: String,
    audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    stop_flag: Arc<AtomicBool>,
    rtmp_error: Arc<AtomicBool>,
    drift_ms: Arc<AtomicU64>,
}

/// Everything needed to spawn the video + audio drain threads.
struct DrainSetup {
    stream_id: String,
    is_restart: bool,
    stdin: std::process::ChildStdin,
    audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    audio_fifo: String,
    stop_flag: Arc<AtomicBool>,
    drift_ms: Arc<AtomicU64>,
}

/// Config for the audio drain thread spawner.
struct AudioDrainSetup {
    stream_id: String,
    audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    fifo_path: String,
    delay: Duration,
    stop: Arc<AtomicBool>,
    drift_ms: Arc<AtomicU64>,
}

// ── RtmpManager ──────────────────────────────────────────

/// Manages all FFmpeg RTMP streams for a session.
pub struct RtmpManager {
    streams: HashMap<String, RtmpStream>,
    video_chunks: Arc<StdMutex<VecDeque<(Instant, Vec<u8>)>>>,
    video_init_segment: Arc<StdMutex<Option<Vec<u8>>>>,
    broadcast_delay: Duration,
    video_codec: String,
}

impl Default for RtmpManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Public API ───────────────────────────────────────────

impl RtmpManager {
    pub fn new() -> Self {
        Self::with_delay(DEFAULT_DELAY_MS)
    }

    pub fn with_delay(delay_ms: u64) -> Self {
        tracing::info!("[SYNC] Broadcast delay: {}ms", delay_ms);
        Self {
            streams: HashMap::new(),
            video_chunks: Arc::new(StdMutex::new(VecDeque::new())),
            video_init_segment: Arc::new(StdMutex::new(None)),
            broadcast_delay: Duration::from_millis(delay_ms),
            video_codec: "vp8".to_string(),
        }
    }

    pub fn broadcast_delay(&self) -> Duration {
        self.broadcast_delay
    }

    /// Return the audio queue depth for each language (lang -> queue size).
    pub fn queue_depths(&self) -> HashMap<String, usize> {
        self.streams.values().map(|s| {
            let depth = s.audio_queue.lock().unwrap().len();
            (s.lang.clone(), depth)
        }).collect()
    }

    pub fn set_video_codec(&mut self, codec: &str) {
        self.video_codec = codec.to_string();
        tracing::info!("[FFMPEG] Video codec set to: {} ({})",
            codec, if codec == "h264" { "passthrough" } else { "re-encode" });
    }

    pub fn start_stream(
        &mut self,
        stream_id: &str,
        lang: &str,
        rtmp_url: &str,
    ) -> Result<(), String> {
        let config = StreamConfig {
            stream_id: stream_id.to_string(),
            lang: lang.to_string(),
            rtmp_url: rtmp_url.to_string(),
        };
        self.spawn_stream_inner(&config, None)?;
        tracing::info!(
            "[FFMPEG] Started RTMP stream {} ({}) -> {} [delay={}ms]",
            stream_id, lang, rtmp_url, self.broadcast_delay.as_millis()
        );
        Ok(())
    }

    pub fn push_video_chunk(&self, data: &[u8]) {
        self.save_init_segment(data);
        let mut buf = self.video_chunks.lock().unwrap();
        buf.push_back((Instant::now(), data.to_vec()));
        let buf_len = buf.len();
        self.drop_overflow_chunks(&mut buf, buf_len);
        if buf_len.is_multiple_of(50) {
            tracing::debug!("[VIDEO] buffered chunk: {}B (buf_depth={})", data.len(), buf_len);
        }
    }

    pub fn queue_audio(&self, lang: &str, pcm: Vec<u8>, utterance_start: Instant) {
        let pcm_len = pcm.len();
        let pcm_arc = Arc::new(StdMutex::new(pcm));
        let complete = Arc::new(AtomicBool::new(true));
        for stream in self.streams.values() {
            if stream.lang == lang {
                let mut q = stream.audio_queue.lock().unwrap();
                q.push_back(QueuedAudio {
                    play_at: utterance_start,
                    pcm: pcm_arc,
                    complete,
                });
                tracing::debug!(
                    "[AUDIO:{}] queued passthrough audio: {}KB ({:.1}s) queue_depth={}",
                    lang, pcm_len / 1024, pcm_len as f64 / BYTES_PER_SEC, q.len()
                );
                return;
            }
        }
        tracing::warn!("[AUDIO] no stream found for lang={}, audio dropped", lang);
    }

    pub fn queue_streaming_audio(&self, lang: &str, utterance_start: Instant) -> StreamingPcm {
        let streaming = StreamingPcm::new();
        for stream in self.streams.values() {
            if stream.lang == lang {
                let mut q = stream.audio_queue.lock().unwrap();
                q.push_back(QueuedAudio {
                    play_at: utterance_start,
                    pcm: streaming.pcm.clone(),
                    complete: streaming.complete.clone(),
                });
                tracing::debug!(
                    "[AUDIO:{}] queued streaming TTS slot, queue_depth={}",
                    lang, q.len()
                );
                return streaming;
            }
        }
        tracing::warn!("[AUDIO] no stream found for lang={}, streaming slot orphaned", lang);
        streaming
    }

    pub fn max_drift_ms(&self) -> u64 {
        self.streams.values()
            .map(|s| s.drift_ms.load(Ordering::Relaxed))
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn detect_crashed(&mut self) -> Vec<(StreamConfig, RestartState)> {
        let to_restart = self.collect_crashed_streams();
        self.cleanup_crashed_streams(to_restart)
    }

    pub(crate) fn restart_stream(
        &mut self,
        config: &StreamConfig,
        state: RestartState,
    ) {
        match self.spawn_stream_inner(config, Some(state.audio_queue)) {
            Ok(()) => {
                if let Some(stream) = self.streams.get_mut(&config.stream_id) {
                    stream.restart_count = state.prev_count + 1;
                }
                tracing::info!(
                    "[FFMPEG] Restarted stream {} ({}) attempt {}/{}",
                    config.stream_id, config.lang, state.prev_count + 1, MAX_FFMPEG_RESTARTS
                );
            }
            Err(e) => {
                tracing::error!(
                    "[FFMPEG] Restart failed for {} ({}): {}",
                    config.stream_id, config.lang, e
                );
            }
        }
    }

    pub async fn stop_all(&mut self) {
        for (id, mut stream) in self.streams.drain() {
            stream.stop_flag.store(true, Ordering::Release);
            kill_ffmpeg_process(&id, &mut stream.child);
            join_drain_threads(&id, &mut stream).await;
            let _ = std::fs::remove_file(&stream.audio_fifo);
        }
    }

    pub async fn restart_all(&mut self) {
        let configs = self.collect_stream_configs_for_restart();
        if configs.is_empty() {
            tracing::info!("[RTMP] restart_all: no streams to restart");
            return;
        }
        tracing::info!("[RTMP] restarting {} stream(s)", configs.len());
        self.stop_all().await;
        self.respawn_all(configs);
        tracing::info!("[RTMP] restart complete");
    }
}

// ── Private helpers ──────────────────────────────────────

impl RtmpManager {
    fn spawn_stream_inner(
        &mut self,
        config: &StreamConfig,
        existing_queue: Option<Arc<StdMutex<VecDeque<QueuedAudio>>>>,
    ) -> Result<(), String> {
        let (child, stdin, rtmp_error, audio_fifo) = self.setup_ffmpeg(&config.stream_id, &config.rtmp_url)?;
        let (audio_queue, stop_flag, is_restart) = prepare_stream_state(existing_queue);
        let drift_ms = Arc::new(AtomicU64::new(0));
        let setup = DrainSetup {
            stream_id: config.stream_id.clone(),
            is_restart,
            stdin,
            audio_queue: audio_queue.clone(),
            audio_fifo: audio_fifo.clone(),
            stop_flag: stop_flag.clone(),
            drift_ms: drift_ms.clone(),
        };
        let (video_handle, audio_handle) = self.spawn_drain_threads(setup)?;
        let new_stream = NewStream {
            child, video_handle, audio_handle, audio_fifo,
            audio_queue, stop_flag, rtmp_error, drift_ms,
        };
        self.register_stream(config, new_stream);
        Ok(())
    }

    fn setup_ffmpeg(
        &self,
        stream_id: &str,
        rtmp_url: &str,
    ) -> Result<(std::process::Child, std::process::ChildStdin, Arc<AtomicBool>, String), String> {
        let audio_fifo = create_audio_fifo(stream_id)?;
        let args = self.build_ffmpeg_args(&audio_fifo, rtmp_url);
        let (child, stdin, rtmp_error) = spawn_ffmpeg_process(stream_id, &args)?;
        Ok((child, stdin, rtmp_error, audio_fifo))
    }

    fn spawn_drain_threads(
        &self,
        setup: DrainSetup,
    ) -> Result<(thread::JoinHandle<()>, thread::JoinHandle<()>), String> {
        let video_config = video_drain::VideoDrainConfig {
            stream_id: setup.stream_id.clone(),
            chunk_buffer: self.video_chunks.clone(),
            init_segment: self.video_init_segment.clone(),
            is_restart: setup.is_restart,
            delay: self.broadcast_delay,
            stop: setup.stop_flag.clone(),
        };
        let video_handle = self.spawn_video_drain(video_config, setup.stdin)?;
        let audio_setup = AudioDrainSetup {
            stream_id: setup.stream_id,
            audio_queue: setup.audio_queue,
            fifo_path: setup.audio_fifo,
            delay: self.broadcast_delay,
            stop: setup.stop_flag,
            drift_ms: setup.drift_ms,
        };
        let audio_handle = spawn_audio_drain(audio_setup)?;
        Ok((video_handle, audio_handle))
    }

    fn register_stream(&mut self, config: &StreamConfig, stream: NewStream) {
        self.streams.insert(
            config.stream_id.clone(),
            RtmpStream {
                child: stream.child,
                video_handle: Some(stream.video_handle),
                audio_handle: Some(stream.audio_handle),
                audio_fifo: stream.audio_fifo,
                lang: config.lang.clone(),
                rtmp_url: config.rtmp_url.clone(),
                audio_queue: stream.audio_queue,
                stop_flag: stream.stop_flag,
                restart_count: 0,
                rtmp_error: stream.rtmp_error,
                drift_ms: stream.drift_ms,
            },
        );
    }

    fn build_ffmpeg_args(&self, audio_fifo: &str, rtmp_url: &str) -> Vec<String> {
        let mut args = self.build_input_args(audio_fifo);
        args.extend(self.build_video_encoding_args());
        args.extend(Self::build_output_args(rtmp_url));
        args
    }

    fn build_input_args(&self, audio_fifo: &str) -> Vec<String> {
        vec![
            "-y".to_string(),
            "-loglevel".to_string(), "warning".to_string(),
            "-i".to_string(), "pipe:0".to_string(),
            "-f".to_string(), "s16le".to_string(),
            "-ar".to_string(), "44100".to_string(),
            "-ac".to_string(), "1".to_string(),
            "-i".to_string(), audio_fifo.to_string(),
        ]
    }

    fn build_video_encoding_args(&self) -> Vec<String> {
        tracing::info!("[FFMPEG] Encoding {} -> H.264 (ultrafast)", self.video_codec);
        vec![
            "-c:v".to_string(), "libx264".to_string(),
            "-preset".to_string(), "ultrafast".to_string(),
            "-tune".to_string(), "zerolatency".to_string(),
            "-crf".to_string(), VIDEO_CRF.to_string(),
            "-maxrate".to_string(), VIDEO_MAX_BITRATE.to_string(),
            "-bufsize".to_string(), VIDEO_BUFSIZE.to_string(),
            "-pix_fmt".to_string(), "yuv420p".to_string(),
            "-g".to_string(), VIDEO_GOP_SIZE.to_string(),
        ]
    }

    fn build_output_args(rtmp_url: &str) -> Vec<String> {
        vec![
            "-c:a".to_string(), "aac".to_string(),
            "-ac:a".to_string(), AUDIO_CHANNELS_OUT.to_string(),
            "-b:a".to_string(), AUDIO_BITRATE.to_string(),
            "-map".to_string(), "0:v".to_string(),
            "-map".to_string(), "1:a".to_string(),
            "-f".to_string(), "flv".to_string(),
            "-flvflags".to_string(), "no_duration_filesize".to_string(),
            "-rtmp_live".to_string(), "live".to_string(),
            rtmp_url.to_string(),
        ]
    }

    fn spawn_video_drain(
        &self,
        config: video_drain::VideoDrainConfig,
        stdin: std::process::ChildStdin,
    ) -> Result<thread::JoinHandle<()>, String> {
        let thread_name = format!("video-drain-{}", config.stream_id);
        thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                video_drain::video_chunk_drain_loop(config, stdin);
            })
            .map_err(|e| format!("Video thread spawn failed: {}", e))
    }

    fn save_init_segment(&self, data: &[u8]) {
        let mut init = self.video_init_segment.lock().unwrap();
        if init.is_none() {
            tracing::debug!("[VIDEO] saved init segment ({}B)", data.len());
            *init = Some(data.to_vec());
        }
    }

    fn drop_overflow_chunks(&self, buf: &mut VecDeque<(Instant, Vec<u8>)>, buf_len: usize) {
        let mut dropped = 0;
        while buf.len() > MAX_VIDEO_CHUNKS {
            buf.pop_front();
            dropped += 1;
        }
        if dropped > 0 {
            tracing::warn!("[VIDEO] buffer overflow: dropped {} old chunks (buf={})", dropped, buf_len);
        }
    }

    fn collect_crashed_streams(&mut self) -> Vec<(String, String, String)> {
        let mut to_restart = Vec::new();
        for (id, stream) in &mut self.streams {
            if let Some(restart_info) = check_stream_health(id, stream) {
                to_restart.push(restart_info);
            }
        }
        to_restart
    }

    fn cleanup_crashed_streams(
        &mut self,
        to_restart: Vec<(String, String, String)>,
    ) -> Vec<(StreamConfig, RestartState)> {
        to_restart
            .into_iter()
            .filter_map(|(id, lang, rtmp_url)| {
                let mut old = self.streams.remove(&id)?;
                let (prev_count, audio_queue) = cleanup_single_stream(&mut old);
                let config = StreamConfig { stream_id: id, lang, rtmp_url };
                let state = RestartState { prev_count, audio_queue };
                Some((config, state))
            })
            .collect()
    }

    fn collect_stream_configs_for_restart(&self) -> Vec<(StreamConfig, RestartState)> {
        self.streams.iter().map(|(id, s)| {
            let config = StreamConfig {
                stream_id: id.clone(),
                lang: s.lang.clone(),
                rtmp_url: s.rtmp_url.clone(),
            };
            let state = RestartState {
                prev_count: 0,
                audio_queue: s.audio_queue.clone(),
            };
            (config, state)
        }).collect()
    }

    fn respawn_all(&mut self, configs: Vec<(StreamConfig, RestartState)>) {
        for (config, state) in configs {
            self.restart_stream(&config, state);
        }
    }
}

// ── Free functions ───────────────────────────────────────

fn prepare_stream_state(
    existing_queue: Option<Arc<StdMutex<VecDeque<QueuedAudio>>>>,
) -> (Arc<StdMutex<VecDeque<QueuedAudio>>>, Arc<AtomicBool>, bool) {
    let is_restart = existing_queue.is_some();
    let audio_queue = existing_queue
        .unwrap_or_else(|| Arc::new(StdMutex::new(VecDeque::new())));
    let stop_flag = Arc::new(AtomicBool::new(false));
    (audio_queue, stop_flag, is_restart)
}

fn spawn_audio_drain(setup: AudioDrainSetup) -> Result<thread::JoinHandle<()>, String> {
    let thread_name = format!("audio-drain-{}", setup.stream_id);
    let config = audio_drain::AudioDrainConfig {
        stream_id: setup.stream_id,
        audio_queue: setup.audio_queue,
        fifo_path: setup.fifo_path,
        delay: setup.delay,
        stop: setup.stop,
        drift_ms: setup.drift_ms,
    };
    thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            audio_drain::audio_drain_loop(config);
        })
        .map_err(|e| format!("Audio thread spawn failed: {}", e))
}

/// Thread-safe wrapper
pub type SharedRtmpManager = Arc<tokio::sync::Mutex<RtmpManager>>;

/// Wrap a SharedRtmpManager as an ErasedRtmpManager for storage in Session.
pub fn erase_rtmp_manager(mgr: SharedRtmpManager) -> crate::core::types::ErasedRtmpManager {
    mgr as crate::core::types::ErasedRtmpManager
}

/// Downcast an ErasedRtmpManager back to SharedRtmpManager.
pub fn downcast_rtmp_manager(
    erased: &crate::core::types::ErasedRtmpManager,
) -> Option<SharedRtmpManager> {
    erased.clone().downcast::<tokio::sync::Mutex<RtmpManager>>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_build_input_args_with_audio_fifo_path() {
        let mgr = RtmpManager::new();

        let args = mgr.build_input_args("/tmp/test_audio_fifo");

        assert!(args.contains(&"pipe:0".to_string()));
        assert!(args.contains(&"/tmp/test_audio_fifo".to_string()));
        assert!(args.contains(&"s16le".to_string()));
        assert!(args.contains(&"44100".to_string()));
    }

    #[test]
    fn should_build_video_encoding_args_with_ultrafast_preset() {
        let mgr = RtmpManager::new();

        let args = mgr.build_video_encoding_args();

        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"ultrafast".to_string()));
        assert!(args.contains(&"zerolatency".to_string()));
        assert!(args.contains(&VIDEO_CRF.to_string()));
        assert!(args.contains(&VIDEO_MAX_BITRATE.to_string()));
        assert!(args.contains(&VIDEO_BUFSIZE.to_string()));
        assert!(args.contains(&VIDEO_GOP_SIZE.to_string()));
    }

    #[test]
    fn should_build_output_args_with_rtmp_url() {
        let url = "rtmp://live.example.com/app/key";

        let args = RtmpManager::build_output_args(url);

        assert_eq!(args.last().unwrap(), url);
        assert!(args.contains(&"aac".to_string()));
        assert!(args.contains(&AUDIO_CHANNELS_OUT.to_string()));
        assert!(args.contains(&AUDIO_BITRATE.to_string()));
        assert!(args.contains(&"flv".to_string()));
    }

    #[test]
    fn should_build_ffmpeg_args_combining_all_sections() {
        let mgr = RtmpManager::new();
        let fifo = "/tmp/test_fifo";
        let url = "rtmp://example.com/stream";

        let args = mgr.build_ffmpeg_args(fifo, url);

        assert!(args.contains(&fifo.to_string()), "should contain fifo path");
        assert!(args.contains(&"libx264".to_string()), "should contain video codec");
        assert_eq!(args.last().unwrap(), url, "should end with rtmp url");
    }

    #[test]
    fn should_create_new_queue_when_no_existing_queue() {
        let (queue, stop_flag, is_restart) = prepare_stream_state(None);

        assert!(!is_restart);
        assert!(!stop_flag.load(Ordering::Acquire));
        assert!(queue.lock().unwrap().is_empty());
    }

    #[test]
    fn should_reuse_existing_queue_on_restart() {
        let existing = Arc::new(StdMutex::new(VecDeque::new()));
        existing.lock().unwrap().push_back(QueuedAudio {
            play_at: Instant::now(),
            pcm: Arc::new(StdMutex::new(vec![1, 2, 3])),
            complete: Arc::new(AtomicBool::new(true)),
        });

        let (queue, _stop_flag, is_restart) = prepare_stream_state(Some(existing));

        assert!(is_restart);
        assert_eq!(queue.lock().unwrap().len(), 1);
    }

    #[test]
    fn should_create_manager_with_default_delay() {
        let mgr = RtmpManager::new();

        assert_eq!(mgr.broadcast_delay(), Duration::from_millis(DEFAULT_DELAY_MS));
    }

    #[test]
    fn should_create_manager_with_custom_delay() {
        let mgr = RtmpManager::with_delay(5000);

        assert_eq!(mgr.broadcast_delay(), Duration::from_millis(5000));
    }

    #[test]
    fn should_create_default_manager_same_as_new() {
        let from_default = RtmpManager::default();
        let from_new = RtmpManager::new();

        assert_eq!(from_default.broadcast_delay(), from_new.broadcast_delay());
    }

    #[test]
    fn should_set_video_codec() {
        let mut mgr = RtmpManager::new();

        mgr.set_video_codec("h264");

        assert_eq!(mgr.video_codec, "h264");
    }

    #[test]
    fn should_drop_overflow_chunks_when_exceeding_max() {
        let mgr = RtmpManager::new();
        let mut buf = VecDeque::new();
        for _ in 0..(MAX_VIDEO_CHUNKS + 5) {
            buf.push_back((Instant::now(), vec![0u8; 10]));
        }
        let buf_len = buf.len();

        mgr.drop_overflow_chunks(&mut buf, buf_len);

        assert_eq!(buf.len(), MAX_VIDEO_CHUNKS);
    }

    #[test]
    fn should_not_drop_chunks_when_under_max() {
        let mgr = RtmpManager::new();
        let mut buf = VecDeque::new();
        buf.push_back((Instant::now(), vec![0u8; 10]));
        let buf_len = buf.len();

        mgr.drop_overflow_chunks(&mut buf, buf_len);

        assert_eq!(buf.len(), 1);
    }

    #[test]
    fn should_save_init_segment_only_once() {
        let mgr = RtmpManager::new();
        let first = vec![1, 2, 3];
        let second = vec![4, 5, 6];

        mgr.save_init_segment(&first);
        mgr.save_init_segment(&second);

        let init = mgr.video_init_segment.lock().unwrap();
        assert_eq!(init.as_ref().unwrap(), &first);
    }

    #[test]
    fn should_erase_and_downcast_rtmp_manager() {
        let mgr: SharedRtmpManager = Arc::new(tokio::sync::Mutex::new(RtmpManager::new()));

        let erased = erase_rtmp_manager(mgr);
        let recovered = downcast_rtmp_manager(&erased);

        assert!(recovered.is_some());
    }
}
