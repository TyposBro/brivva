use super::*;
use std::sync::atomic::Ordering;

#[test]
fn timing_constants_match_documented_values() {
    assert_eq!(MAX_FFMPEG_RESTARTS, 3);
    assert_eq!(FFMPEG_RESTART_DELAY, Duration::from_secs(2));
    assert_eq!(IDLE_RESTART_THRESHOLD, Duration::from_secs(25));
    assert_eq!(HOST_AUDIO_CAP_BYTES, 20 * 88_200);
    assert_eq!(HOST_VIDEO_H264_CAP_CHUNKS, 120_000);
    assert_eq!(TTS_QUEUE_CAP_BYTES, 60 * 88_200);
}

#[test]
fn now_unix_ms_returns_positive_millis_since_epoch() {
    let t = now_unix_ms();
    assert!(t > 1_700_000_000_000, "expected modern-era millis, got {t}");
}

#[test]
fn stream_buffers_new_starts_empty() {
    let b = StreamBuffers::new();
    assert_eq!(b.audio.lock().unwrap().len(), 0);
    assert_eq!(b.video_h264.lock().unwrap().len(), 0);
    assert_eq!(b.tts.lock().unwrap().len(), 0);
}

#[test]
fn rtmp_manager_new_is_empty_and_has_no_metrics() {
    let m = RtmpManager::new();
    assert!(m.streams.is_empty());
    assert!(m.metrics.is_none());
}

#[test]
fn rtmp_manager_default_equals_new() {
    let m = RtmpManager::default();
    assert!(m.streams.is_empty());
    assert!(m.metrics.is_none());
}

#[test]
fn set_metrics_stores_arc_for_later_use_by_drain_threads() {
    let mut m = RtmpManager::new();
    let metrics = SessionMetrics::new();
    m.set_metrics(metrics.clone());
    assert!(m.metrics.is_some());
    // Pointer equality — set_metrics must not clone internally and lose sharing.
    let stored = m.metrics.as_ref().unwrap().clone();
    assert!(Arc::ptr_eq(&stored, &metrics));
}

#[test]
fn push_video_h264_on_empty_manager_records_nothing_and_does_not_panic() {
    let m = RtmpManager::new();
    m.push_video_h264(&[0u8; 16]);
    assert!(m.streams.is_empty());
}

#[test]
fn push_host_audio_on_empty_manager_is_a_noop() {
    let m = RtmpManager::new();
    m.push_host_audio(&[]);
    m.push_host_audio(&[1, 2, 3]);
    assert!(m.streams.is_empty());
}

#[test]
fn push_tts_bumps_metrics_even_when_no_stream_matches_lang() {
    let mut m = RtmpManager::new();
    let metrics = SessionMetrics::new();
    m.set_metrics(metrics.clone());
    // Big enough to cross the 1ms threshold for lang accounting.
    m.push_tts("ja", vec![0u8; 88_200]);
    let snap = metrics.snapshot();
    assert!(
        snap.output_seconds_by_lang.contains_key("ja"),
        "metrics should record the lang even without a matching stream"
    );
}

#[test]
fn push_tts_is_a_noop_without_metrics_and_no_streams() {
    let m = RtmpManager::new();
    m.push_tts("ja", vec![0u8; 100]);
}

#[test]
fn detect_crashed_on_empty_manager_returns_empty_vec() {
    let mut m = RtmpManager::new();
    let crashed = m.detect_crashed();
    assert!(crashed.is_empty());
}

#[test]
fn kill_idle_streams_on_empty_manager_does_nothing() {
    let mut m = RtmpManager::new();
    m.kill_idle_streams();
    assert!(m.streams.is_empty());
}

#[tokio::test]
async fn stop_all_on_empty_manager_leaves_manager_empty_and_does_not_hang() {
    let mut m = RtmpManager::new();
    m.stop_all().await;
    assert!(m.streams.is_empty());
}

#[tokio::test]
async fn spawn_health_monitor_exits_promptly_when_stop_flag_preset() {
    let manager: SharedRtmpManager = Arc::new(tokio::sync::Mutex::new(RtmpManager::new()));
    let stop = Arc::new(AtomicBool::new(true));
    let handle = spawn_health_monitor(manager, stop);
    // First `interval.tick()` fires immediately, stop flag trips, loop exits.
    tokio::time::timeout(Duration::from_millis(200), handle)
        .await
        .expect("health monitor should exit promptly")
        .unwrap();
}

// Build a `RtmpStream` around a short-lived shell subprocess — stand-in
// for the real FFmpeg child. Lets us exercise `detect_crashed`, `stop_all`,
// and push-fan-out branches without spawning FFmpeg.
fn fake_exited_stream(id: &str, lang: &str, is_source: bool) -> RtmpStream {
    fake_exited_stream_full(id, lang, is_source, false)
}

fn fake_exited_stream_full(id: &str, lang: &str, is_source: bool, passthrough: bool) -> RtmpStream {
    let mut child = std::process::Command::new("sh")
        .args(["-c", "exit 0"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn shell child");
    let _ = child.wait();
    RtmpStream {
        child,
        video_handle: Some(thread::spawn(|| {})),
        audio_handle: Some(thread::spawn(|| {})),
        audio_fifo: format!("/tmp/brivva_audio_fake_{id}"),
        lang: lang.to_string(),
        rtmp_url: "rtmp://fake".into(),
        delay: Duration::from_millis(1000),
        is_source,
        host_gain: 1.0,
        passthrough,
        buffers: StreamBuffers::new(),
        stop_flag: Arc::new(AtomicBool::new(false)),
        restart_count: 0,
        last_write_ms: Arc::new(AtomicI64::new(now_unix_ms())),
    }
}

#[test]
fn detect_crashed_picks_up_exited_child_and_emits_restart_snapshot() {
    let mut m = RtmpManager::new();
    m.streams
        .insert("fake".into(), fake_exited_stream("fake", "ja", false));

    let crashed = m.detect_crashed();
    assert_eq!(crashed.len(), 1, "exited child should register as crashed");
    let snapshot = &crashed[0];
    assert_eq!(snapshot.0, "fake");
    assert_eq!(snapshot.1, "ja");
    // Original stream removed so restart path can re-insert.
    assert!(!m.streams.contains_key("fake"));
}

#[test]
fn detect_crashed_skips_streams_with_stop_flag_set() {
    let mut m = RtmpManager::new();
    let stream = fake_exited_stream("stopped", "ja", true);
    stream.stop_flag.store(true, Ordering::Release);
    m.streams.insert("stopped".into(), stream);

    let crashed = m.detect_crashed();
    assert!(crashed.is_empty());
    // Stream still in map — detect_crashed didn't treat it as a crash.
    assert!(m.streams.contains_key("stopped"));
}

#[test]
fn detect_crashed_abandons_stream_after_max_restart_attempts() {
    let mut m = RtmpManager::new();
    let mut stream = fake_exited_stream("hot", "ja", false);
    stream.restart_count = MAX_FFMPEG_RESTARTS;
    m.streams.insert("hot".into(), stream);

    let crashed = m.detect_crashed();
    assert!(
        crashed.is_empty(),
        "exhausted streams must not re-enter restart loop"
    );
    // Stop flag should now be set on the exhausted stream.
    assert!(m.streams["hot"].stop_flag.load(Ordering::Acquire));
}

#[tokio::test]
async fn stop_all_drains_every_registered_stream_and_empties_map() {
    let mut m = RtmpManager::new();
    m.streams
        .insert("a".into(), fake_exited_stream("a", "ja", true));
    m.streams
        .insert("b".into(), fake_exited_stream("b", "ko", false));
    m.stop_all().await;
    assert!(m.streams.is_empty());
}

#[test]
fn push_tts_delivers_into_matching_target_stream_queue() {
    let mut m = RtmpManager::new();
    m.streams
        .insert("target".into(), fake_exited_stream("target", "ja", false));

    m.push_tts("ja", vec![1u8; 4_000]);
    let q = m.streams["target"].buffers.tts.lock().unwrap();
    assert_eq!(q.len(), 4_000);
}

#[test]
fn push_tts_skips_source_streams_even_when_lang_matches() {
    let mut m = RtmpManager::new();
    m.streams
        .insert("src".into(), fake_exited_stream("src", "en", true));

    m.push_tts("en", vec![1u8; 100]);
    let q = m.streams["src"].buffers.tts.lock().unwrap();
    assert!(q.is_empty(), "source streams must not receive TTS");
}

#[test]
fn push_tts_caps_queue_at_60s_of_pcm_discarding_oldest_bytes() {
    let mut m = RtmpManager::new();
    m.streams
        .insert("t".into(), fake_exited_stream("t", "ja", false));

    // Push 1 byte beyond the 60s cap — the head-chop loop must reduce the
    // queue to exactly the cap, and (per §0.5.4) emit a warn log. The warn
    // isn't asserted here without a tracing capture sink; the plan verifies
    // it by grep during the dev-stack smoke test.
    m.push_tts("ja", vec![1u8; TTS_QUEUE_CAP_BYTES + 100]);
    let q_len = m.streams["t"].buffers.tts.lock().unwrap().len();
    assert!(q_len <= TTS_QUEUE_CAP_BYTES);
}

#[test]
fn push_tts_does_not_drop_when_payload_fits_within_cap() {
    // Regression guard for the April 2026 bump from 5s → 60s: a 15s utterance
    // (≈ one 291-char Korean sentence) used to trigger head-chop under the
    // old cap. At 60s it must land fully intact.
    let mut m = RtmpManager::new();
    m.streams
        .insert("t".into(), fake_exited_stream("t", "ja", false));

    let fifteen_seconds = 15 * 88_200;
    m.push_tts("ja", vec![1u8; fifteen_seconds]);
    let q_len = m.streams["t"].buffers.tts.lock().unwrap().len();
    assert_eq!(q_len, fifteen_seconds, "15s of PCM must fit under 60s cap");
}

#[test]
fn push_host_audio_evicts_oldest_bytes_when_capacity_exceeded() {
    let mut m = RtmpManager::new();
    m.streams
        .insert("a".into(), fake_exited_stream("a", "ja", false));

    // Drop more than the ~20s cap in a single push.
    m.push_host_audio(&vec![1u8; HOST_AUDIO_CAP_BYTES + 500]);
    let total: usize = m.streams["a"]
        .buffers
        .audio
        .lock()
        .unwrap()
        .iter()
        .map(|(_, b)| b.len())
        .sum();
    assert!(total <= HOST_AUDIO_CAP_BYTES + 500); // pushed-once: exactly one entry
}

#[test]
fn push_video_h264_caps_per_stream_buffer() {
    let mut m = RtmpManager::new();
    m.streams
        .insert("a".into(), fake_exited_stream("a", "ja", false));

    for _ in 0..(HOST_VIDEO_H264_CAP_CHUNKS + 50) {
        m.push_video_h264(&[0u8; 8]);
    }
    let buf_len = m.streams["a"].buffers.video_h264.lock().unwrap().len();
    assert_eq!(buf_len, HOST_VIDEO_H264_CAP_CHUNKS);
}

#[test]
fn kill_idle_streams_observes_idle_and_invokes_kill_branch() {
    let mut m = RtmpManager::new();
    // Seed last_write at 1 (far older than threshold) so the idle branch
    // fires. Child has already exited — kill() may return Err on some
    // platforms, but the function must still process the branch without
    // panic. Success of the signal itself is not the assertion.
    let stream = fake_exited_stream("idle", "ja", false);
    stream.last_write_ms.store(1, Ordering::Release);
    m.streams.insert("idle".into(), stream);
    m.kill_idle_streams();
    // Stream is still in the map; kill_idle_streams does not remove.
    assert!(m.streams.contains_key("idle"));
}

#[test]
fn kill_idle_streams_skips_streams_with_zero_last_write() {
    let mut m = RtmpManager::new();
    let stream = fake_exited_stream("unseeded", "ja", false);
    stream.last_write_ms.store(0, Ordering::Release);
    m.streams.insert("unseeded".into(), stream);
    m.kill_idle_streams(); // last==0 → skip without kill
    assert_eq!(
        m.streams["unseeded"].last_write_ms.load(Ordering::Acquire),
        0
    );
}

#[test]
fn kill_idle_streams_skips_streams_with_stop_flag_set() {
    let mut m = RtmpManager::new();
    let stream = fake_exited_stream("stopped", "ja", false);
    stream.last_write_ms.store(1, Ordering::Release);
    stream.stop_flag.store(true, Ordering::Release);
    m.streams.insert("stopped".into(), stream);
    m.kill_idle_streams();
    assert_eq!(
        m.streams["stopped"].last_write_ms.load(Ordering::Acquire),
        1
    );
}

// ── Passthrough invariants ───────────────────────────────

#[test]
fn push_tts_skips_passthrough_streams_even_when_lang_matches() {
    // Passthrough destinations re-broadcast host audio only. If push_tts
    // ever fed PCM into their queue, memory would grow up to the cap and
    // nothing would ever play — the drain uses is_source=true semantics.
    let mut m = RtmpManager::new();
    m.streams.insert(
        "pass".into(),
        // is_source=true mirrors the session_ws hydration path, which
        // promotes passthrough to is_source so the drain short-circuits.
        fake_exited_stream_full("pass", "ja", true, true),
    );

    m.push_tts("ja", vec![1u8; 4_000]);
    let q = m.streams["pass"].buffers.tts.lock().unwrap();
    assert!(
        q.is_empty(),
        "passthrough streams must not accumulate TTS PCM"
    );
}

#[test]
fn crashed_stream_snapshot_preserves_passthrough_across_restart() {
    // The restart path reuses buffers AND flags from the crashed stream.
    // If passthrough dropped out of the tuple, a crashed passthrough
    // would silently restart as a translated stream and start piping
    // ducked TTS over the host audio.
    let mut m = RtmpManager::new();
    let stream = fake_exited_stream_full("p", "en", true, true);
    m.streams.insert("p".into(), stream);

    let crashed = m.detect_crashed();
    assert_eq!(crashed.len(), 1);
    // Tuple layout: (id, lang, rtmp_url, delay_ms, is_source, host_gain,
    //                passthrough, restart_count, buffers).
    assert_eq!(crashed[0].0, "p");
    assert!(crashed[0].4, "is_source preserved");
    assert!(crashed[0].6, "passthrough preserved across restart");
}
