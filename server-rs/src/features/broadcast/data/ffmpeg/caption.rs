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

    #[test]
    fn sanitize_caption_passes_through_empty_input() {
        assert_eq!(sanitize_caption(""), "");
    }

    #[test]
    fn sanitize_caption_keeps_short_text_unchanged() {
        assert_eq!(sanitize_caption("안녕하세요"), "안녕하세요");
    }

    #[test]
    fn sanitize_caption_strips_other_control_chars_but_keeps_newline() {
        let out = sanitize_caption("a\x07\tb\n");
        assert_eq!(out, "ab\n");
    }

    #[test]
    fn write_caption_atomic_writes_file_at_target_path() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("brivva_caption_test_{}.txt", std::process::id()));
        let path_str = path.to_str().unwrap().to_string();

        write_caption_atomic(&path_str, "hello");
        let contents = std::fs::read_to_string(&path).expect("caption file written");
        assert_eq!(contents, "hello");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn caption_state_push_and_shutdown_drains_gracefully() {
        let id = format!("test-{}", std::process::id());
        let mut state = CaptionState::spawn(&id);
        assert!(state.path.contains(&id));
        state.push("hi");
        state.shutdown();
        // Second shutdown is a no-op.
        state.shutdown();
        let _ = std::fs::remove_file(&state.path);
    }

    #[tokio::test]
    async fn caption_state_push_flushes_first_write_immediately_and_coalesces_bursts() {
        let id = format!("flush-{}", std::process::id());
        let mut state = CaptionState::spawn(&id);
        let path = state.path.clone();

        // First write has no dwell-time debt → land within a few ms.
        state.push("first");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let first = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            first, "first",
            "first write should flush before dwell timer"
        );

        // Two rapid writes inside the dwell window — only the latest should
        // land, because the worker coalesces via `try_recv`.
        state.push("second");
        state.push("third");
        tokio::time::sleep(std::time::Duration::from_millis(2_000)).await;
        let final_contents = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            final_contents, "third",
            "burst should coalesce to the newest text after dwell"
        );

        state.shutdown();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_caption_atomic_overwrites_existing_file_contents() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "brivva_caption_atomic_test_{}.txt",
            std::process::id()
        ));
        let path_str = path.to_str().unwrap().to_string();

        write_caption_atomic(&path_str, "one");
        write_caption_atomic(&path_str, "two");
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "two");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_caption_atomic_handles_unwritable_directory_gracefully() {
        // Writing under a non-existent directory tree triggers the error
        // branch for `fs::write(tmp, ...)`. The function must not panic.
        let path = "/this/does/not/exist/brivva_caption_test.txt";
        write_caption_atomic(path, "hi"); // returns early with log
    }
}
