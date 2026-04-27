//! Startup cleanup of orphan ffmpeg processes + stale tmp files, and the
//! one-off MP3 → PCM decoder used by the TTS ingest path.

use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
                && (name.starts_with("brivva_audio_") || name.starts_with("brivva_video_"))
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

    // Drain stdout concurrently with stdin write. Sequential write→read
    // deadlocks on multi-MB PCM output: ffmpeg's 64KB stdout pipe fills
    // before it finishes reading stdin, so stdin write blocks → decode
    // stalls and hits the 30s outer timeout. This manifested as zh TTS
    // (multi-second Chinese utterances → multi-MB PCM) silently dropping
    // while en (shorter PCM < 64KB pipe cap) decoded fine.
    let mut stdin = child.stdin.take().ok_or("No stdin")?;
    let mut stdout = child.stdout.take().ok_or("No stdout")?;
    let mp3_owned = mp3.to_vec();
    let write_task = tokio::spawn(async move {
        let res = stdin.write_all(&mp3_owned).await;
        drop(stdin);
        res
    });
    let mut pcm = Vec::new();
    let read_res = stdout.read_to_end(&mut pcm).await;
    let write_res = write_task
        .await
        .map_err(|e| format!("FFmpeg stdin writer join failed: {}", e))?;
    write_res.map_err(|e| format!("FFmpeg stdin write failed: {}", e))?;
    read_res.map_err(|e| format!("FFmpeg stdout read failed: {}", e))?;

    let status = child
        .wait()
        .await
        .map_err(|e| format!("FFmpeg wait failed: {}", e))?;
    if !status.success() {
        return Err(format!("FFmpeg exited with status {}", status));
    }
    if pcm.is_empty() {
        return Err("Empty PCM output".to_string());
    }
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: zh TTS dropped silently because decode deadlocked on
    /// multi-MB PCM output (stdout pipe filled before stdin drained).
    /// This test encodederates a 3-second sine MP3 — whose PCM is ~264KB,
    /// well over the 64KB kernel pipe buffer — and asserts decode
    /// completes without hitting the outer 30s timeout.
    #[tokio::test]
    async fn decode_mp3_survives_pcm_output_larger_than_pipe_buffer() {
        // Skip on systems without ffmpeg (CI containers, sandboxes).
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("ffmpeg not available; skipping decode test");
            return;
        }

        // Generate 3s of sine at 44.1k mono, encode as mp3 into memory.
        let encoded = TokioCommand::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3:sample_rate=44100",
                "-ac",
                "1",
                "-b:a",
                "128k",
                "-f",
                "mp3",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .expect("ffmpeg mp3 encoded");
        assert!(encoded.status.success(), "mp3 encoded failed");
        let mp3 = encoded.stdout;
        assert!(!mp3.is_empty(), "expected non-empty mp3");

        let pcm = decode_mp3_to_pcm(&mp3).await.expect("decode ok");
        // 3s @ 44.1k @ 16-bit mono = 264_600 bytes expected (± encoder
        // framing). Key assertion: well above 64KB pipe buffer — the
        // exact size the sequential-write-then-read path deadlocked on.
        assert!(
            pcm.len() > 200_000,
            "expected >200KB pcm (got {}) — deadlock regression?",
            pcm.len()
        );
    }
}
