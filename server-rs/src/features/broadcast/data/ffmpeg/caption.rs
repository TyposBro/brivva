//! Burn-in caption writer. Keeps a minimum dwell time per update so a
//! burst of translations doesn't flash through drawtext's `reload=1` poll.

use std::time::{Duration, Instant};

const MIN_CAPTION_DWELL_MS: u64 = 1_500;

/// Writer task pair driven by `RtmpStream`. Each stream owns one caption
/// state so different target languages can write concurrently.
pub(super) struct CaptionState {
    pub path: String,
    sender: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    writer: Option<tokio::task::JoinHandle<()>>,
}

impl CaptionState {
    pub fn spawn(stream_id: &str) -> Self {
        let path = format!("/tmp/brivva_caption_{}.txt", stream_id);
        // Empty initial file so drawtext reads cleanly from the first frame.
        let _ = std::fs::write(&path, "");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let path_clone = path.clone();
        let writer = tokio::spawn(async move {
            let min_dwell = Duration::from_millis(MIN_CAPTION_DWELL_MS);
            let mut last_written = Instant::now()
                .checked_sub(min_dwell)
                .unwrap_or_else(Instant::now);
            while let Some(mut text) = rx.recv().await {
                let elapsed = last_written.elapsed();
                if elapsed < min_dwell {
                    tokio::time::sleep(min_dwell - elapsed).await;
                }
                while let Ok(newer) = rx.try_recv() {
                    text = newer;
                }
                let sanitized = sanitize_caption(&text);
                write_caption_atomic(&path_clone, &sanitized);
                last_written = Instant::now();
            }
        });
        Self {
            path,
            sender: Some(tx),
            writer: Some(writer),
        }
    }

    pub fn push(&self, text: &str) {
        if let Some(tx) = &self.sender {
            let _ = tx.send(text.to_string());
        }
    }

    /// Close the writer channel and abort the background task. Idempotent.
    pub fn shutdown(&mut self) {
        self.sender.take();
        if let Some(handle) = self.writer.take() {
            handle.abort();
        }
    }
}

/// Remove control chars and cap caption length so drawtext stays legible.
pub(super) fn sanitize_caption(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(200)
        .collect()
}

/// Write the caption atomically so FFmpeg's `reload=1` never sees a torn file.
fn write_caption_atomic(path: &str, text: &str) {
    let tmp = format!("{}.tmp", path);
    if let Err(e) = std::fs::write(&tmp, text) {
        eprintln!("[CAPTION] write tmp failed ({}): {}", path, e);
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        eprintln!("[CAPTION] rename failed ({}): {}", path, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_caption_strips_control_chars_and_truncates() {
        let text = format!("hi\x00there\n{}", "a".repeat(250));
        let out = sanitize_caption(&text);

        assert!(!out.contains('\x00'));
        assert!(out.contains('\n'));
        assert_eq!(out.chars().count(), 200);
    }
}
