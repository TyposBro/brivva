use server_rs::features::broadcast::data::ffmpeg::{
    RtmpDestination, RtmpManager, StartStreamGroupArgs, VideoProfileCaps, spawn_health_monitor,
};
use server_rs::features::broadcast::domain::VideoEncoderKind;
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    manager.start_stream_group(StartStreamGroupArgs {
        stream_id: "mp4-fanout-smoke",
        lang: "pass",
        rtmp_urls: args.rtmp.iter().map(|dest| dest.url.clone()).collect(),
        destinations: args.rtmp.clone(),
        delay_ms: 0,
        is_source: true,
        host_gain: 1.0,
        output_id: None,
        destination_platform: "mp4-smoke",
        output_controls_enabled: true,
        render_graph_node: None,
        passthrough: true,
    })?;
    if let Some(subtitle) = &args.subtitle {
        manager.push_subtitle("pass", subtitle);
    }

    let manager = Arc::new(tokio::sync::Mutex::new(manager));
    let stop = Arc::new(AtomicBool::new(false));
    let monitor = spawn_health_monitor(manager.clone(), stop.clone());
    let mut video = spawn_video_ffmpeg(&args.mp4)?;
    let mut audio = spawn_audio_ffmpeg(&args.mp4)?;
    let video_stdout = video.stdout.take().ok_or("video ffmpeg stdout missing")?;
    let audio_stdout = audio.stdout.take().ok_or("audio ffmpeg stdout missing")?;
    let video_task = tokio::spawn(feed_h264(manager.clone(), video_stdout));
    let audio_task = tokio::spawn(feed_pcm(manager.clone(), audio_stdout));

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
    let mut mp4 = None;
    let mut rtmp = Vec::new();
    let mut duration_secs = 180;
    let mut encoder = VideoEncoderKind::X264;
    let mut max_width = env_u32("BRIVVA_VIDEO_MAX_WIDTH", 1920);
    let mut max_height = env_u32("BRIVVA_VIDEO_MAX_HEIGHT", 1080);
    let mut max_fps = env_u32("BRIVVA_VIDEO_MAX_FPS", 30);
    let mut capture_width = max_width;
    let mut capture_height = max_height;
    let mut capture_fps = max_fps;
    let mut subtitle = None;
    let mut youtube_url = std::env::var("YOUTUBE_RTMP_URL")
        .unwrap_or_else(|_| "rtmp://a.rtmp.youtube.com/live2".to_string());
    let mut youtube_key_env = "STREAM_KEY_YOUTUBE".to_string();

    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mp4" => {
                mp4 = Some(value(&args, i)?);
                i += 2;
            }
            "--rtmp" => {
                rtmp.push(RtmpDestination::new("rtmp", value(&args, i)?));
                i += 2;
            }
            "--youtube-url" => {
                youtube_url = value(&args, i)?;
                i += 2;
            }
            "--youtube-key-env" => {
                youtube_key_env = value(&args, i)?;
                i += 2;
            }
            "--youtube" => {
                let key = std::env::var(&youtube_key_env)
                    .map_err(|_| format!("{youtube_key_env} env var missing"))?;
                rtmp.push(RtmpDestination::new(
                    "youtube",
                    format!("{}/{}", youtube_url.trim_end_matches('/'), key),
                ));
                i += 1;
            }
            "--duration" => {
                duration_secs = value(&args, i)?.parse().map_err(|_| "bad --duration")?;
                i += 2;
            }
            "--encoder" => {
                encoder = VideoEncoderKind::from_wire(&value(&args, i)?);
                i += 2;
            }
            "--max-width" => {
                max_width = value(&args, i)?.parse().map_err(|_| "bad --max-width")?;
                i += 2;
            }
            "--max-height" => {
                max_height = value(&args, i)?.parse().map_err(|_| "bad --max-height")?;
                i += 2;
            }
            "--max-fps" => {
                max_fps = value(&args, i)?.parse().map_err(|_| "bad --max-fps")?;
                i += 2;
            }
            "--capture-width" => {
                capture_width = value(&args, i)?
                    .parse()
                    .map_err(|_| "bad --capture-width")?;
                i += 2;
            }
            "--capture-height" => {
                capture_height = value(&args, i)?
                    .parse()
                    .map_err(|_| "bad --capture-height")?;
                i += 2;
            }
            "--capture-fps" => {
                capture_fps = value(&args, i)?.parse().map_err(|_| "bad --capture-fps")?;
                i += 2;
            }
            "--subtitle" => {
                subtitle = Some(value(&args, i)?);
                i += 2;
            }
            "--help" | "-h" => return Err(usage()),
            other => return Err(format!("unknown arg: {other}\n{}", usage())),
        }
    }

    if rtmp.is_empty() {
        return Err("provide --youtube or at least one --rtmp <full-url>".to_string());
    }
    Ok(Args {
        mp4: mp4.ok_or_else(|| "--mp4 <path> required".to_string())?,
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
    })
}

fn usage() -> String {
    "Usage: cargo run -p server-rs --bin mp4_fanout_smoke -- --mp4 file.mp4 (--youtube | --rtmp full-url) [--duration 180] [--encoder x264|nvenc] [--max-width 1920 --max-height 1080 --max-fps 60]".into()
}

fn value(args: &[String], i: usize) -> Result<String, String> {
    args.get(i + 1)
        .cloned()
        .ok_or_else(|| format!("missing value for {}", args[i]))
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
    stdout: tokio::process::ChildStdout,
) {
    let mut reader = BufReader::new(stdout);
    let mut buf = vec![0u8; 1764];
    loop {
        if reader.read_exact(&mut buf).await.is_err() {
            break;
        }
        manager.lock().await.push_host_audio(&buf);
    }
}
