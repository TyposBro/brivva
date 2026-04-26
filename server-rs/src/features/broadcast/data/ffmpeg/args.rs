use std::io::BufRead;

/// Pure builder for the FFmpeg CLI args. Factored out of `spawn_stream_inner`
/// so tests can pin the command shape without spawning an FFmpeg child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoInputMode {
    Mjpeg,
    H264AnnexB,
}

pub(super) fn build_ffmpeg_args(
    audio_fifo: &str,
    rtmp_url: &str,
    video_input_mode: VideoInputMode,
) -> Vec<String> {
    let mut ffmpeg_args: Vec<String> = match video_input_mode {
        VideoInputMode::Mjpeg => vec![
            "-y".into(),
            "-loglevel".into(),
            "warning".into(),
            "-f".into(),
            "image2pipe".into(),
            // Force mjpeg on stdin so ffmpeg does not block on codec auto-detection
            // before the host has sent a first JPEG. The browser sends JPEG frames
            // over the session WebSocket, and image2pipe would otherwise fail probe
            // with "Could not find codec parameters" and exit.
            "-vcodec".into(),
            "mjpeg".into(),
            "-framerate".into(),
            "30".into(),
            "-i".into(),
            "pipe:0".into(),
        ],
        VideoInputMode::H264AnnexB => vec![
            "-y".into(),
            "-loglevel".into(),
            "warning".into(),
            "-fflags".into(),
            "+genpts".into(),
            "-flags".into(),
            "low_delay".into(),
            "-thread_queue_size".into(),
            "512".into(),
            "-use_wallclock_as_timestamps".into(),
            "1".into(),
            "-r".into(),
            "30".into(),
            "-f".into(),
            "h264".into(),
            "-i".into(),
            "pipe:0".into(),
        ],
    };
    ffmpeg_args.extend_from_slice(&[
        "-f".into(),
        "s16le".into(),
        "-ar".into(),
        "44100".into(),
        "-ac".into(),
        "1".into(),
        "-i".into(),
        audio_fifo.to_string(),
    ]);
    match video_input_mode {
        VideoInputMode::Mjpeg => ffmpeg_args.extend_from_slice(&[
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "ultrafast".into(),
            "-tune".into(),
            "zerolatency".into(),
            // Target bitrate must be set explicitly — with -preset ultrafast the
            // CRF path alone does not respect -maxrate closely enough, and Grip
            // (AWS IVS) caps ingest at ~3 Mbps and drops the RTMP connection on
            // overshoot. Pin 2500k target, 2500k max, 5000k buffer, 1s GOP.
            "-b:v".into(),
            "2500k".into(),
            "-maxrate".into(),
            "2500k".into(),
            "-bufsize".into(),
            "5000k".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-g".into(),
            "30".into(),
        ]),
        VideoInputMode::H264AnnexB => ffmpeg_args.extend_from_slice(&[
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "ultrafast".into(),
            "-tune".into(),
            "zerolatency".into(),
            "-b:v".into(),
            "2500k".into(),
            "-maxrate".into(),
            "2500k".into(),
            "-bufsize".into(),
            "5000k".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-g".into(),
            "30".into(),
        ]),
    }
    ffmpeg_args.extend_from_slice(&[
        "-c:a".into(),
        "aac".into(),
        "-ac:a".into(),
        "2".into(),
        "-b:a".into(),
        "128k".into(),
        "-map".into(),
        "0:v".into(),
        "-map".into(),
        "1:a".into(),
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
    fn build_ffmpeg_args_skips_drawtext_vf_filter() {
        let args = build_ffmpeg_args("/tmp/fifo_src", "rtmp://x/y", VideoInputMode::Mjpeg);
        let joined = args.join(" ");
        assert!(
            !joined.contains("-vf"),
            "Brivva no longer burns captions into video"
        );
        assert!(
            !joined.contains("drawtext"),
            "caption rendering must stay out of the RTMP path"
        );
        assert!(joined.ends_with("rtmp://x/y"));
        assert!(joined.contains("image2pipe"));
        assert!(joined.contains("-vcodec mjpeg"));
    }

    #[test]
    fn build_ffmpeg_args_always_passes_f_flv_for_rtmp_family_publish() {
        let args = build_ffmpeg_args("/tmp/fifo", "rtmp://localhost/live", VideoInputMode::Mjpeg);
        let idx = args.iter().rposition(|s| s == "-f").expect("-f present");
        assert_eq!(args[idx + 1], "flv");
    }

    #[test]
    fn build_ffmpeg_args_passes_44100_mono_s16le_for_audio_fifo_input() {
        let args = build_ffmpeg_args("/tmp/fifo", "rtmp://x", VideoInputMode::Mjpeg);
        let ar_idx = args.iter().position(|s| s == "-ar").unwrap();
        assert_eq!(args[ar_idx + 1], "44100");
        let ac_idx = args.iter().position(|s| s == "-ac").unwrap();
        assert_eq!(args[ac_idx + 1], "1");
    }

    #[test]
    fn build_ffmpeg_args_enforces_grip_ivs_bitrate_and_keyframe_caps() {
        // Grip (AWS IVS) caps RTMP ingest at ~3 Mbps with a 1s keyframe
        // requirement. Overshooting either value makes IVS drop the stream
        // silently, which in turn causes the ffmpeg child to exit, the
        // idle-restart loop to re-fire three times, and the target prompter
        // to stay dark. Pin: -b:v/-maxrate 2500k, -bufsize 5000k, -g 30.
        let args = build_ffmpeg_args("/tmp/fifo", "rtmp://x", VideoInputMode::Mjpeg);

        let b_v_idx = args
            .iter()
            .position(|s| s == "-b:v")
            .expect("-b:v must be set to enforce target bitrate under ultrafast preset");
        assert_eq!(args[b_v_idx + 1], "2500k");

        let maxrate_idx = args.iter().position(|s| s == "-maxrate").unwrap();
        assert_eq!(args[maxrate_idx + 1], "2500k");

        let bufsize_idx = args.iter().position(|s| s == "-bufsize").unwrap();
        assert_eq!(args[bufsize_idx + 1], "5000k");

        let g_idx = args.iter().position(|s| s == "-g").unwrap();
        assert_eq!(args[g_idx + 1], "30", "1s keyframe interval @ 30fps");

        let b_a_idx = args.iter().position(|s| s == "-b:a").unwrap();
        assert_eq!(args[b_a_idx + 1], "128k");

        let pix_idx = args.iter().position(|s| s == "-pix_fmt").unwrap();
        assert_eq!(args[pix_idx + 1], "yuv420p");
    }

    #[test]
    fn build_ffmpeg_args_h264_webrtc_ingest_decodes_browser_video() {
        let args = build_ffmpeg_args("/tmp/fifo", "rtmp://x", VideoInputMode::H264AnnexB);
        let joined = args.join(" ");
        assert!(joined.contains("-fflags +genpts"));
        assert!(!joined.contains("nobuffer"));
        assert!(joined.contains("-thread_queue_size 512"));
        assert!(joined.contains("-use_wallclock_as_timestamps 1"));
        assert!(joined.contains("-r 30"));
        assert!(joined.contains("-f h264 -i pipe:0"));
        assert!(joined.contains("-c:v libx264"));
        assert!(joined.contains("-preset ultrafast"));
        assert!(joined.contains("-tune zerolatency"));
        assert!(joined.contains("-b:v 2500k"));
        assert!(!joined.contains("drawtext"));
    }
}
