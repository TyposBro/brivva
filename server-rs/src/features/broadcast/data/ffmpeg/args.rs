use std::io::BufRead;
use std::time::Instant;

use crate::features::broadcast::domain::VideoEncoderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoInputCodec {
    H264AnnexB,
    Vp8Ivf,
}

impl VideoInputCodec {
    pub fn ffmpeg_format(self) -> &'static str {
        match self {
            Self::H264AnnexB => "h264",
            Self::Vp8Ivf => "ivf",
        }
    }

    pub fn log_label(self) -> &'static str {
        match self {
            Self::H264AnnexB => "h264_annexb",
            Self::Vp8Ivf => "vp8_ivf",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoProfile {
    pub input_fps: u32,
    pub output_fps: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub bitrate_kbps: u32,
    pub maxrate_kbps: u32,
    pub bufsize_kbps: u32,
    pub keyframe_interval_frames: u32,
    pub pad_to_canvas: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoProfileCaps {
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: u32,
}

impl Default for VideoProfileCaps {
    fn default() -> Self {
        Self {
            max_width: 1920,
            max_height: 1080,
            max_fps: 30,
        }
    }
}

impl VideoProfileCaps {
    const BACKEND_MAX_WIDTH: u32 = 1920;
    const BACKEND_MAX_HEIGHT: u32 = 1080;

    pub fn new(max_width: u32, max_height: u32, max_fps: u32) -> Self {
        Self {
            max_width: max_width.clamp(640, Self::BACKEND_MAX_WIDTH),
            max_height: max_height.clamp(360, Self::BACKEND_MAX_HEIGHT),
            max_fps: max_fps.clamp(15, 120),
        }
    }
}

impl Default for VideoProfile {
    fn default() -> Self {
        Self {
            input_fps: 30,
            output_fps: 30,
            max_width: 1920,
            max_height: 1080,
            bitrate_kbps: 6_000,
            maxrate_kbps: 9_000,
            bufsize_kbps: 18_000,
            keyframe_interval_frames: 60,
            pad_to_canvas: false,
        }
    }
}

impl VideoProfile {
    pub fn from_capture(width: u32, height: u32, fps: u32) -> Self {
        Self::from_capture_with_caps(width, height, fps, VideoProfileCaps::default())
    }

    pub fn from_capture_with_caps(
        width: u32,
        height: u32,
        fps: u32,
        caps: VideoProfileCaps,
    ) -> Self {
        let input_fps = fps.clamp(15, 120);
        // Keep RTMP output at the actual capture tier. Upscaling a 720p
        // browser feed to 1080p made Fargate/libx264 fall below realtime until
        // FFmpeg stopped draining stdin/FIFO. Cap by server/runtime tier so
        // Fargate can stay conservative while GPU nodes can opt into 4K/high
        // fps without changing media code.
        let output_fps = input_fps.min(caps.max_fps);
        let (max_width, max_height) = if is_4k_class(width, height) {
            (3840.min(caps.max_width), 2160.min(caps.max_height))
        } else if width >= 1920 && height >= 1080 {
            (1920.min(caps.max_width), 1080.min(caps.max_height))
        } else {
            (1280.min(caps.max_width), 720.min(caps.max_height))
        };
        let bitrate_kbps = bitrate_for(max_width, max_height, output_fps);
        Self {
            input_fps,
            output_fps,
            max_width,
            max_height,
            bitrate_kbps,
            maxrate_kbps: bitrate_kbps + bitrate_kbps / 2,
            bufsize_kbps: bitrate_kbps * 3,
            keyframe_interval_frames: output_fps * 2,
            pad_to_canvas: false,
        }
    }

    pub fn for_destination_platform(self, _platform: &str) -> Self {
        if !mobile_portrait_output_enabled() {
            return self;
        }
        self.as_mobile_portrait_output()
    }

    fn as_mobile_portrait_output(self) -> Self {
        let output_fps = self.output_fps.min(30);
        // bufsize ≥ 2× bitrate avoids NVENC "limited by bandwidth" warnings.
        // 5000k gives the VBV buffer enough headroom for CBR at 2500k while
        // keeping RTMP latency bounded for live commerce.
        Self {
            input_fps: self.input_fps,
            output_fps,
            max_width: 720,
            max_height: 1280,
            bitrate_kbps: 2_500,
            maxrate_kbps: 2_800,
            bufsize_kbps: 5_000,
            keyframe_interval_frames: output_fps * 2,
            pad_to_canvas: true,
        }
    }
}

fn bitrate_for(width: u32, height: u32, fps: u32) -> u32 {
    match (width, height, fps) {
        (w, h, f) if is_4k_class(w, h) && f > 60 => 35_000,
        (w, h, _) if is_4k_class(w, h) => 24_000,
        (w, h, f) if w >= 1920 && h >= 1080 && f > 60 => 12_000,
        (w, h, _) if w >= 1920 && h >= 1080 => 6_000,
        (_, _, f) if f > 60 => 6_000,
        _ => 3_500,
    }
}

fn is_4k_class(width: u32, height: u32) -> bool {
    width >= 3840 && width.saturating_mul(height) >= 3840 * 1600
}

fn mobile_portrait_output_enabled() -> bool {
    !matches!(
        std::env::var("BRIVVA_RTMP_OUTPUT_LAYOUT")
            .unwrap_or_else(|_| "mobile_portrait".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "source" | "source_aspect" | "landscape" | "desktop"
    )
}

/// Pure builder for the FFmpeg CLI args. Factored out of `spawn_stream_inner`
/// so tests can pin the command shape without spawning an FFmpeg child.
#[cfg(test)]
pub(super) fn build_ffmpeg_args(audio_fifo: &str, rtmp_url: &str) -> Vec<String> {
    build_ffmpeg_args_with_profile(
        audio_fifo,
        None,
        &[rtmp_url.to_string()],
        VideoProfile::default(),
        VideoEncoderKind::X264,
        VideoInputCodec::H264AnnexB,
    )
}

pub(super) fn build_ffmpeg_args_with_profile(
    audio_fifo: &str,
    subtitle_textfile: Option<&str>,
    rtmp_urls: &[String],
    profile: VideoProfile,
    encoder: VideoEncoderKind,
    input_codec: VideoInputCodec,
) -> Vec<String> {
    build_ffmpeg_args_with_profile_and_encoder(
        audio_fifo,
        subtitle_textfile,
        rtmp_urls,
        profile,
        encoder,
        input_codec,
    )
}

pub(super) fn build_ffmpeg_args_with_profile_and_encoder(
    audio_fifo: &str,
    subtitle_textfile: Option<&str>,
    rtmp_urls: &[String],
    profile: VideoProfile,
    encoder: VideoEncoderKind,
    input_codec: VideoInputCodec,
) -> Vec<String> {
    let mut ffmpeg_args: Vec<String> = vec![
        "-y".into(),
        "-loglevel".into(),
        "warning".into(),
        // Emit machine-readable progress on stderr once per second. This gives
        // us fps/speed/drop/dup/bitrate in CloudWatch/local logs without
        // depending on terminal-style carriage-return stats lines.
        "-progress".into(),
        "pipe:2".into(),
        "-stats_period".into(),
        "1".into(),
        "-fflags".into(),
        "+genpts+nobuffer".into(),
        // WebRTC drains can burst a few packets at startup / after silence.
        // FFmpeg's default input queue is only 8 packets, which produced
        // "Thread message queue blocking" warnings and can add RTMP latency.
        "-thread_queue_size".into(),
        "1024".into(),
        "-f".into(),
        input_codec.ffmpeg_format().into(),
        // Raw H.264 is timestampless. Use wall-clock input timestamps instead
        // of a declared input -r: browser H.264 encoders often deliver 15fps
        // even when getSettings() reports 30fps, and -r 30 makes FFmpeg run at
        // ~0.5x realtime. The output fps filter/r below owns RTMP cadence.
        "-use_wallclock_as_timestamps".into(),
        "1".into(),
        "-i".into(),
        "pipe:0".into(),
        "-thread_queue_size".into(),
        "1024".into(),
        "-f".into(),
        "s16le".into(),
        "-ar".into(),
        "44100".into(),
        "-ac".into(),
        "1".into(),
        "-i".into(),
        audio_fifo.to_string(),
    ];
    ffmpeg_args.extend_from_slice(&[
        "-map".into(),
        "0:v".into(),
        "-map".into(),
        "1:a".into(),
        // Own the output video clock for RTMP/YouTube. Copying browser H.264
        // is cheaper, but live validation showed FFmpeg can restart mid-GOP
        // and then receive slices before SPS/PPS ("non-existing PPS"), so
        // YouTube never starts. Re-encode for the launch path; keep the
        // lower-level drain/backpressure fixes and revisit copy once we can
        // guarantee parameter sets/keyframes across restarts.
        "-vf".into(),
        video_filter(profile, subtitle_textfile),
    ]);
    ffmpeg_args.extend_from_slice(&video_encoder_args(encoder, profile));
    ffmpeg_args.extend_from_slice(&[
        "-c:a".into(),
        "aac".into(),
        "-ac:a".into(),
        "2".into(),
        "-b:a".into(),
        "128k".into(),
        // Low-latency FLV/RTMP muxing. These do not fix timestamp bugs, but
        // once video is CFR they keep FFmpeg from intentionally batching.
        "-muxdelay".into(),
        "0".into(),
        "-muxpreload".into(),
        "0".into(),
        "-flush_packets".into(),
        "1".into(),
    ]);
    append_publish_target(&mut ffmpeg_args, rtmp_urls);
    ffmpeg_args
}

fn append_publish_target(ffmpeg_args: &mut Vec<String>, rtmp_urls: &[String]) {
    if rtmp_urls.len() <= 1 {
        if rtmp_urls
            .first()
            .is_some_and(|url| url.starts_with("rtmp://") || url.starts_with("rtmps://"))
        {
            ffmpeg_args.extend_from_slice(&["-rtmp_live".into(), "live".into()]);
        }
        ffmpeg_args.extend_from_slice(&[
            "-flvflags".into(),
            "no_duration_filesize".into(),
            "-f".into(),
            "flv".into(),
            rtmp_urls.first().cloned().unwrap_or_default(),
        ]);
        return;
    }
    ffmpeg_args.extend_from_slice(&[
        "-f".into(),
        "tee".into(),
        rtmp_urls
            .iter()
            .map(|url| {
                format!(
                    "[f=flv:flvflags=no_duration_filesize:onfail=ignore]{}",
                    escape_tee_url(url)
                )
            })
            .collect::<Vec<_>>()
            .join("|"),
    ]);
}

fn escape_tee_url(url: &str) -> String {
    url.replace('\\', "\\\\").replace('|', "\\|")
}

fn video_filter(profile: VideoProfile, subtitle_textfile: Option<&str>) -> String {
    let mut filter = if profile.pad_to_canvas {
        format!(
            "fps={},scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2:black",
            profile.output_fps,
            profile.max_width,
            profile.max_height,
            profile.max_width,
            profile.max_height
        )
    } else {
        format!(
            "fps={},scale='min({},iw)':'min({},ih)':force_original_aspect_ratio=decrease",
            profile.output_fps, profile.max_width, profile.max_height
        )
    };
    if debug_video_clock_enabled() {
        filter.push(',');
        filter.push_str(&debug_video_clock_filter());
    }
    if let Some(textfile) = subtitle_textfile {
        let fontfile = subtitle_fontfile();
        filter.push_str(&format!(
            ",drawtext=textfile={}:fontfile={}:reload=1:x=(w-text_w)/2:y=h-(text_h*3):fontcolor=white:fontsize=44:box=1:boxcolor=black@0.55:boxborderw=18",
            escape_drawtext_value(textfile),
            escape_drawtext_value(&fontfile)
        ));
    }
    filter
}

fn debug_video_clock_enabled() -> bool {
    matches!(
        std::env::var("BRIVVA_DEBUG_VIDEO_CLOCK")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn debug_video_clock_filter() -> String {
    let fontfile = subtitle_fontfile();
    format!(
        "drawtext=text='BRIVVA %{{pts\\:hms}}':fontfile={}:reload=0:x=24:y=24:fontcolor=white:fontsize=34:box=1:boxcolor=black@0.65:boxborderw=12",
        escape_drawtext_value(&fontfile)
    )
}

fn subtitle_fontfile() -> String {
    if let Ok(path) = std::env::var("BRIVVA_SUBTITLE_FONTFILE")
        && !path.trim().is_empty()
    {
        return path;
    }
    for path in [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ] {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".to_string()
}

fn escape_drawtext_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace('\'', "\\'")
        .replace(',', "\\,")
}

fn video_encoder_args(encoder: VideoEncoderKind, profile: VideoProfile) -> Vec<String> {
    let mut args = vec!["-c:v".into()];
    match encoder {
        VideoEncoderKind::X264 => args.extend_from_slice(&[
            "libx264".into(),
            "-preset".into(),
            "ultrafast".into(),
            "-tune".into(),
            "zerolatency".into(),
            "-profile:v".into(),
            "main".into(),
            "-bf".into(),
            "0".into(),
            "-sc_threshold".into(),
            "0".into(),
        ]),
        VideoEncoderKind::Nvenc => args.extend_from_slice(&[
            "h264_nvenc".into(),
            // Live streaming: p1 (fastest) + low-latency tune keeps the T4
            // encoder from buffering behind RTMP backpressure during long
            // YouTube pushes.
            "-preset".into(),
            "p1".into(),
            "-tune".into(),
            "ll".into(),
            "-rc".into(),
            "cbr".into(),
            "-profile:v".into(),
            "high".into(),
            "-bf".into(),
            "0".into(),
        ]),
    }
    args.extend_from_slice(&[
        "-r".into(),
        profile.output_fps.to_string(),
        "-g".into(),
        profile.keyframe_interval_frames.to_string(),
        "-keyint_min".into(),
        profile.keyframe_interval_frames.to_string(),
        "-b:v".into(),
        format!("{}k", profile.bitrate_kbps),
        "-maxrate".into(),
        format!("{}k", profile.maxrate_kbps),
        "-bufsize".into(),
        format!("{}k", profile.bufsize_kbps),
        "-pix_fmt".into(),
        "yuv420p".into(),
    ]);
    args
}

/// Pure drain loop for the ffmpeg-child stderr pipe. Reads `BufRead`
/// line-by-line, invoking `on_line` for each successful line and stopping
/// on EOF (None) or any io error (logged via `tracing::debug!`).
///
/// Factored out of `spawn_stream_inner` so integration tests can pin the
/// behavior (line-fan-out + EOF termination + error tolerance) against a
/// stand-in `BufRead` like a captured `/bin/sh` stderr, without spawning
/// real ffmpeg. The thread in production passes a `|line| tracing::warn!`
/// closure; tests pass one that collects into an `mpsc` so assertions are
/// deterministic.
pub fn drain_stderr_lines<R, F>(reader: R, mut on_line: F)
where
    R: BufRead,
    F: FnMut(String),
{
    for line in reader.lines() {
        match line {
            Ok(l) => on_line(redact_rtmp_secrets(&l)),
            Err(e) => {
                tracing::debug!(error = %e, "ffmpeg stderr reader ended");
                break;
            }
        }
    }
}

pub(super) fn parse_tee_slave_muxer_index(line: &str) -> Option<usize> {
    let marker = "Slave muxer #";
    let start = line.find(marker)? + marker.len();
    let digits = line[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FfmpegProgressSnapshot {
    pub fps: Option<f64>,
    pub speed: Option<f64>,
    pub dup_frames: Option<u64>,
    pub drop_frames: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum FfmpegProgressAlert {
    SlowEncode { speed: f64, consecutive_ticks: u32 },
    DroppedFrames { total: u64, delta: u64 },
}

#[derive(Debug, Default)]
pub(super) struct FfmpegProgressMonitor {
    fps: Option<f64>,
    speed: Option<f64>,
    out_time_us: Option<u64>,
    last_out_time_us: Option<u64>,
    last_progress_at: Option<Instant>,
    output_started: bool,
    dup_frames: Option<u64>,
    drop_frames: Option<u64>,
    last_drop_frames: u64,
    consecutive_slow_ticks: u32,
}

impl FfmpegProgressMonitor {
    const SLOW_SPEED_THRESHOLD: f64 = 0.95;
    const SLOW_SPEED_ALERT_TICKS: u32 = 5;

    pub(super) fn ingest_line(&mut self, line: &str) -> Vec<FfmpegProgressAlert> {
        let Some((key, value)) = line.split_once('=') else {
            return Vec::new();
        };
        match key {
            "fps" => self.fps = parse_progress_f64(value),
            "speed" => self.speed = parse_speed(value),
            "out_time_us" | "out_time_ms" => self.out_time_us = value.parse::<u64>().ok(),
            "dup_frames" => self.dup_frames = value.parse::<u64>().ok(),
            "drop_frames" => self.drop_frames = value.parse::<u64>().ok(),
            "progress" => return self.finish_tick(),
            _ => {}
        }
        Vec::new()
    }

    pub(super) fn snapshot(&self) -> FfmpegProgressSnapshot {
        FfmpegProgressSnapshot {
            fps: self.fps,
            speed: self.speed,
            dup_frames: self.dup_frames,
            drop_frames: self.drop_frames,
        }
    }

    fn finish_tick(&mut self) -> Vec<FfmpegProgressAlert> {
        let mut alerts = Vec::new();
        let effective_speed = self.interval_speed().or(self.speed);
        if let Some(speed) = effective_speed {
            if speed < Self::SLOW_SPEED_THRESHOLD {
                self.consecutive_slow_ticks += 1;
                if self.consecutive_slow_ticks >= Self::SLOW_SPEED_ALERT_TICKS {
                    alerts.push(FfmpegProgressAlert::SlowEncode {
                        speed,
                        consecutive_ticks: self.consecutive_slow_ticks,
                    });
                }
            } else {
                self.consecutive_slow_ticks = 0;
            }
        }
        if let Some(drop_frames) = self.drop_frames
            && drop_frames > self.last_drop_frames
        {
            alerts.push(FfmpegProgressAlert::DroppedFrames {
                total: drop_frames,
                delta: drop_frames - self.last_drop_frames,
            });
            self.last_drop_frames = drop_frames;
        }
        alerts
    }

    fn interval_speed(&mut self) -> Option<f64> {
        let now = Instant::now();
        let out_time_us = self.out_time_us?;
        let last_out_time_us = self.last_out_time_us.replace(out_time_us);
        let last_progress_at = self.last_progress_at.replace(now);
        if out_time_us > 0 {
            self.output_started = true;
        }
        if !self.output_started {
            return None;
        }
        let (Some(last_out_time_us), Some(last_progress_at)) = (last_out_time_us, last_progress_at)
        else {
            return None;
        };
        let elapsed_us = now.duration_since(last_progress_at).as_micros() as f64;
        if elapsed_us <= 0.0 || out_time_us < last_out_time_us {
            return None;
        }
        Some((out_time_us - last_out_time_us) as f64 / elapsed_us)
    }
}

fn parse_speed(value: &str) -> Option<f64> {
    parse_progress_f64(value.trim_end_matches('x'))
}

fn parse_progress_f64(value: &str) -> Option<f64> {
    let value = value.trim();
    if value == "N/A" {
        return None;
    }
    value.parse::<f64>().ok()
}

pub fn redact_rtmp_secrets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(pos) = rest.find("rtmp") {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];

        let Some(scheme_len) = rest
            .strip_prefix("rtmps://")
            .map(|_| "rtmps://".len())
            .or_else(|| rest.strip_prefix("rtmp://").map(|_| "rtmp://".len()))
        else {
            out.push_str("rtmp");
            rest = &rest["rtmp".len()..];
            continue;
        };

        let url_end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | ')' | ']' | '}'))
            .unwrap_or(rest.len());
        let url = &rest[..url_end];
        let redacted = match url.rfind('/') {
            Some(last_slash) if last_slash + 1 < url.len() && last_slash >= scheme_len => {
                format!("{}<redacted>", &url[..=last_slash])
            }
            _ => url.to_string(),
        };

        out.push_str(&redacted);
        rest = &rest[url_end..];
    }

    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ffmpeg_args_reencodes_webrtc_h264_with_stable_clock() {
        let args = build_ffmpeg_args("/tmp/fifo_src", "rtmp://x/y");
        let joined = args.join(" ");
        assert!(joined.ends_with("rtmp://x/y"));
        assert!(joined.contains("-progress pipe:2 -stats_period 1"));
        assert!(joined.contains("-f h264"));
        assert!(joined.contains("-use_wallclock_as_timestamps 1 -i pipe:0"));
        assert!(joined.contains(
            "-vf fps=30,scale='min(1920,iw)':'min(1080,ih)':force_original_aspect_ratio=decrease"
        ));
        assert!(joined.contains("-c:v libx264"));
        assert!(joined.contains("-preset ultrafast"));
        assert!(joined.contains("-tune zerolatency"));
        assert!(joined.contains("-bf 0"));
        assert!(joined.contains("-b:v 6000k -maxrate 9000k -bufsize 18000k"));
        assert!(joined.contains("-g 60"));
        assert!(!joined.contains("-c:v copy"));
        assert!(!joined.contains("image2pipe"));
        assert!(!joined.contains("mjpeg"));
        assert!(!joined.contains("udp"));
        assert!(!joined.contains("rtp"));
    }

    #[test]
    fn build_ffmpeg_args_always_passes_f_flv_for_rtmp_family_publish() {
        let args = build_ffmpeg_args("/tmp/fifo", "rtmp://localhost/live");
        let idx = args.iter().rposition(|s| s == "-f").expect("-f present");
        assert_eq!(args[idx + 1], "flv");
    }

    #[test]
    fn build_ffmpeg_args_passes_44100_mono_s16le_for_audio_fifo_input() {
        let args = build_ffmpeg_args("/tmp/fifo", "rtmp://x");
        let ar_idx = args.iter().position(|s| s == "-ar").unwrap();
        assert_eq!(args[ar_idx + 1], "44100");
        let ac_idx = args.iter().position(|s| s == "-ac").unwrap();
        assert_eq!(args[ac_idx + 1], "1");
    }

    #[test]
    fn build_ffmpeg_args_raises_input_thread_queues() {
        let args = build_ffmpeg_args("/tmp/fifo", "rtmp://x");
        let queue_flags = args
            .iter()
            .filter(|s| s.as_str() == "-thread_queue_size")
            .count();
        assert_eq!(queue_flags, 2, "one queue per input");
        for (i, arg) in args.iter().enumerate() {
            if arg == "-thread_queue_size" {
                assert_eq!(args[i + 1], "1024");
            }
        }
    }

    #[test]
    fn build_ffmpeg_args_keeps_audio_encoding_for_rtmp() {
        let args = build_ffmpeg_args("/tmp/fifo", "rtmp://x");
        let b_a_idx = args.iter().position(|s| s == "-b:a").unwrap();
        assert_eq!(args[b_a_idx + 1], "128k");
    }

    #[test]
    fn build_ffmpeg_args_uses_low_latency_flv_muxing() {
        let args = build_ffmpeg_args("/tmp/fifo", "rtmp://x");
        let joined = args.join(" ");
        assert!(joined.contains("-muxdelay 0"));
        assert!(joined.contains("-muxpreload 0"));
        assert!(joined.contains("-flush_packets 1"));
        assert!(joined.contains("-rtmp_live live"));
        assert!(joined.contains("-flvflags no_duration_filesize"));
    }

    #[test]
    fn video_profile_keeps_launch_rtmp_output_at_1080p30() {
        let profile = VideoProfile::from_capture(3840, 2160, 60);
        assert_eq!(profile.input_fps, 60);
        assert_eq!(profile.output_fps, 30);
        assert_eq!(profile.max_width, 1920);
        assert_eq!(profile.max_height, 1080);
        assert_eq!(profile.bitrate_kbps, 6_000);
    }

    #[test]
    fn video_profile_uses_720p30_for_720p_capture() {
        let profile = VideoProfile::from_capture(1280, 720, 15);
        assert_eq!(profile.input_fps, 15);
        assert_eq!(profile.output_fps, 15);
        assert_eq!(profile.max_width, 1280);
        assert_eq!(profile.max_height, 720);
        assert_eq!(profile.bitrate_kbps, 3_500);
    }

    #[test]
    fn video_profile_caps_4k_capture_to_1080p_backend_ceiling() {
        let profile = VideoProfile::from_capture_with_caps(
            3840,
            2160,
            120,
            VideoProfileCaps::new(3840, 2160, 120),
        );
        assert_eq!(profile.input_fps, 120);
        assert_eq!(profile.output_fps, 120);
        assert_eq!(profile.max_width, 1920);
        assert_eq!(profile.max_height, 1080);
        assert_eq!(profile.bitrate_kbps, 12_000);
    }

    #[test]
    fn video_profile_caps_wide_4k_capture_to_1080p_backend_ceiling() {
        let profile = VideoProfile::from_capture_with_caps(
            3840,
            1920,
            30,
            VideoProfileCaps::new(3840, 2160, 30),
        );
        assert_eq!(profile.input_fps, 30);
        assert_eq!(profile.output_fps, 30);
        assert_eq!(profile.max_width, 1920);
        assert_eq!(profile.max_height, 1080);
        assert_eq!(profile.bitrate_kbps, 6_000);
    }

    #[test]
    fn video_profile_caps_high_capture_to_runtime_tier() {
        let profile = VideoProfile::from_capture_with_caps(
            3840,
            2160,
            120,
            VideoProfileCaps::new(1920, 1080, 60),
        );
        assert_eq!(profile.input_fps, 120);
        assert_eq!(profile.output_fps, 60);
        assert_eq!(profile.max_width, 1920);
        assert_eq!(profile.max_height, 1080);
        assert_eq!(profile.bitrate_kbps, 6_000);
    }

    #[test]
    fn video_profile_for_rtmp_platforms_defaults_to_mobile_portrait() {
        let profile = VideoProfile::from_capture_with_caps(
            1920,
            1080,
            60,
            VideoProfileCaps::new(1920, 1080, 60),
        )
        .for_destination_platform("youtube-ja");

        assert_eq!(profile.input_fps, 60);
        assert_eq!(profile.output_fps, 30);
        assert_eq!(profile.max_width, 720);
        assert_eq!(profile.max_height, 1280);
        assert_eq!(profile.bitrate_kbps, 2_500);
        assert_eq!(profile.maxrate_kbps, 2_800);
        assert_eq!(profile.keyframe_interval_frames, 60);
        assert!(profile.pad_to_canvas);
    }

    #[test]
    fn build_ffmpeg_args_for_mobile_rtmp_pads_to_portrait_canvas() {
        let profile = VideoProfile::default().for_destination_platform("youtube-ja");
        let args = build_ffmpeg_args_with_profile(
            "/tmp/fifo",
            None,
            &["rtmp://youtube/live/key".to_string()],
            profile,
            VideoEncoderKind::Nvenc,
            VideoInputCodec::H264AnnexB,
        );
        let joined = args.join(" ");

        assert!(joined.contains("-vf fps=30,scale=720:1280:force_original_aspect_ratio=decrease,pad=720:1280:(ow-iw)/2:(oh-ih)/2:black"));
        assert!(joined.contains("-g 60 -keyint_min 60"));
        assert!(joined.contains("-b:v 2500k -maxrate 2800k -bufsize 5000k"));
    }

    #[test]
    fn build_ffmpeg_args_can_accept_vp8_ivf_input() {
        let args = build_ffmpeg_args_with_profile(
            "/tmp/fifo",
            None,
            &["rtmp://x/y".to_string()],
            VideoProfile::default(),
            VideoEncoderKind::X264,
            VideoInputCodec::Vp8Ivf,
        );
        let joined = args.join(" ");
        assert!(joined.contains("-f ivf"));
        assert!(joined.contains("-use_wallclock_as_timestamps 1 -i pipe:0"));
        assert!(joined.contains("-c:v libx264"));
    }

    #[test]
    fn build_ffmpeg_args_uses_wallclock_input_for_reencode_profile() {
        let args = build_ffmpeg_args_with_profile(
            "/tmp/fifo",
            None,
            &["rtmp://x/y".to_string()],
            VideoProfile::from_capture(3840, 2160, 60),
            VideoEncoderKind::X264,
            VideoInputCodec::H264AnnexB,
        );
        let joined = args.join(" ");
        assert!(joined.contains("-use_wallclock_as_timestamps 1 -i pipe:0"));
        assert!(joined.contains(
            "-vf fps=30,scale='min(1920,iw)':'min(1080,ih)':force_original_aspect_ratio=decrease"
        ));
        assert!(joined.contains("-c:v libx264"));
        assert!(joined.contains("-b:v 6000k -maxrate 9000k -bufsize 18000k"));
        assert!(!joined.contains("-c:v copy"));
    }

    #[test]
    fn build_ffmpeg_args_can_use_nvenc_for_gpu_launch_path() {
        let args = build_ffmpeg_args_with_profile_and_encoder(
            "/tmp/fifo",
            None,
            &["rtmp://x/y".to_string()],
            VideoProfile::default(),
            VideoEncoderKind::Nvenc,
            VideoInputCodec::H264AnnexB,
        );
        let joined = args.join(" ");
        assert!(joined.contains("-c:v h264_nvenc"));
        assert!(joined.contains("-preset p1"));
        assert!(joined.contains("-tune ll"));
        assert!(joined.contains("-rc cbr"));
        assert!(joined.contains("-profile:v high"));
        assert!(joined.contains("-b:v 6000k -maxrate 9000k -bufsize 18000k"));
        assert!(!joined.contains("-c:v libx264"));
    }

    #[test]
    fn build_ffmpeg_args_can_burn_subtitles_from_reloadable_textfile() {
        let args = build_ffmpeg_args_with_profile(
            "/tmp/fifo",
            Some("/tmp/brivva_subtitle_stream-1.txt"),
            &["rtmp://x/y".to_string()],
            VideoProfile::default(),
            VideoEncoderKind::X264,
            VideoInputCodec::H264AnnexB,
        );
        let joined = args.join(" ");
        assert!(joined.contains("drawtext=textfile=/tmp/brivva_subtitle_stream-1.txt"));
        assert!(joined.contains(":fontfile="));
        assert!(joined.contains(":reload=1"));
        assert!(joined.contains("box=1"));
    }

    #[test]
    fn build_ffmpeg_args_can_publish_one_encode_to_multiple_rtmp_urls() {
        let args = build_ffmpeg_args_with_profile(
            "/tmp/fifo",
            None,
            &[
                "rtmp://a/live/key-a".to_string(),
                "rtmp://b/live/key-b".to_string(),
            ],
            VideoProfile::default(),
            VideoEncoderKind::X264,
            VideoInputCodec::H264AnnexB,
        );
        let joined = args.join(" ");
        assert!(joined.contains("-f tee"));
        assert!(
            joined
                .contains("[f=flv:flvflags=no_duration_filesize:onfail=ignore]rtmp://a/live/key-a")
        );
        assert!(
            joined.contains(
                "|[f=flv:flvflags=no_duration_filesize:onfail=ignore]rtmp://b/live/key-b"
            )
        );
        assert!(!joined.contains("-f flv rtmp://a/live/key-a"));
    }

    #[test]
    fn parse_tee_slave_muxer_index_extracts_failed_destination_index() {
        assert_eq!(
            parse_tee_slave_muxer_index("[tee @ 0x1] Slave muxer #1 failed: Broken pipe"),
            Some(1)
        );
        assert_eq!(parse_tee_slave_muxer_index("ordinary ffmpeg stderr"), None);
    }

    #[test]
    fn ffmpeg_progress_monitor_alerts_after_sustained_slow_speed() {
        let mut monitor = FfmpegProgressMonitor::default();
        let mut alerts = Vec::new();
        for _ in 0..4 {
            monitor.ingest_line("speed=0.83x");
            alerts.extend(monitor.ingest_line("progress=continue"));
        }
        assert!(alerts.is_empty());

        monitor.ingest_line("speed=0.84x");
        alerts.extend(monitor.ingest_line("progress=continue"));

        assert_eq!(
            alerts,
            vec![FfmpegProgressAlert::SlowEncode {
                speed: 0.84,
                consecutive_ticks: 5,
            }]
        );
    }

    #[test]
    fn ffmpeg_progress_monitor_uses_interval_speed_over_cumulative_startup_speed() {
        let mut monitor = FfmpegProgressMonitor::default();
        let mut alerts = Vec::new();
        monitor.ingest_line("speed=0.10x");
        monitor.ingest_line("out_time_us=1000000");
        alerts.extend(monitor.ingest_line("progress=continue"));
        for i in 2..10 {
            monitor.last_progress_at = Some(Instant::now() - std::time::Duration::from_secs(1));
            monitor.ingest_line("speed=0.20x");
            monitor.ingest_line(&format!("out_time_us={}", i * 1_000_000));
            alerts.extend(monitor.ingest_line("progress=continue"));
        }

        assert!(
            alerts.is_empty(),
            "cumulative ffmpeg speed includes no-input warmup; interval speed should stay healthy"
        );
    }

    #[test]
    fn ffmpeg_progress_monitor_resets_slow_speed_after_recovery() {
        let mut monitor = FfmpegProgressMonitor::default();
        for _ in 0..4 {
            monitor.ingest_line("speed=0.80x");
            monitor.ingest_line("progress=continue");
        }
        monitor.ingest_line("speed=1.01x");
        assert!(monitor.ingest_line("progress=continue").is_empty());

        monitor.ingest_line("speed=0.80x");
        assert!(monitor.ingest_line("progress=continue").is_empty());
    }

    #[test]
    fn ffmpeg_progress_monitor_alerts_when_drop_frames_increase() {
        let mut monitor = FfmpegProgressMonitor::default();
        monitor.ingest_line("drop_frames=2");
        assert_eq!(
            monitor.ingest_line("progress=continue"),
            vec![FfmpegProgressAlert::DroppedFrames { total: 2, delta: 2 }]
        );
        monitor.ingest_line("drop_frames=5");
        assert_eq!(
            monitor.ingest_line("progress=continue"),
            vec![FfmpegProgressAlert::DroppedFrames { total: 5, delta: 3 }]
        );
    }

    #[test]
    fn redact_rtmp_secrets_hides_last_path_segment() {
        let line = "Output #0 to 'rtmps://a.rtmps.youtube.com/live2/secret-key-123':";
        let redacted = redact_rtmp_secrets(line);

        assert_eq!(
            redacted,
            "Output #0 to 'rtmps://a.rtmps.youtube.com/live2/<redacted>':"
        );
        assert!(!redacted.contains("secret-key-123"));
    }

    #[test]
    fn redact_rtmp_secrets_handles_multiple_urls() {
        let line = "rtmp://one/app/key1 -> rtmps://two/live/key2";

        assert_eq!(
            redact_rtmp_secrets(line),
            "rtmp://one/app/<redacted> -> rtmps://two/live/<redacted>"
        );
    }
}
