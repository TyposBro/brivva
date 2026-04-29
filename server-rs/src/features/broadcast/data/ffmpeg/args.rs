use std::io::BufRead;

/// Pure builder for the FFmpeg CLI args. Factored out of `spawn_stream_inner`
/// so tests can pin the command shape without spawning an FFmpeg child.
pub(super) fn build_ffmpeg_args(audio_fifo: &str, rtmp_url: &str) -> Vec<String> {
    let input_fps = std::env::var("BRIVVA_H264_INPUT_FPS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|fps| (1..=60).contains(fps))
        .unwrap_or(30);
    build_ffmpeg_args_with_input_fps(audio_fifo, rtmp_url, input_fps)
}

fn build_ffmpeg_args_with_input_fps(audio_fifo: &str, rtmp_url: &str, input_fps: u32) -> Vec<String> {
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
        // Default to 30fps for the native camera WebRTC path. Override with
        // BRIVVA_H264_INPUT_FPS=15 only for browsers/devices proven to send
        // 15fps.
        "-r".into(),
        input_fps.to_string(),
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
        // Own the output video clock. Raw WebRTC H.264 arrives as an
        // elementary stream with jittery/guessed timestamps; copying it into
        // FLV made YouTube buffer and FFmpeg warn "Timestamps are unset".
        // Keep local demo output bounded: unconstrained 4K x264 can fall
        // behind real time, which makes FFmpeg's audio input queue grow until
        // YouTube buffers. Production should replace this with one shared
        // hardware/GPU encode, but the live path must be realtime now.
        "-vf".into(),
        "fps=30,scale='min(1920,iw)':'min(1080,ih)':force_original_aspect_ratio=decrease".into(),
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
        "30".into(),
        "-g".into(),
        "60".into(),
        "-keyint_min".into(),
        "60".into(),
        "-sc_threshold".into(),
        "0".into(),
        "-b:v".into(),
        "3500k".into(),
        "-maxrate".into(),
        "4500k".into(),
        "-bufsize".into(),
        "9000k".into(),
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
            Ok(l) => on_line(l),
            Err(e) => {
                tracing::debug!(error = %e, "ffmpeg stderr reader ended");
                break;
            }
        }
    }
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
        assert!(joined.contains("-vf fps=30,scale='min(1920,iw)':'min(1080,ih)':force_original_aspect_ratio=decrease"));
        assert!(joined.contains("-c:v libx264"));
        assert!(joined.contains("-preset ultrafast"));
        assert!(joined.contains("-tune zerolatency"));
        assert!(joined.contains("-bf 0"));
        assert!(joined.contains("-b:v 3500k -maxrate 4500k -bufsize 9000k"));
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
        let queue_flags = args.iter().filter(|s| s.as_str() == "-thread_queue_size").count();
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
}
