use std::{path::PathBuf, sync::Arc, time::Instant};

use axum::extract::ws::{Message, WebSocket};
use futures_util::StreamExt;
use tokio::{
    sync::{oneshot, Mutex},
    time::{self, Duration},
};

use crate::{
    output::{
        channel_sink::{run_sink_writer, ChannelAudioSink, ChannelVideoSink, SinkCommand},
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

pub struct SourceStreamRuntime {
    pub session: SharedSourceSession,
    pub stop_tx: oneshot::Sender<()>,
}

pub async fn spawn_source_runtime(
    output_url: String,
    delay_ms: u64,
) -> Result<SourceStreamRuntime, String> {
    let session = new_source_session(delay_ms);
    let config = FfmpegProcessConfig {
        output_url,
        audio_fifo: PathBuf::from(format!("/tmp/brivva_audio_{}", uuid_like())),
        copy_video: false,
        ..FfmpegProcessConfig::default()
    };
    let ffmpeg = tokio::task::spawn_blocking(move || config.spawn())
        .await
        .map_err(|e| format!("spawn_blocking panic: {e}"))?
        .map_err(|e| e.to_string())?;

    let (sink_tx, sink_rx) = std::sync::mpsc::channel::<SinkCommand>();

    // Destructure — writer thread owns I/O handles and FFmpeg child
    let FfmpegProcess {
        mut child,
        audio_writer,
        video_stdin,
        audio_fifo,
    } = ffmpeg;

    std::thread::spawn(move || {
        run_sink_writer(sink_rx, audio_writer, video_stdin);
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&audio_fifo);
    });

    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
    let tick_session = session.clone();

    tokio::spawn(async move {
        let mut audio_sink = ChannelAudioSink::new(sink_tx.clone());
        let mut video_sink = ChannelVideoSink::new(sink_tx);
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

    Ok(SourceStreamRuntime { session, stop_tx })
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
