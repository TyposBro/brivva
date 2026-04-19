//! Integration coverage for the ffmpeg-child stderr drain loop added in
//! commit `000af86`. Prior to that commit, ffmpeg errors were silently
//! discarded: Grip would drop the RTMP connection, ffmpeg would exit, the
//! restart loop would spin three times, and no log line ever told us why.
//!
//! The production path is:
//!   ffmpeg child → BufReader → `drain_stderr_lines` → `tracing::warn!`
//! In a unit test we cannot spawn a real ffmpeg (binary may be missing,
//! rtmp destination flaky). Instead we drive `drain_stderr_lines` with a
//! stand-in child (`/bin/sh -c "echo line1 >&2; echo line2 >&2"`) so the
//! exact same BufRead-over-ChildStderr topology is exercised — just with
//! a shell process in place of ffmpeg.
//!
//! Assertions:
//! * Each stderr line is forwarded exactly once (ordering preserved).
//! * The loop terminates when the child closes stderr on exit (EOF).
//! * Calling the drain against an already-closed pipe does not panic.

use server_rs::features::broadcast::data::ffmpeg::drain_stderr_lines;
use std::io::BufReader;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Shell stand-in for ffmpeg: writes two stderr lines then exits cleanly.
/// Mirrors the production topology where ffmpeg is spawned with `stderr(Stdio::piped())`
/// and its stderr handle is handed to a BufReader on a dedicated drain thread.
fn spawn_two_line_stderr_child() -> std::process::Child {
    Command::new("/bin/sh")
        .args(["-c", "echo line1 >&2; echo line2 >&2"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn shell stand-in for ffmpeg")
}

#[test]
fn drain_stderr_lines_forwards_every_line_in_order_until_eof() {
    let mut child = spawn_two_line_stderr_child();
    let stderr = child.stderr.take().expect("piped stderr handle");

    let (tx, rx) = mpsc::channel::<String>();
    // Spawn the drain on a background thread to mirror production — the
    // reader's `for line in reader.lines()` blocks until EOF. This proves
    // the thread-based drain exits cleanly instead of hanging forever.
    let drain_handle = thread::spawn(move || {
        drain_stderr_lines(BufReader::new(stderr), |line| {
            tx.send(line).expect("recv side dropped");
        });
    });

    let _ = child.wait();
    drain_handle
        .join()
        .expect("drain thread must exit on EOF without panicking");

    let mut lines: Vec<String> = Vec::new();
    while let Ok(line) = rx.recv_timeout(Duration::from_millis(50)) {
        lines.push(line);
    }
    assert_eq!(
        lines,
        vec!["line1".to_string(), "line2".to_string()],
        "drain must forward every stderr line exactly once, in order"
    );
}

#[test]
fn drain_stderr_lines_emits_nothing_for_silent_child_but_still_terminates() {
    // Regression guard: a well-behaved ffmpeg that never writes to stderr
    // (e.g. `-loglevel quiet` + success path) must still let the drain
    // thread exit on EOF. Without EOF termination, the session's teardown
    // path would block waiting on the join handle.
    let mut child = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn silent child");
    let stderr = child.stderr.take().expect("piped stderr handle");

    let (tx, rx) = mpsc::channel::<String>();
    let handle = thread::spawn(move || {
        drain_stderr_lines(BufReader::new(stderr), |line| {
            tx.send(line).expect("recv side dropped");
        });
    });

    let _ = child.wait();
    handle.join().expect("drain must terminate on EOF");

    // No bytes written → no lines captured.
    assert!(rx.try_recv().is_err(), "silent child yields no lines");
}

#[test]
fn drain_stderr_lines_tolerates_already_closed_stderr_without_panic() {
    // If ffmpeg dies before the drain thread starts, BufRead::lines on the
    // now-closed pipe yields None immediately (EOF). The drain must handle
    // this gracefully — no panic, no hang.
    let mut child = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn child");
    // Wait for the child to exit BEFORE starting the drain so stderr is
    // definitely closed by the time we read.
    let _ = child.wait();
    let stderr = child.stderr.take().expect("piped stderr handle");

    let (tx, rx) = mpsc::channel::<String>();
    drain_stderr_lines(BufReader::new(stderr), |line| {
        tx.send(line).expect("recv side dropped");
    });
    assert!(rx.try_recv().is_err(), "closed pipe yields no lines");
}
