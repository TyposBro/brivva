//! Integration test for the burn-in caption pipeline.
//!
//! Drives a translation event through `RtmpManager::push_caption` (the same
//! entry point `emit_translation` uses in production) and asserts the
//! resulting drawtext textfile lands on disk with the expected text within
//! 500 ms.
//!
//! This test exists because the 2026-04-20 production session against Grip
//! produced zero `drawtext|font|caption` log lines and zero on-screen
//! captions, with no way to bisect "writer never invoked" from "writer wrote
//! but ffmpeg never reloaded." The unit-level tests under
//! `caption.rs` and `mod.rs` cover the `CaptionState` writer in isolation;
//! this file pins the cross-module behavior end-to-end at the file-system
//! boundary so a regression here surfaces in CI.
//!
//! Requires the `test-helpers` feature so `RtmpManager::insert_test_target_stream`
//! is reachable from outside the crate's `#[cfg(test)]` scope.

use server_rs::features::broadcast::data::ffmpeg::RtmpManager;
use std::time::{Duration, Instant};

/// Block until the textfile contains `expected` or the deadline passes.
async fn wait_for_textfile(path: &str, expected: &str, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    let mut last = String::new();
    while Instant::now() < deadline {
        last = std::fs::read_to_string(path).unwrap_or_default();
        if last == expected {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    last
}

#[tokio::test]
async fn translated_text_lands_on_drawtext_textfile_within_500ms() {
    let mut manager = RtmpManager::new();
    let stream_id = format!("intg-cap-{}", std::process::id());
    let textfile = manager.insert_test_target_stream(&stream_id, "ja");

    manager.push_caption("ja", "翻訳テスト".into());

    let on_disk = wait_for_textfile(&textfile, "翻訳テスト", Duration::from_millis(500)).await;
    assert_eq!(
        on_disk, "翻訳テスト",
        "drawtext textfile must contain the translated text within 500 ms"
    );

    manager.stop_all().await;
    let _ = std::fs::remove_file(&textfile);
}

#[tokio::test]
async fn caption_is_isolated_per_stream_when_two_target_langs_are_attached() {
    let mut manager = RtmpManager::new();
    let ja_id = format!("intg-cap-ja-{}", std::process::id());
    let ko_id = format!("intg-cap-ko-{}", std::process::id());
    let ja_path = manager.insert_test_target_stream(&ja_id, "ja");
    let ko_path = manager.insert_test_target_stream(&ko_id, "ko");

    manager.push_caption("ja", "こんにちは".into());
    manager.push_caption("ko", "안녕하세요".into());

    let ja_disk = wait_for_textfile(&ja_path, "こんにちは", Duration::from_millis(500)).await;
    let ko_disk = wait_for_textfile(&ko_path, "안녕하세요", Duration::from_millis(500)).await;
    assert_eq!(ja_disk, "こんにちは", "ja textfile diverged from its lang");
    assert_eq!(ko_disk, "안녕하세요", "ko textfile diverged from its lang");

    manager.stop_all().await;
    let _ = std::fs::remove_file(&ja_path);
    let _ = std::fs::remove_file(&ko_path);
}

#[tokio::test]
async fn caption_for_unmatched_lang_does_not_pollute_target_textfile() {
    let mut manager = RtmpManager::new();
    let stream_id = format!("intg-cap-skip-{}", std::process::id());
    let textfile = manager.insert_test_target_stream(&stream_id, "ja");

    // Lang mismatch — must NOT land on the ja stream's textfile.
    manager.push_caption("ko", "안녕하세요".into());

    tokio::time::sleep(Duration::from_millis(200)).await;
    let on_disk = std::fs::read_to_string(&textfile).unwrap_or_default();
    assert!(
        on_disk.is_empty(),
        "ja textfile must stay empty when ko caption is pushed: got {on_disk:?}"
    );

    manager.stop_all().await;
    let _ = std::fs::remove_file(&textfile);
}
