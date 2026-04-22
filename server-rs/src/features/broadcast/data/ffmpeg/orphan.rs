//! Startup cleanup of orphan ffmpeg processes + stale tmp files, and the
//! one-off MP3 → PCM decoder used by the TTS ingest path.

use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

pub fn kill_orphan_ffmpeg() {
    let output = match std::process::Command::new("pgrep")
        .args(["-f", "brivva_audio"])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "pgrep not available; skipping orphan cleanup"
            );
            return;
        }
    };

    let pids = String::from_utf8_lossy(&output.stdout);
    let mut killed = 0;
    for line in pids.lines() {
        if let Ok(pid) = line.trim().parse::<i32>() {
            let my_pid = std::process::id() as i32;
            if pid == my_pid {
                continue;
            }
            tracing::info!(pid, "killing orphan ffmpeg process");
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
            killed += 1;
        }
    }

    let mut stale_files = 0;
    if let Ok(entries) = std::fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && (name.starts_with("brivva_audio_") || name.starts_with("brivva_caption_"))
            {
                let _ = std::fs::remove_file(entry.path());
                stale_files += 1;
            }
        }
    }

    tracing::info!(
        orphan_pids_killed = killed,
        stale_files_removed = stale_files,
        "orphan ffmpeg cleanup complete"
    );
}

/// Decode MP3 bytes to raw PCM s16le 44.1 kHz mono via FFmpeg subprocess.
///
/// Wrapped in a hard 30s timeout (§0.5.4): without it, a hung ffmpeg child
/// (malformed MP3, fd exhaustion, kernel pipe stall) silently froze the
/// per-lang TTS worker — every subsequent utterance queued behind it,
/// listeners on Grip / YouTube heard nothing, and ops had no greppable
/// trace. April 2026 default-male-voice dropout was this exact path.
pub async fn decode_mp3_to_pcm(mp3: &[u8]) -> Result<Vec<u8>, String> {
    use std::time::Duration;
    const DECODE_TIMEOUT: Duration = Duration::from_secs(30);
    match tokio::time::timeout(DECODE_TIMEOUT, decode_mp3_to_pcm_inner(mp3)).await {
        Ok(result) => result,
        Err(_) => Err(format!(
            "FFmpeg mp3→pcm decode exceeded {}s timeout",
            DECODE_TIMEOUT.as_secs()
        )),
    }
}

async fn decode_mp3_to_pcm_inner(mp3: &[u8]) -> Result<Vec<u8>, String> {
    let mut child = TokioCommand::new("ffmpeg")
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
    Ok(output.stdout)
}
