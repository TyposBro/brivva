use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use axum::extract::ws::{Message, WebSocket};
use futures_util::StreamExt;
use serde::Serialize;
use tokio::{
    sync::{oneshot, Mutex},
    time::{self, Duration},
};

use crate::{
    output::{
        channel_sink::{run_audio_writer, run_video_writer, AudioSinkCommand, ChannelAudioSink, ChannelVideoSink},
        FfmpegProcess, FfmpegProcessConfig,
    },
    protocol::{parse_message, Message as ProtocolMessage},
    session::{SessionConfig, SourceSession},
};

pub type SharedSourceSession = Arc<Mutex<SourceSession>>;

pub fn new_source_session(delay_ms: u64) -> SharedSourceSession {
    Arc::new(Mutex::new(SourceSession::new(
        Instant::now(),
        SessionConfig { delay_ms },
    )))
}

#[derive(Debug, Clone)]
pub struct SharedPublisherDebug(Arc<PublisherDebugState>);

#[derive(Debug)]
struct PublisherDebugState {
    ffmpeg_pid: u32,
    output_url: String,
    audio_fifo: String,
    exited: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublisherDebugSnapshot {
    pub ffmpeg_pid: u32,
    pub output_url: String,
    pub audio_fifo: String,
    pub ffmpeg_exited: bool,
}

impl SharedPublisherDebug {
    fn new(ffmpeg_pid: u32, output_url: String, audio_fifo: String) -> Self {
        Self(Arc::new(PublisherDebugState {
            ffmpeg_pid,
            output_url,
            audio_fifo,
            exited: AtomicBool::new(false),
        }))
    }

    fn mark_exited(&self) {
        self.0.exited.store(true, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> PublisherDebugSnapshot {
        PublisherDebugSnapshot {
            ffmpeg_pid: self.0.ffmpeg_pid,
            output_url: self.0.output_url.clone(),
            audio_fifo: self.0.audio_fifo.clone(),
            ffmpeg_exited: self.0.exited.load(Ordering::Relaxed),
        }
    }
}

pub struct SourceStreamRuntime {
    pub session: SharedSourceSession,
    pub debug: SharedPublisherDebug,
    pub stop_tx: oneshot::Sender<()>,
}

pub async fn spawn_source_runtime(
    output_url: String,
    delay_ms: u64,
) -> Result<SourceStreamRuntime, String> {
    let session = new_source_session(delay_ms);
    let config = FfmpegProcessConfig {
        output_url: output_url.clone(),
        audio_fifo: PathBuf::from(format!("/tmp/brivva_audio_{}", uuid_like())),
        copy_video: false,
        ..FfmpegProcessConfig::default()
    };
    let ffmpeg = tokio::task::spawn_blocking(move || config.spawn())
        .await
        .map_err(|e| format!("spawn_blocking panic: {e}"))?
        .map_err(|e| e.to_string())?;

    let (audio_tx, audio_rx) = std::sync::mpsc::channel::<AudioSinkCommand>();
    let (video_tx, video_rx) = std::sync::mpsc::channel::<Vec<u8>>();

    let FfmpegProcess {
        mut child,
        audio_writer,
        video_stdin,
        audio_fifo,
    } = ffmpeg;
    let debug = SharedPublisherDebug::new(
        child.id(),
        output_url,
        audio_fifo.display().to_string(),
    );
    let writer_debug = debug.clone();

    // Separate threads prevent audio/video writes from deadlocking each other
    let audio_handle = std::thread::spawn(move || {
        run_audio_writer(audio_rx, audio_writer);
    });
    let video_handle = std::thread::spawn(move || {
        run_video_writer(video_rx, video_stdin);
    });
    let ffmpeg_pid = child.id();
    std::thread::spawn(move || {
        let _ = audio_handle.join();
        let _ = video_handle.join();
        eprintln!("[ffmpeg-supervisor] writers exited, killing ffmpeg pid={ffmpeg_pid}");
        let _ = child.kill();
        match child.wait() {
            Ok(status) => eprintln!("[ffmpeg-supervisor] ffmpeg exited: {status}"),
            Err(e) => eprintln!("[ffmpeg-supervisor] wait error: {e}"),
        }
        let _ = std::fs::remove_file(&audio_fifo);
        writer_debug.mark_exited();
    });

    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
    let tick_session = session.clone();

    tokio::spawn(async move {
        let mut audio_sink = ChannelAudioSink::new(audio_tx);
        let mut video_sink = ChannelVideoSink::new(video_tx);
        let mut interval = time::interval(Duration::from_millis(10));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let mut locked = tick_session.lock().await;
                    locked.tick(Instant::now(), &mut audio_sink, &mut video_sink);
                }
                _ = &mut stop_rx => break,
            }
        }
        // Sinks drop here → channel closes → writer thread exits → FFmpeg killed
    });

    Ok(SourceStreamRuntime {
        session,
        debug,
        stop_tx,
    })
}

pub async fn handle_source_socket(mut socket: WebSocket, session: SharedSourceSession) {
    let mut session_initialized = false;
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Binary(bytes) => {
                let parsed = match parse_message(&bytes) {
                    Ok(parsed) => parsed,
                    Err(err) => {
                        let _ = socket
                            .send(Message::Text(format!("protocol error: {err}")))
                            .await;
                        continue;
                    }
                };

                match parsed {
                    ProtocolMessage::SessionInit(_) => {
                        let mut locked = session.lock().await;
                        locked.reset_session_start(Instant::now());
                        session_initialized = true;
                    }
                    ProtocolMessage::AudioFrame(frame) => {
                        if !session_initialized {
                            let _ = socket
                                .send(Message::Text(
                                    "protocol error: session_init required before media".into(),
                                ))
                                .await;
                            break;
                        }
                        let mut locked = session.lock().await;
                        let _ = locked.push_audio(frame);
                    }
                    ProtocolMessage::VideoChunk(chunk) => {
                        if !session_initialized {
                            let _ = socket
                                .send(Message::Text(
                                    "protocol error: session_init required before media".into(),
                                ))
                                .await;
                            break;
                        }
                        let mut locked = session.lock().await;
                        let _ = locked.push_video(chunk);
                    }
                    ProtocolMessage::StreamEnd(_) => break,
                    ProtocolMessage::Ping(_) => {}
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{nanos:x}")
}
