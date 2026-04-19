use crate::features::broadcast::domain::LiveSessions;
use futures_util::SinkExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{self, Message},
};

use super::soniox::SonioxMode;

pub(super) const STT_RECONNECT_MAX: u32 = 5;
pub(super) const STT_RECONNECT_DELAY: Duration = Duration::from_secs(1);

pub(super) type SonioxWs = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub(super) type SonioxSink = futures_util::stream::SplitSink<SonioxWs, Message>;
pub(super) type SonioxStream = futures_util::stream::SplitStream<SonioxWs>;

pub(super) async fn connect_soniox(
    live_session_id: &str,
    live_sessions: &LiveSessions,
    tag: &str,
    reconnect_count: u32,
    soniox_ws_url: &str,
) -> Option<SonioxWs> {
    let max_attempts = if reconnect_count == 0 {
        10
    } else {
        STT_RECONNECT_MAX
    };

    for attempt in 1..=max_attempts {
        if !live_sessions.contains_key(live_session_id) {
            eprintln!("[STT {}] live session gone, stopping", tag);
            return None;
        }

        match tokio_tungstenite::connect_async(soniox_ws_url).await {
            Ok((stream, _)) => return Some(stream),
            Err(error) => {
                eprintln!(
                    "[STT {}] connect attempt {}/{} failed: {}",
                    tag, attempt, max_attempts, error
                );
                let delay = if reconnect_count == 0 {
                    Duration::from_secs(3)
                } else {
                    STT_RECONNECT_DELAY
                };
                tokio::time::sleep(delay).await;
            }
        }
    }

    eprintln!("[STT {}] giving up after {} attempts", tag, max_attempts);
    None
}

pub(super) async fn send_soniox_config(
    mode: &SonioxMode,
    tag: &str,
    stt_sink: &mut SonioxSink,
    reconnect_count: &mut u32,
    soniox_api_key: &str,
) -> Result<(), ()> {
    let config = mode.build_config(soniox_api_key);
    let config_json = serde_json::to_string(&config).map_err(|error| {
        eprintln!("[STT {}] config serialize error: {}", tag, error);
    })?;

    stt_sink
        .send(tungstenite::Message::Text(config_json.into()))
        .await
        .map_err(|error| {
            eprintln!("[STT {}] config send failed: {}", tag, error);
            *reconnect_count += 1;
        })
}

pub(super) fn spawn_audio_forwarder(
    audio_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>>,
    mut stt_sink: SonioxSink,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut audio_rx = audio_rx.lock().await;
        while let Some(data) = audio_rx.recv().await {
            if stt_sink
                .send(tungstenite::Message::Binary(data.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    })
}
