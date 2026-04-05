//! Incremental MP3 decoder using a long-lived FFmpeg subprocess.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::process::Stdio;
use tokio::process::Command as TokioCommand;
use crate::core::config::BYTES_PER_SEC;
use super::FFMPEG_BIN;

// ── Incremental MP3 Decoder ─────────────────────────────
//
// Long-lived FFmpeg subprocess for streaming MP3→PCM decode.
// Each `feed()` writes MP3 bytes to stdin and reads available PCM from stdout.
// `finish()` closes stdin and drains remaining PCM.

pub struct IncrementalMp3Decoder {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    total_mp3_in: usize,
    total_pcm_out: usize,
}

impl IncrementalMp3Decoder {
    pub async fn new() -> Result<Self, String> {
        let mut child = TokioCommand::new(&*FFMPEG_BIN)
            .args([
                "-f", "mp3", "-i", "pipe:0",
                "-f", "s16le", "-ar", "44100", "-ac", "1",
                "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("IncrementalMp3Decoder spawn failed: {}", e))?;

        let stdin = child.stdin.take().ok_or("No stdin on decoder")?;
        let stdout = child.stdout.take().ok_or("No stdout on decoder")?;

        Ok(Self { child, stdin, stdout, total_mp3_in: 0, total_pcm_out: 0 })
    }

    /// Feed an MP3 chunk and read any available PCM output.
    /// Pre-drains stdout before writing to prevent pipe buffer deadlock.
    pub async fn feed(&mut self, mp3_chunk: &[u8]) -> Result<Vec<u8>, String> {
        // Pre-drain stdout to free pipe buffer space, preventing deadlock
        // when FFmpeg's decoded output exceeds the pipe buffer (64KB).
        let mut pcm = self.read_available().await;

        self.stdin
            .write_all(mp3_chunk)
            .await
            .map_err(|e| format!("Decoder stdin write failed: {}", e))?;
        self.total_mp3_in += mp3_chunk.len();

        // Read decoded PCM produced from this chunk
        pcm.extend(self.read_available().await);
        self.total_pcm_out += pcm.len();
        Ok(pcm)
    }

    /// Close stdin and drain all remaining PCM.
    pub async fn finish(mut self) -> Result<Vec<u8>, String> {
        drop(self.stdin);

        // Wait for FFmpeg to finish processing and write all remaining output.
        // Use a longer timeout (500ms) since FFmpeg may need to flush internal buffers.
        let mut remaining = Vec::new();
        let mut buf = [0u8; 16384];
        loop {
            match tokio::time::timeout(
                Duration::from_millis(500),
                self.stdout.read(&mut buf),
            ).await {
                Ok(Ok(0)) => break,        // EOF — FFmpeg closed stdout cleanly
                Ok(Ok(n)) => remaining.extend_from_slice(&buf[..n]),
                Ok(Err(_)) => break,        // read error
                Err(_) => break,            // timeout — assume no more data
            }
        }

        // Wait for FFmpeg to exit gracefully before force-killing
        match tokio::time::timeout(Duration::from_millis(100), self.child.wait()).await {
            Ok(_) => {}
            Err(_) => { let _ = self.child.kill().await; }
        }
        self.total_pcm_out += remaining.len();

        tracing::debug!(
            "[FFMPEG] IncrementalMp3Decoder: {}B MP3 -> {}B PCM ({:.1}s audio)",
            self.total_mp3_in, self.total_pcm_out,
            self.total_pcm_out as f64 / BYTES_PER_SEC
        );
        Ok(remaining)
    }

    /// Try to read available PCM without blocking for too long.
    async fn read_available(&mut self) -> Vec<u8> {
        let mut result = Vec::new();
        let mut buf = [0u8; 8192];
        // Read in a tight loop with short timeouts to drain buffered output
        loop {
            match tokio::time::timeout(
                Duration::from_millis(5),
                self.stdout.read(&mut buf),
            ).await {
                Ok(Ok(0)) => break,        // EOF
                Ok(Ok(n)) => result.extend_from_slice(&buf[..n]),
                Ok(Err(_)) => break,
                Err(_) => break,            // timeout — nothing more available right now
            }
        }
        result
    }
}
