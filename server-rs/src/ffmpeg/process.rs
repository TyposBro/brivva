use std::process::Stdio;
use std::sync::LazyLock;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

use crate::constants::BYTES_PER_SEC;

// ── FFmpeg Binary Resolution ──────────────────────────────
//
// Looks for bundled FFmpeg (Tauri sidecar) next to the executable first,
// then falls back to system PATH.

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
const SIDECAR_NAME: &str = "ffmpeg-aarch64-apple-darwin";
#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
const SIDECAR_NAME: &str = "ffmpeg-x86_64-apple-darwin";
#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
const SIDECAR_NAME: &str = "ffmpeg-x86_64-unknown-linux-gnu";
#[cfg(not(any(
    all(target_arch = "aarch64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"),
)))]
const SIDECAR_NAME: &str = "ffmpeg";

pub(crate) static FFMPEG_BIN: LazyLock<String> = LazyLock::new(|| {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent() {
            // Tauri sidecar convention: ffmpeg-{target_triple}
            let sidecar = dir.join(SIDECAR_NAME);
            if sidecar.exists() {
                tracing::info!("[FFMPEG] Using bundled: {}", sidecar.display());
                return sidecar.to_string_lossy().to_string();
            }
            // Plain name (manual placement)
            let plain = dir.join("ffmpeg");
            if plain.exists() {
                tracing::info!("[FFMPEG] Using bundled: {}", plain.display());
                return plain.to_string_lossy().to_string();
            }
        }
    tracing::info!("[FFMPEG] Using system ffmpeg from PATH");
    "ffmpeg".to_string()
});

/// Kill any orphaned FFmpeg processes from a previous server crash.
/// Called once at startup before accepting connections.
/// Matches FFmpeg processes whose cmdline contains "brivva_audio" (our FIFO naming convention).
pub fn kill_orphan_ffmpeg() {
    let output = match std::process::Command::new("pgrep")
        .args(["-f", "brivva_audio"])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("[STARTUP] pgrep not available, skipping orphan cleanup: {}", e);
            return;
        }
    };

    let pids = String::from_utf8_lossy(&output.stdout);
    let mut killed = 0;
    for line in pids.lines() {
        if let Ok(pid) = line.trim().parse::<i32>() {
            // Don't kill ourselves
            let my_pid = std::process::id() as i32;
            if pid == my_pid {
                continue;
            }
            tracing::info!("[STARTUP] killing orphan FFmpeg process (PID {})", pid);
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
            killed += 1;
        }
    }

    // Clean up stale FIFOs
    if let Ok(entries) = std::fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && name.starts_with("brivva_audio_") {
                    let _ = std::fs::remove_file(entry.path());
                    tracing::info!("[STARTUP] removed stale FIFO: {}", name);
                }
        }
    }

    if killed > 0 {
        tracing::info!("[STARTUP] killed {} orphan FFmpeg process(es)", killed);
    } else {
        tracing::info!("[STARTUP] no orphan FFmpeg processes found");
    }
}

/// Decode MP3 bytes to raw PCM s16le 44100Hz mono using FFmpeg subprocess
pub async fn decode_mp3_to_pcm(mp3: &[u8]) -> Result<Vec<u8>, String> {
    tracing::debug!("[FFMPEG] decode_mp3_to_pcm: {}B MP3 input", mp3.len());
    let decode_start = Instant::now();
    let mut child = TokioCommand::new(&*FFMPEG_BIN)
        .args([
            "-f", "mp3", "-i", "pipe:0", "-f", "s16le", "-ar", "44100", "-ac", "1", "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("FFmpeg decode spawn failed: {}", e))?;

    let mut stdin = child.stdin.take().ok_or("No stdin")?;
    stdin
        .write_all(mp3)
        .await
        .map_err(|e| format!("FFmpeg stdin write failed: {}", e))?;
    drop(stdin);

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("FFmpeg wait failed: {}", e))?;
    if output.stdout.is_empty() {
        return Err("Empty PCM output".to_string());
    }
    tracing::debug!(
        "[FFMPEG] decode_mp3_to_pcm: {}B MP3 -> {}B PCM ({:.1}s audio) in {}ms",
        mp3.len(), output.stdout.len(),
        output.stdout.len() as f64 / BYTES_PER_SEC,
        decode_start.elapsed().as_millis()
    );
    Ok(output.stdout)
}
