use std::process::Stdio;
use std::sync::LazyLock;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

use crate::core::config::BYTES_PER_SEC;

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

// ── Public API ───────────────────────────────────────────

/// Kill any orphaned FFmpeg processes from a previous server crash.
/// Called once at startup before accepting connections.
/// Matches FFmpeg processes whose cmdline contains "brivva_audio" (our FIFO naming convention).
pub fn kill_orphan_ffmpeg() {
    let pids = find_orphan_pids();
    let killed = kill_processes(&pids);
    cleanup_stale_fifos();
    log_kill_summary(killed);
}

/// Decode MP3 bytes to raw PCM s16le 44100Hz mono using FFmpeg subprocess
pub async fn decode_mp3_to_pcm(mp3: &[u8]) -> Result<Vec<u8>, String> {
    tracing::debug!("[FFMPEG] decode_mp3_to_pcm: {}B MP3 input", mp3.len());
    let decode_start = Instant::now();

    let output = run_ffmpeg_decode(mp3).await?;
    validate_pcm_output(&output, mp3.len(), decode_start)
}

// ── Orphan cleanup helpers ───────────────────────────────

fn find_orphan_pids() -> Vec<i32> {
    let output = match std::process::Command::new("pgrep")
        .args(["-f", "brivva_audio"])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("[STARTUP] pgrep not available, skipping orphan cleanup: {}", e);
            return Vec::new();
        }
    };
    parse_pids(&String::from_utf8_lossy(&output.stdout))
}

fn parse_pids(raw: &str) -> Vec<i32> {
    let my_pid = std::process::id() as i32;
    raw.lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .filter(|&pid| pid != my_pid)
        .collect()
}

fn kill_processes(pids: &[i32]) -> usize {
    pids.iter().filter(|&&pid| kill_one(pid)).count()
}

fn kill_one(pid: i32) -> bool {
    tracing::info!("[STARTUP] killing orphan FFmpeg process (PID {})", pid);
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .output();
    true
}

fn cleanup_stale_fifos() {
    let entries = match std::fs::read_dir("/tmp") {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        remove_if_stale_fifo(&entry);
    }
}

fn remove_if_stale_fifo(entry: &std::fs::DirEntry) {
    if let Some(name) = entry.file_name().to_str()
        && name.starts_with("brivva_audio_") {
            let _ = std::fs::remove_file(entry.path());
            tracing::info!("[STARTUP] removed stale FIFO: {}", name);
        }
}

fn log_kill_summary(killed: usize) {
    if killed > 0 {
        tracing::info!("[STARTUP] killed {} orphan FFmpeg process(es)", killed);
    } else {
        tracing::info!("[STARTUP] no orphan FFmpeg processes found");
    }
}

// ── MP3 decode helpers ───────────────────────────────────

async fn run_ffmpeg_decode(mp3: &[u8]) -> Result<std::process::Output, String> {
    let mut child = spawn_ffmpeg_decoder()?;
    write_stdin(&mut child, mp3).await?;
    child.wait_with_output().await.map_err(|e| format!("FFmpeg wait failed: {}", e))
}

fn spawn_ffmpeg_decoder() -> Result<tokio::process::Child, String> {
    TokioCommand::new(&*FFMPEG_BIN)
        .args(["-f", "mp3", "-i", "pipe:0", "-f", "s16le", "-ar", "44100", "-ac", "1", "pipe:1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("FFmpeg decode spawn failed: {}", e))
}

async fn write_stdin(child: &mut tokio::process::Child, data: &[u8]) -> Result<(), String> {
    let mut stdin = child.stdin.take().ok_or("No stdin")?;
    stdin.write_all(data).await.map_err(|e| format!("FFmpeg stdin write failed: {}", e))?;
    drop(stdin);
    Ok(())
}

fn validate_pcm_output(output: &std::process::Output, mp3_len: usize, start: Instant) -> Result<Vec<u8>, String> {
    if output.stdout.is_empty() {
        return Err("Empty PCM output".to_string());
    }
    tracing::debug!(
        "[FFMPEG] decode_mp3_to_pcm: {}B MP3 -> {}B PCM ({:.1}s audio) in {}ms",
        mp3_len, output.stdout.len(),
        output.stdout.len() as f64 / BYTES_PER_SEC,
        start.elapsed().as_millis()
    );
    Ok(output.stdout.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_parse_single_pid_from_pgrep_output() {
        let my_pid = std::process::id() as i32;
        let foreign_pid = if my_pid == 99999 { 99998 } else { 99999 };
        let raw = format!("{}\n", foreign_pid);

        let pids = parse_pids(&raw);

        assert_eq!(pids, vec![foreign_pid]);
    }

    #[test]
    fn should_parse_multiple_pids() {
        let my_pid = std::process::id() as i32;
        let pid_a = if my_pid == 111 { 112 } else { 111 };
        let pid_b = if my_pid == 222 { 223 } else { 222 };
        let raw = format!("{}\n{}\n", pid_a, pid_b);

        let pids = parse_pids(&raw);

        assert_eq!(pids, vec![pid_a, pid_b]);
    }

    #[test]
    fn should_exclude_own_pid() {
        let my_pid = std::process::id() as i32;
        let raw = format!("{}\n", my_pid);

        let pids = parse_pids(&raw);

        assert!(pids.is_empty());
    }

    #[test]
    fn should_return_empty_for_empty_input() {
        let pids = parse_pids("");

        assert!(pids.is_empty());
    }

    #[test]
    fn should_skip_non_numeric_lines() {
        let raw = "not_a_pid\nabc\n";

        let pids = parse_pids(raw);

        assert!(pids.is_empty());
    }

    #[test]
    fn should_handle_whitespace_around_pids() {
        let my_pid = std::process::id() as i32;
        let foreign_pid = if my_pid == 42 { 43 } else { 42 };
        let raw = format!("  {}  \n", foreign_pid);

        let pids = parse_pids(&raw);

        assert_eq!(pids, vec![foreign_pid]);
    }

    #[test]
    fn should_skip_blank_lines() {
        let my_pid = std::process::id() as i32;
        let foreign_pid = if my_pid == 500 { 501 } else { 500 };
        let raw = format!("\n{}\n\n", foreign_pid);

        let pids = parse_pids(&raw);

        assert_eq!(pids, vec![foreign_pid]);
    }

    #[test]
    fn should_handle_mixed_valid_and_invalid_lines() {
        let my_pid = std::process::id() as i32;
        let foreign_pid = if my_pid == 777 { 778 } else { 777 };
        let raw = format!("abc\n{}\nnot_valid\n{}\n", foreign_pid, my_pid);

        let pids = parse_pids(&raw);

        assert_eq!(pids, vec![foreign_pid]);
    }
}
