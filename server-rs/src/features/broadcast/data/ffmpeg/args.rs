use std::io::BufRead;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoProfile {
    pub input_fps: u32,
    pub output_fps: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub bitrate_kbps: u32,
    pub maxrate_kbps: u32,
    pub bufsize_kbps: u32,
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
        }
    }
}

impl VideoProfile {
    pub fn from_capture(width: u32, height: u32, fps: u32) -> Self {
        let input_fps = fps.clamp(30, 60);
        // Keep RTMP output at the actual capture tier. Upscaling a 720p
        // browser feed to 1080p made Fargate/libx264 fall below realtime until
        // FFmpeg stopped draining stdin/FIFO and the idle watchdog restarted
        // the stream. FFmpeg still owns the CFR clock via fps=30; it should
        // duplicate cadence, not manufacture extra pixels.
        let output_fps = 30;
        let (max_width, max_height, bitrate_kbps) = if width >= 1920 && height >= 1080 {
            (1920, 1080, 6_000)
        } else {
            (1280, 720, 3_500)
        };
        Self {
            input_fps,
            output_fps,
            max_width,
            max_height,
            bitrate_kbps,
            maxrate_kbps: bitrate_kbps + bitrate_kbps / 2,
            bufsize_kbps: bitrate_kbps * 3,
        }
    }
}

/// Pure builder for the FFmpeg CLI args. Factored out of `spawn_stream_inner`
/// so tests can pin the command shape without spawning an FFmpeg child.
#[cfg(test)]
pub(super) fn build_ffmpeg_args(audio_fifo: &str, rtmp_url: &str) -> Vec<String> {
    build_ffmpeg_args_with_profile(audio_fifo, rtmp_url, VideoProfile::default())
}

pub(super) fn build_ffmpeg_args_with_profile(
    audio_fifo: &str,
    rtmp_url: &str,
    profile: VideoProfile,
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
        "h264".into(),
        // Raw H.264 is timestampless, so FFmpeg needs the expected input rate.
        // The browser sends its actual capture profile before WebRTC starts;
        // 1080p30 is the floor, and 4K60 is preserved when the host can
        // hardware-encode it.
        "-r".into(),
        profile.input_fps.to_string(),
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
        format!(
            "fps={},scale='min({},iw)':'min({},ih)':force_original_aspect_ratio=decrease",
            profile.output_fps, profile.max_width, profile.max_height
        ),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "ultrafast".into(),
        "-tune".into(),
        "zerolatency".into(),
        "-profile:v".into(),
        "main".into(),
        "-bf".into(),
        "0".into(),
        "-r".into(),
        profile.output_fps.to_string(),
        "-g".into(),
        (profile.output_fps * 2).to_string(),
        "-keyint_min".into(),
        (profile.output_fps * 2).to_string(),
        "-sc_threshold".into(),
        "0".into(),
        "-b:v".into(),
        format!("{}k", profile.bitrate_kbps),
        "-maxrate".into(),
        format!("{}k", profile.maxrate_kbps),
        "-bufsize".into(),
        format!("{}k", profile.bufsize_kbps),
        "-pix_fmt".into(),
        "yuv420p".into(),
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
        "-f".into(),
        "flv".into(),
        rtmp_url.to_string(),
    ]);
    ffmpeg_args
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
        if let Some(speed) = self.speed {
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
        assert!(joined.contains("-r 30 -i pipe:0"));
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
        assert_eq!(profile.input_fps, 30);
        assert_eq!(profile.output_fps, 30);
        assert_eq!(profile.max_width, 1280);
        assert_eq!(profile.max_height, 720);
        assert_eq!(profile.bitrate_kbps, 3_500);
    }

    #[test]
    fn build_ffmpeg_args_uses_capture_clock_for_reencode_profile() {
        let args = build_ffmpeg_args_with_profile(
            "/tmp/fifo",
            "rtmp://x/y",
            VideoProfile::from_capture(3840, 2160, 60),
        );
        let joined = args.join(" ");
        assert!(joined.contains("-r 60 -i pipe:0"));
        assert!(joined.contains(
            "-vf fps=30,scale='min(1920,iw)':'min(1080,ih)':force_original_aspect_ratio=decrease"
        ));
        assert!(joined.contains("-c:v libx264"));
        assert!(joined.contains("-b:v 6000k -maxrate 9000k -bufsize 18000k"));
        assert!(!joined.contains("-c:v copy"));
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
