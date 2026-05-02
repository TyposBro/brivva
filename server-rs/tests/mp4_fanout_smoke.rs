use server_rs::features::broadcast::data::ffmpeg::{
    RtmpDestination, RtmpManager, SharedRtmpManager, StartStreamGroupArgs, VideoProfileCaps,
    spawn_health_monitor,
};
use server_rs::features::broadcast::data::pipeline::{PipelineSession, start_stt_pipelines};
use server_rs::features::broadcast::domain::{
    Lang, LiveSession, LiveSessionHandle, LiveSessions, PipelineConfig, VideoEncoderKind,
};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug)]
struct Args {
    mp4: String,
    rtmp: Vec<RtmpDestination>,
    duration_secs: u64,
    encoder: VideoEncoderKind,
    max_width: u32,
    max_height: u32,
    max_fps: u32,
    capture_width: u32,
    capture_height: u32,
    capture_fps: u32,
    subtitle: Option<String>,
    source_lang: Lang,
    target_lang: Option<Lang>,
    soniox_api_key: String,
    soniox_ws_url: String,
    elevenlabs_api_key: String,
    elevenlabs_base_url: String,
}

#[tokio::test]
#[ignore = "manual RTMP e2e; requires MP4_FANOUT_SMOKE_MP4 and live destination secrets"]
async fn mp4_fanout_smoke() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let args = parse_args()?;
    tracing::info!(?args, "mp4 fanout smoke starting");

    let mut manager = RtmpManager::new();
    manager.set_video_encoder(args.encoder);
    manager.set_video_profile_caps(VideoProfileCaps::new(
        args.max_width,
        args.max_height,
        args.max_fps,
    ));
    manager.set_capture_video_profile(args.capture_width, args.capture_height, args.capture_fps);
    let output_lang = args
        .target_lang
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "pass".to_string());
    let is_passthrough = args.target_lang.is_none();
    manager.start_stream_group(StartStreamGroupArgs {
        stream_id: "mp4-fanout-smoke",
        lang: &output_lang,
        rtmp_urls: args.rtmp.iter().map(|dest| dest.url.clone()).collect(),
        destinations: args.rtmp.clone(),
        delay_ms: 0,
        is_source: is_passthrough,
        host_gain: if is_passthrough { 1.0 } else { 0.2 },
        output_id: None,
        destination_platform: "mp4-smoke",
        output_controls_enabled: true,
        render_graph_node: None,
        passthrough: is_passthrough,
    })?;
    if let Some(subtitle) = &args.subtitle {
        manager.push_subtitle(&output_lang, subtitle);
    }

    let manager: SharedRtmpManager = Arc::new(tokio::sync::Mutex::new(manager));
    let stt_tx = if let Some(target_lang) = args.target_lang.clone() {
        Some(spawn_translation_pipeline(
            &args,
            manager.clone(),
            target_lang,
        ))
    } else {
        None
    };
    let stop = Arc::new(AtomicBool::new(false));
    let monitor = spawn_health_monitor(manager.clone(), stop.clone());
    let mut video = spawn_video_ffmpeg(&args.mp4)?;
    let mut audio = spawn_audio_ffmpeg(&args.mp4)?;
    let video_stdout = video.stdout.take().ok_or("video ffmpeg stdout missing")?;
    let audio_stdout = audio.stdout.take().ok_or("audio ffmpeg stdout missing")?;
    let video_task = tokio::spawn(feed_h264(manager.clone(), video_stdout));
    let audio_task = tokio::spawn(feed_pcm(manager.clone(), stt_tx, audio_stdout));

    tokio::time::sleep(Duration::from_secs(args.duration_secs)).await;
    stop.store(true, std::sync::atomic::Ordering::Release);
    let _ = video.kill().await;
    let _ = audio.kill().await;
    video_task.abort();
    audio_task.abort();
    manager.lock().await.stop_all().await;
    let _ = monitor.await;
    tracing::info!("mp4 fanout smoke finished");
    Ok(())
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

fn parse_args() -> Result<Args, String> {
    let mp4 = std::env::var("MP4_FANOUT_SMOKE_MP4")
        .map_err(|_| "MP4_FANOUT_SMOKE_MP4 env var required".to_string())?;
    let mut rtmp = Vec::new();
    let duration_secs = env_u32("MP4_FANOUT_SMOKE_DURATION", 180) as u64;
    let encoder = VideoEncoderKind::from_wire(
        &std::env::var("MP4_FANOUT_SMOKE_ENCODER").unwrap_or_else(|_| "x264".to_string()),
    );
    let max_width = env_u32("BRIVVA_VIDEO_MAX_WIDTH", 1920);
    let max_height = env_u32("BRIVVA_VIDEO_MAX_HEIGHT", 1080);
    let max_fps = env_u32("BRIVVA_VIDEO_MAX_FPS", 30);
    let capture_width = env_u32("MP4_FANOUT_SMOKE_CAPTURE_WIDTH", max_width);
    let capture_height = env_u32("MP4_FANOUT_SMOKE_CAPTURE_HEIGHT", max_height);
    let capture_fps = env_u32("MP4_FANOUT_SMOKE_CAPTURE_FPS", max_fps);
    let subtitle = std::env::var("MP4_FANOUT_SMOKE_SUBTITLE").ok();
    let source_lang = parse_lang_env("MP4_FANOUT_SMOKE_SOURCE_LANG", Lang::Ko)?;
    let target_lang = parse_optional_lang_env("MP4_FANOUT_SMOKE_TARGET_LANG")?;
    let soniox_api_key = std::env::var("SONIOX_API_KEY").unwrap_or_default();
    let soniox_ws_url = std::env::var("SONIOX_WS_URL")
        .unwrap_or_else(|_| "wss://stt-rt.soniox.com/transcribe-websocket".to_string());
    let elevenlabs_api_key = std::env::var("ELEVENLABS_API_KEY").unwrap_or_default();
    let elevenlabs_base_url = std::env::var("ELEVENLABS_BASE_URL")
        .unwrap_or_else(|_| "https://api.elevenlabs.io".to_string());
    let youtube_url = std::env::var("YOUTUBE_RTMP_URL")
        .unwrap_or_else(|_| "rtmp://a.rtmp.youtube.com/live2".to_string());
    if let Ok(key) = std::env::var("STREAM_KEY_YOUTUBE") {
        rtmp.push(RtmpDestination::new(
            "youtube",
            format!("{}/{}", youtube_url.trim_end_matches('/'), key),
        ));
    }
    for (idx, url) in std::env::var("MP4_FANOUT_SMOKE_RTMP_URLS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .enumerate()
    {
        rtmp.push(RtmpDestination::new(format!("rtmp{idx}"), url.to_string()));
    }

    if rtmp.is_empty() {
        return Err("provide STREAM_KEY_YOUTUBE or MP4_FANOUT_SMOKE_RTMP_URLS".to_string());
    }
    Ok(Args {
        mp4,
        rtmp,
        duration_secs,
        encoder,
        max_width,
        max_height,
        max_fps,
        capture_width,
        capture_height,
        capture_fps,
        subtitle,
        source_lang,
        target_lang,
        soniox_api_key,
        soniox_ws_url,
        elevenlabs_api_key,
        elevenlabs_base_url,
    })
}

fn spawn_translation_pipeline(
    args: &Args,
    manager: SharedRtmpManager,
    target_lang: Lang,
) -> tokio::sync::mpsc::Sender<Vec<u8>> {
    let sessions: LiveSessions = Arc::new(dashmap::DashMap::new());
    let mut live = LiveSession::new(
        "mp4-fanout-smoke".to_string(),
        args.source_lang.clone(),
        None,
        Arc::new(PipelineConfig {
            soniox_api_key: args.soniox_api_key.clone(),
            soniox_ws_url: args.soniox_ws_url.clone(),
            elevenlabs_api_key: args.elevenlabs_api_key.clone(),
            elevenlabs_base_url: args.elevenlabs_base_url.clone(),
            ..Default::default()
        }),
    );
    live.rtmp_manager = Some(manager);
    sessions.insert("mp4-fanout-smoke".to_string(), live);
    let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let session = PipelineSession {
        handle: LiveSessionHandle::new("mp4-fanout-smoke".to_string(), sessions),
        source_lang: args.source_lang.clone(),
        target_langs: vec![target_lang],
        config: Arc::new(PipelineConfig {
            soniox_api_key: args.soniox_api_key.clone(),
            soniox_ws_url: args.soniox_ws_url.clone(),
            elevenlabs_api_key: args.elevenlabs_api_key.clone(),
            elevenlabs_base_url: args.elevenlabs_base_url.clone(),
            ..Default::default()
        }),
    };
    tokio::spawn(start_stt_pipelines(session, rx));
    tx
}

fn parse_lang_env(key: &str, default: Lang) -> Result<Lang, String> {
    match std::env::var(key) {
        Ok(value) => Lang::from_str(&value).ok_or_else(|| format!("{key} must be en|ko|ja|zh")),
        Err(_) => Ok(default),
    }
}

fn parse_optional_lang_env(key: &str) -> Result<Option<Lang>, String> {
    let Ok(value) = std::env::var(key) else {
        return Ok(None);
    };
    if value == "pass" || value.is_empty() {
        return Ok(None);
    }
    Lang::from_str(&value)
        .map(Some)
        .ok_or_else(|| format!("{key} must be pass|en|ko|ja|zh"))
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn spawn_video_ffmpeg(mp4: &str) -> std::io::Result<tokio::process::Child> {
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "warning",
            "-re",
            "-stream_loop",
            "-1",
            "-i",
            mp4,
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-tune",
            "zerolatency",
            "-f",
            "h264",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
}

fn spawn_audio_ffmpeg(mp4: &str) -> std::io::Result<tokio::process::Child> {
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "warning",
            "-re",
            "-stream_loop",
            "-1",
            "-i",
            mp4,
            "-vn",
            "-f",
            "s16le",
            "-ar",
            "44100",
            "-ac",
            "1",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
}

async fn feed_h264(
    manager: Arc<tokio::sync::Mutex<RtmpManager>>,
    stdout: tokio::process::ChildStdout,
) {
    let mut reader = BufReader::new(stdout);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let Ok(n) = reader.read(&mut buf).await else {
            break;
        };
        if n == 0 {
            break;
        }
        manager.lock().await.push_video_h264(&buf[..n]);
    }
}

async fn feed_pcm(
    manager: Arc<tokio::sync::Mutex<RtmpManager>>,
    stt_tx: Option<tokio::sync::mpsc::Sender<Vec<u8>>>,
    stdout: tokio::process::ChildStdout,
) {
    let mut reader = BufReader::new(stdout);
    let mut buf = vec![0u8; 1764];
    loop {
        if reader.read_exact(&mut buf).await.is_err() {
            break;
        }
        let chunk = buf.clone();
        manager.lock().await.push_host_audio(&chunk);
        if let Some(tx) = &stt_tx {
            let _ = tx.try_send(chunk);
        }
    }
}
