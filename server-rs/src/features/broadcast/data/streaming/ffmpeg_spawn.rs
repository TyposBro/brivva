//! FFmpeg process spawning, FIFO creation, and stderr monitoring.

use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use std::sync::LazyLock;


const FONTCONFIG_PATH: &str = "/tmp/brivva_fonts.conf";

/// Minimal Fontconfig XML that points at macOS / Linux system font directories.
/// Written once to /tmp so the bundled FFmpeg sidecar can find it.
/// `<rescan><int>0</int></rescan>` disables periodic re-scanning (avoids I/O stalls).
const MINIMAL_FONTS_CONF: &str = r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <dir>/System/Library/Fonts</dir>
  <dir>/System/Library/Fonts/Supplemental</dir>
  <dir>/Library/Fonts</dir>
  <dir>/usr/share/fonts</dir>
  <cachedir>/tmp/brivva_fontconfig_cache</cachedir>
  <config><rescan><int>0</int></rescan></config>
</fontconfig>
"#;

const FONTCONFIG_CACHE_DIR: &str = "/tmp/brivva_fontconfig_cache";

/// Write a minimal fonts.conf once and pre-create the cache dir.
static FONTCONFIG_FILE: LazyLock<Option<String>> = LazyLock::new(|| {
    let _ = std::fs::create_dir_all(FONTCONFIG_CACHE_DIR);
    match std::fs::write(FONTCONFIG_PATH, MINIMAL_FONTS_CONF) {
        Ok(()) => {
            tracing::debug!("[FFMPEG] Wrote minimal fontconfig to {}", FONTCONFIG_PATH);
            Some(FONTCONFIG_PATH.to_string())
        }
        Err(e) => {
            tracing::warn!("[FFMPEG] Failed to write fontconfig: {}", e);
            None
        }
    }
});

fn ensure_fontconfig() -> Option<String> {
    FONTCONFIG_FILE.clone()
}

/// Create a named FIFO for audio data at /tmp/brivva_audio_{stream_id}.
pub(super) fn create_audio_fifo(stream_id: &str) -> Result<String, String> {
    let path = format!("/tmp/brivva_audio_{}", stream_id);
    let _ = std::fs::remove_file(&path);
    tracing::info!("[FFMPEG:{}] creating FIFO: {}", stream_id, path);
    std::process::Command::new("mkfifo")
        .arg(&path)
        .output()
        .map_err(|e| format!("mkfifo failed: {}", e))?;
    Ok(path)
}

/// Spawn the FFmpeg process. Returns the child, its stdin, and an RTMP error flag.
/// Uses system FFmpeg for RTMPS URLs (needs OpenSSL), bundled for everything else.
pub(super) fn spawn_ffmpeg_process(
    stream_id: &str,
    args: &[String],
    rtmp_url: &str,
) -> Result<(std::process::Child, std::process::ChildStdin, Arc<AtomicBool>), String> {
    validate_rtmps_support(rtmp_url)?;
    let bin = pick_ffmpeg_bin(rtmp_url);
    tracing::info!(
        "[FFMPEG:{}] spawning: {} {}",
        stream_id, bin, args.join(" ")
    );
    let fontconfig_file = ensure_fontconfig();
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(ref path) = fontconfig_file {
        cmd.env("FONTCONFIG_FILE", path);
        cmd.env("FONTCONFIG_PATH", "/tmp");
    }
    cmd.env("FC_DEBUG", "0");
    let mut child = cmd.spawn()
        .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;
    tracing::info!("[FFMPEG:{}] spawned PID={}", stream_id, child.id());

    let stdin = child.stdin.take().ok_or("No FFmpeg stdin")?;
    let rtmp_error = Arc::new(AtomicBool::new(false));
    spawn_stderr_reader(stream_id, &mut child, &rtmp_error);
    Ok((child, stdin, rtmp_error))
}

/// Fail early when RTMPS is requested but no system FFmpeg with TLS is available.
/// Bundled FFmpeg uses SecureTransport (macOS) which doesn't support RTMPS properly.
fn validate_rtmps_support(rtmp_url: &str) -> Result<(), String> {
    if rtmp_url.starts_with("rtmps://") && super::process::FFMPEG_SYSTEM.is_none() {
        return Err(
            "RTMPS requires system FFmpeg with TLS/OpenSSL support, but none was found in PATH. \
             Install: `brew install ffmpeg` (macOS) or `apt-get install ffmpeg` (Linux).".to_string()
        );
    }
    Ok(())
}

/// RTMPS needs OpenSSL (system FFmpeg). Bundled FFmpeg uses SecureTransport = broken RTMPS.
fn pick_ffmpeg_bin(rtmp_url: &str) -> &str {
    if rtmp_url.starts_with("rtmps://") {
        if let Some(sys) = super::process::FFMPEG_SYSTEM.as_deref() {
            tracing::info!("[FFMPEG] Using system ffmpeg for RTMPS (OpenSSL)");
            return sys;
        }
        tracing::warn!("[FFMPEG] RTMPS URL but no system ffmpeg found, using bundled (may fail)");
    }
    &super::process::FFMPEG_BIN
}

fn spawn_stderr_reader(
    stream_id: &str,
    child: &mut std::process::Child,
    rtmp_error: &Arc<AtomicBool>,
) {
    if let Some(stderr) = child.stderr.take() {
        let sid = stream_id.to_string();
        let err_flag = rtmp_error.clone();
        thread::Builder::new()
            .name(format!("ffmpeg-stderr-{}", stream_id))
            .spawn(move || {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(l) if !l.is_empty() => process_stderr_line(&sid, &l, &err_flag),
                        Err(_) => break,
                        _ => {}
                    }
                }
            })
            .ok();
    }
}

fn process_stderr_line(stream_id: &str, line: &str, err_flag: &AtomicBool) {
    tracing::warn!("[FFMPEG:{}] {}", stream_id, line);
    if is_rtmp_connection_error(line) {
        tracing::error!("[FFMPEG:{}] RTMP error detected, flagging for restart", stream_id);
        err_flag.store(true, Ordering::Release);
    }
}

fn is_rtmp_connection_error(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("broken pipe")
        || lower.contains("connection timed out")
        || lower.contains("i/o error")
        || lower.contains("error writing trailer")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_detect_connection_refused() {
        assert!(is_rtmp_connection_error("Connection refused"));
    }

    #[test]
    fn should_detect_connection_reset() {
        assert!(is_rtmp_connection_error("Connection reset by peer"));
    }

    #[test]
    fn should_detect_broken_pipe() {
        assert!(is_rtmp_connection_error("Broken pipe"));
    }

    #[test]
    fn should_detect_connection_timed_out() {
        assert!(is_rtmp_connection_error("Connection timed out"));
    }

    #[test]
    fn should_detect_io_error() {
        assert!(is_rtmp_connection_error("I/O error reading from socket"));
    }

    #[test]
    fn should_detect_error_writing_trailer() {
        assert!(is_rtmp_connection_error("Error writing trailer"));
    }

    #[test]
    fn should_be_case_insensitive() {
        assert!(is_rtmp_connection_error("CONNECTION REFUSED"));
        assert!(is_rtmp_connection_error("broken PIPE"));
    }

    #[test]
    fn should_not_match_normal_ffmpeg_output() {
        assert!(!is_rtmp_connection_error("frame= 100 fps=30 q=23.0 size=256kB"));
    }

    #[test]
    fn should_not_match_empty_string() {
        assert!(!is_rtmp_connection_error(""));
    }

    #[test]
    fn should_detect_error_embedded_in_longer_line() {
        assert!(is_rtmp_connection_error(
            "[flv @ 0x5f] Connection refused while writing packet"
        ));
    }
}
