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
    outputs: Vec<SmokeOutput>,
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
    soniox_api_key: String,
    soniox_ws_url: String,
    elevenlabs_api_key: String,
    elevenlabs_base_url: String,
}

#[derive(Debug, Clone)]
struct SmokeOutput {
    label: String,
    lang: Option<Lang>,
    destinations: Vec<RtmpDestination>,
}

impl SmokeOutput {
    fn is_passthrough(&self, source_lang: &Lang) -> bool {
        self.lang.as_ref().is_none_or(|lang| lang == source_lang)
    }

    fn output_lang(&self) -> String {
        self.lang
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "pass".to_string())
    }
}

#[tokio::test]
#[ignore = "manual RTMP e2e; requires MP4_FANOUT_SMOKE_MP4 and live destination secrets"]
async fn mp4_fanout_smoke() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let args = parse_args()?;
    log_smoke_args(&args);

    let mut manager = RtmpManager::new();
    manager.set_video_encoder(args.encoder);
    manager.set_video_profile_caps(VideoProfileCaps::new(
        args.max_width,
        args.max_height,
        args.max_fps,
    ));
    manager.set_capture_video_profile(args.capture_width, args.capture_height, args.capture_fps);
    for output in &args.outputs {
        let output_lang = output.output_lang();
        let is_passthrough = output.is_passthrough(&args.source_lang);
        manager.start_stream_group(StartStreamGroupArgs {
            stream_id: &format!("mp4-fanout-smoke-{}", output.label),
            lang: &output_lang,
            rtmp_urls: output
                .destinations
                .iter()
                .map(|dest| dest.url.clone())
                .collect(),
            destinations: output.destinations.clone(),
            delay_ms: 0,
            is_source: is_passthrough,
            host_gain: if is_passthrough { 1.0 } else { 0.2 },
            output_id: None,
            destination_platform: &output.label,
            output_controls_enabled: true,
            render_graph_node: None,
            passthrough: is_passthrough,
        })?;
        if let Some(subtitle) = &args.subtitle
            && !is_passthrough
        {
            manager.push_subtitle(&output_lang, subtitle);
        }
    }

    let manager: SharedRtmpManager = Arc::new(tokio::sync::Mutex::new(manager));
    let target_langs = target_langs_for_translation(&args.outputs, &args.source_lang);
    let stt_tx = if target_langs.is_empty() {
        None
    } else {
        Some(spawn_translation_pipeline(
            &args,
            manager.clone(),
            target_langs,
        ))
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

fn log_smoke_args(args: &Args) {
    let outputs: Vec<_> = args
        .outputs
        .iter()
        .map(|output| {
            serde_json::json!({
                "label": output.label,
                "lang": output.lang.as_ref().map(ToString::to_string),
                "destination_platforms": output
                    .destinations
                    .iter()
                    .map(|dest| dest.platform.clone())
                    .collect::<Vec<_>>(),
                "destination_count": output.destinations.len(),
            })
        })
        .collect();
    tracing::info!(
        mp4 = %args.mp4,
        outputs = %serde_json::Value::Array(outputs),
        duration_secs = args.duration_secs,
        video_encoder = %args.encoder.codec_name(),
        max_width = args.max_width,
        max_height = args.max_height,
        max_fps = args.max_fps,
        capture_width = args.capture_width,
        capture_height = args.capture_height,
        capture_fps = args.capture_fps,
        subtitle_enabled = args.subtitle.is_some(),
        source_lang = %args.source_lang,
        soniox_api_key_present = !args.soniox_api_key.trim().is_empty(),
        soniox_ws_url = %args.soniox_ws_url,
        elevenlabs_api_key_present = !args.elevenlabs_api_key.trim().is_empty(),
        elevenlabs_base_url = %args.elevenlabs_base_url,
        "mp4 fanout smoke starting"
    );
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
    let duration_secs = env_u32("MP4_FANOUT_SMOKE_DURATION", 180) as u64;
    let encoder = VideoEncoderKind::from_wire(
        &std::env::var("MP4_FANOUT_SMOKE_ENCODER").unwrap_or_else(|_| "x264".to_string()),
    );
    let max_width = env_u32("BRIVVA_VIDEO_MAX_WIDTH", 1920);
    let max_height = env_u32("BRIVVA_VIDEO_MAX_HEIGHT", 1080);
    let max_fps = env_u32("BRIVVA_VIDEO_MAX_FPS", 30);
    let detected = probe_mp4_video(&mp4)
        .ok_or_else(|| format!("MP4_FANOUT_SMOKE_MP4 is not a readable video file: {mp4}"))?;
    if !detected.codec.eq_ignore_ascii_case("h264") {
        return Err(format!(
            "MP4_FANOUT_SMOKE_MP4 must be H.264 for this smoke test, got {}. Convert fixture once with ffmpeg before running.",
            detected.codec
        ));
    }
    let capture_width = env_u32("MP4_FANOUT_SMOKE_CAPTURE_WIDTH", detected.width);
    let capture_height = env_u32("MP4_FANOUT_SMOKE_CAPTURE_HEIGHT", detected.height);
    let capture_fps = env_u32("MP4_FANOUT_SMOKE_CAPTURE_FPS", detected.fps);
    let subtitle = std::env::var("MP4_FANOUT_SMOKE_SUBTITLE").ok();
    let source_lang = parse_lang_env("MP4_FANOUT_SMOKE_SOURCE_LANG", Lang::Ko)?;
    let explicit_target_langs = parse_target_langs_env("MP4_FANOUT_SMOKE_TARGET_LANGS")?;
    let legacy_target_lang = parse_optional_lang_env("MP4_FANOUT_SMOKE_TARGET_LANG")?;
    let soniox_api_key = std::env::var("SONIOX_API_KEY").unwrap_or_default();
    let soniox_ws_url = std::env::var("SONIOX_WS_URL")
        .unwrap_or_else(|_| "wss://stt-rt.soniox.com/transcribe-websocket".to_string());
    let elevenlabs_api_key = std::env::var("ELEVENLABS_API_KEY").unwrap_or_default();
    let elevenlabs_base_url = std::env::var("ELEVENLABS_BASE_URL")
        .unwrap_or_else(|_| "https://api.elevenlabs.io".to_string());
    let youtube_url = std::env::var("YOUTUBE_RTMP_URL")
        .unwrap_or_else(|_| "rtmp://a.rtmp.youtube.com/live2".to_string());
    let output_filter = parse_output_filter_env("MP4_FANOUT_SMOKE_OUTPUTS")?;
    let mut outputs = youtube_outputs(
        &youtube_url,
        &source_lang,
        &explicit_target_langs,
        output_filter.as_deref(),
    );
    let mut legacy_rtmp = Vec::new();
    if output_allowed(output_filter.as_deref(), "legacy") {
        if let Ok(key) = std::env::var("STREAM_KEY_YOUTUBE") {
            if !key.trim().is_empty() {
                legacy_rtmp.push(youtube_destination("youtube", &youtube_url, &key));
            }
        }
    }
    if output_allowed(output_filter.as_deref(), "legacy") {
        for (idx, url) in std::env::var("MP4_FANOUT_SMOKE_RTMP_URLS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .enumerate()
        {
            legacy_rtmp.push(RtmpDestination::new(format!("rtmp{idx}"), url.to_string()));
        }
    }

    if !legacy_rtmp.is_empty() {
        outputs.push(SmokeOutput {
            label: "legacy".to_string(),
            lang: legacy_target_lang,
            destinations: legacy_rtmp,
        });
    }

    if outputs.is_empty() {
        return Err(
            "provide STREAM_KEY_YOUTUBE, STREAM_KEY_YOUTUBE_{PASS,KO,EN,JA,ZH}, or MP4_FANOUT_SMOKE_RTMP_URLS"
                .to_string(),
        );
    }
    Ok(Args {
        mp4,
        outputs,
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
        soniox_api_key,
        soniox_ws_url,
        elevenlabs_api_key,
        elevenlabs_base_url,
    })
}

fn youtube_outputs(
    youtube_url: &str,
    source_lang: &Lang,
    explicit_target_langs: &[Lang],
    output_filter: Option<&[String]>,
) -> Vec<SmokeOutput> {
    let mut outputs = Vec::new();
    if output_allowed(output_filter, "pass") {
        if let Ok(key) = std::env::var("STREAM_KEY_YOUTUBE_PASS") {
            if !key.trim().is_empty() {
                outputs.push(SmokeOutput {
                    label: "youtube-pass".to_string(),
                    lang: None,
                    destinations: vec![youtube_destination("youtube-pass", youtube_url, &key)],
                });
            }
        }
    }
    for lang in [Lang::Ko, Lang::En, Lang::Ja, Lang::Zh] {
        let lang_label = lang.to_string();
        if !output_allowed(output_filter, &lang_label) {
            continue;
        }
        let key = format!(
            "STREAM_KEY_YOUTUBE_{}",
            lang.to_string().to_ascii_uppercase()
        );
        let should_include = std::env::var(&key).is_ok()
            || explicit_target_langs.contains(&lang)
            || (&lang == source_lang && explicit_target_langs.is_empty());
        if !should_include {
            continue;
        }
        let Ok(stream_key) = std::env::var(&key) else {
            continue;
        };
        if stream_key.trim().is_empty() {
            continue;
        }
        let label = format!("youtube-{}", lang);
        outputs.push(SmokeOutput {
            label: label.clone(),
            lang: Some(lang),
            destinations: vec![youtube_destination(&label, youtube_url, &stream_key)],
        });
    }
    outputs
}

fn parse_output_filter_env(name: &str) -> Result<Option<Vec<String>>, String> {
    let Ok(raw) = std::env::var(name) else {
        return Ok(None);
    };
    let mut outputs = Vec::new();
    for value in raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let normalized = value.to_ascii_lowercase();
        match normalized.as_str() {
            "pass" | "ko" | "en" | "ja" | "zh" | "legacy" => outputs.push(normalized),
            _ => {
                return Err(format!(
                    "{name} contains unsupported output '{value}', expected pass,ko,en,ja,zh,legacy"
                ));
            }
        }
    }
    Ok(Some(outputs))
}

fn output_allowed(filter: Option<&[String]>, output: &str) -> bool {
    match filter {
        Some(outputs) => outputs.iter().any(|candidate| candidate == output),
        None => true,
    }
}

fn youtube_destination(platform: &str, youtube_url: &str, key: &str) -> RtmpDestination {
    RtmpDestination::new(
        platform,
        format!("{}/{}", youtube_url.trim_end_matches('/'), key),
    )
}

fn target_langs_for_translation(outputs: &[SmokeOutput], source_lang: &Lang) -> Vec<Lang> {
    let mut langs = Vec::new();
    for output in outputs {
        let Some(lang) = &output.lang else {
            continue;
        };
        if lang == source_lang || langs.contains(lang) {
            continue;
        }
        langs.push(lang.clone());
    }
    langs
}

#[derive(Debug, Clone)]
struct ProbedVideo {
    codec: String,
    width: u32,
    height: u32,
    fps: u32,
}

fn probe_mp4_video(mp4: &str) -> Option<ProbedVideo> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,avg_frame_rate",
            "-of",
            "csv=p=0",
            mp4,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_ffprobe_video(&String::from_utf8_lossy(&output.stdout))
}

fn parse_ffprobe_video(output: &str) -> Option<ProbedVideo> {
    let line = output.lines().next()?.trim();
    let mut parts = line.split(',');
    let codec = parts.next()?.to_string();
    let width = parts.next()?.parse().ok()?;
    let height = parts.next()?.parse().ok()?;
    let fps = parse_frame_rate(parts.next()?)?.clamp(15, 120);
    Some(ProbedVideo {
        codec,
        width,
        height,
        fps,
    })
}

fn parse_frame_rate(value: &str) -> Option<u32> {
    let value = value.trim();
    let Some((num, den)) = value.split_once('/') else {
        return value.parse().ok();
    };
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    if den == 0.0 {
        return None;
    }
    Some((num / den).round() as u32)
}

fn spawn_translation_pipeline(
    args: &Args,
    manager: SharedRtmpManager,
    target_langs: Vec<Lang>,
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
        target_langs,
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

fn parse_target_langs_env(key: &str) -> Result<Vec<Lang>, String> {
    let Ok(value) = std::env::var(key) else {
        return Ok(Vec::new());
    };
    let mut langs = Vec::new();
    for part in value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        if part.eq_ignore_ascii_case("pass") {
            continue;
        }
        let lang = Lang::from_str(part).ok_or_else(|| format!("{key} must contain en,ko,ja,zh"))?;
        if !langs.contains(&lang) {
            langs.push(lang);
        }
    }
    Ok(langs)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn spawn_video_ffmpeg(mp4: &str) -> std::io::Result<tokio::process::Child> {
    let mut command = Command::new("ffmpeg");
    command.args([
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
        "copy",
        "-bsf:v",
        "h264_mp4toannexb",
    ]);
    command
        .args(["-f", "h264", "pipe:1"])
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
