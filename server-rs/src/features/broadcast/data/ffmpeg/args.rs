use std::io::BufRead;

/// Pick the Noto Sans CJK regional variant whose glyph forms match the
/// caption language. The single `NotoSansCJK.ttc` ships all four; selecting
/// the right family name avoids cross-region glyph substitution (e.g.
/// Chinese variants of kanji on a Japanese stream).
fn caption_font_for_lang(lang: &str) -> &'static str {
    match lang {
        "ja" => "Noto Sans CJK JP",
        "ko" => "Noto Sans CJK KR",
        "zh" | "zh-CN" | "zh-Hans" => "Noto Sans CJK SC",
        "zh-TW" | "zh-Hant" => "Noto Sans CJK TC",
        _ => "Noto Sans CJK JP",
    }
}

/// Pure builder for the FFmpeg CLI args. Factored out of `spawn_stream_inner`
/// so tests can pin the command shape without spawning an FFmpeg child.
pub(super) fn build_ffmpeg_args(
    audio_fifo: &str,
    caption_textfile_path: Option<&str>,
    rtmp_url: &str,
    caption_lang: Option<&str>,
) -> Vec<String> {
    let mut ffmpeg_args: Vec<String> = vec![
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
        "-f".into(),
        "s16le".into(),
        "-ar".into(),
        "44100".into(),
        "-ac".into(),
        "1".into(),
        "-i".into(),
        audio_fifo.to_string(),
    ];
    if let Some(path) = caption_textfile_path {
        // Escape the textfile path for drawtext — it uses `\` as an escape and
        // `:` as a filter-option separator.
        let escaped = path.replace('\\', "\\\\").replace(':', "\\:");
        let font = caption_font_for_lang(caption_lang.unwrap_or(""));
        let drawtext = format!(
            "drawtext=textfile={}:reload=1:font={}:fontcolor=white:fontsize=28:box=1:boxcolor=black@0.6:boxborderw=10:x=(w-text_w)/2:y=h-120",
            escaped, font
        );
        ffmpeg_args.extend_from_slice(&["-vf".into(), drawtext]);
    }
    ffmpeg_args.extend_from_slice(&[
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
    fn build_ffmpeg_args_source_stream_skips_drawtext_vf_filter() {
        let args = build_ffmpeg_args("/tmp/fifo_src", None, "rtmp://x/y", None);
        let joined = args.join(" ");
        assert!(!joined.contains("-vf"), "source must not add drawtext");
        assert!(joined.ends_with("rtmp://x/y"));
        assert!(joined.contains("image2pipe"));
        assert!(joined.contains("-vcodec mjpeg"));
    }

    #[test]
    fn build_ffmpeg_args_target_stream_injects_escaped_drawtext_vf() {
        let args = build_ffmpeg_args(
            "/tmp/fifo_tgt",
            Some("/tmp/caption:with:colons"),
            "rtmps://edge/live/KEY",
            Some("ja"),
        );
        let vf_index = args
            .iter()
            .position(|s| s == "-vf")
            .expect("drawtext filter present");
        let filter = &args[vf_index + 1];
        assert!(filter.starts_with("drawtext=textfile="));
        assert!(
            filter.contains("/tmp/caption\\:with\\:colons"),
            "colons must be escaped for drawtext: got {filter}"
        );
        assert!(args.last().unwrap().starts_with("rtmps://"));
    }

    #[test]
    fn build_ffmpeg_args_picks_cjk_font_matching_caption_lang() {
        let ko = build_ffmpeg_args("/tmp/f", Some("/tmp/c"), "rtmp://x", Some("ko"));
        assert!(ko.iter().any(|a| a.contains("font=Noto Sans CJK KR")));
        let zh = build_ffmpeg_args("/tmp/f", Some("/tmp/c"), "rtmp://x", Some("zh"));
        assert!(zh.iter().any(|a| a.contains("font=Noto Sans CJK SC")));
    }

    #[test]
    fn build_ffmpeg_args_always_passes_f_flv_for_rtmp_family_publish() {
        let args = build_ffmpeg_args("/tmp/fifo", None, "rtmp://localhost/live", None);
        let idx = args.iter().rposition(|s| s == "-f").expect("-f present");
        assert_eq!(args[idx + 1], "flv");
    }

    #[test]
    fn build_ffmpeg_args_passes_44100_mono_s16le_for_audio_fifo_input() {
        let args = build_ffmpeg_args("/tmp/fifo", None, "rtmp://x", None);
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
        let args = build_ffmpeg_args("/tmp/fifo", None, "rtmp://x", None);

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
}
