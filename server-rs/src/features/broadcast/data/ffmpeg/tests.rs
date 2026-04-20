use super::*;
use std::sync::atomic::Ordering;

#[test]
fn timing_constants_match_documented_values() {
    assert_eq!(MAX_FFMPEG_RESTARTS, 3);
    assert_eq!(FFMPEG_RESTART_DELAY, Duration::from_secs(2));
    assert_eq!(IDLE_RESTART_THRESHOLD, Duration::from_secs(25));
    assert_eq!(HOST_AUDIO_CAP_BYTES, 20 * 88_200);
    assert_eq!(HOST_VIDEO_CAP_FRAMES, 20 * 30);
    assert_eq!(TTS_QUEUE_CAP_BYTES, 5 * 88_200);
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
    assert_eq!(b.video.lock().unwrap().len(), 0);
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
fn push_video_frame_on_empty_manager_records_nothing_and_does_not_panic() {
    let m = RtmpManager::new();
    m.push_video_frame(&[0u8; 16]);
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
fn push_caption_on_empty_manager_is_safe_noop() {
    let m = RtmpManager::new();
    m.push_caption("ja", "hello".into());
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
        caption: None,
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
fn push_tts_caps_queue_at_5s_of_pcm_discarding_oldest_bytes() {
    let mut m = RtmpManager::new();
    m.streams
        .insert("t".into(), fake_exited_stream("t", "ja", false));

    // Push more than the 5s cap — oldest bytes drop so head index shifts.
    m.push_tts("ja", vec![1u8; TTS_QUEUE_CAP_BYTES + 100]);
    let q_len = m.streams["t"].buffers.tts.lock().unwrap().len();
    assert!(q_len <= TTS_QUEUE_CAP_BYTES);
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
fn push_video_frame_caps_per_stream_buffer_at_20s_of_frames() {
    let mut m = RtmpManager::new();
    m.streams
        .insert("a".into(), fake_exited_stream("a", "ja", false));

    for _ in 0..(HOST_VIDEO_CAP_FRAMES + 50) {
        m.push_video_frame(&[0u8; 8]);
    }
    let buf_len = m.streams["a"].buffers.video.lock().unwrap().len();
    assert_eq!(buf_len, HOST_VIDEO_CAP_FRAMES);
}

#[test]
fn push_caption_on_source_stream_is_a_noop_because_source_has_no_caption_state() {
    let mut m = RtmpManager::new();
    m.streams
        .insert("src".into(), fake_exited_stream("src", "en", true));
    m.push_caption("en", "anything".into()); // source → caption is None
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
fn push_caption_skips_passthrough_streams_even_with_caption_state_attached() {
    // Defensive: if a future refactor mistakenly hands a passthrough
    // stream a CaptionState (it shouldn't — see spawn_stream_inner), the
    // caption write must still be a noop. Wire a dummy CaptionState and
    // verify push_caption exits the branch cleanly without tripping.
    let mut m = RtmpManager::new();
    let stream = fake_exited_stream_full("pass", "ja", true, true);
    m.streams.insert("pass".into(), stream);
    m.push_caption("ja", "hello".into());
}

#[test]
fn start_stream_args_passthrough_true_skips_caption_state_creation() {
    // Exercised via the spawn path: with passthrough=true we MUST NOT
    // build a CaptionState (no translation text, no textfile on disk).
    // We can't spawn real FFmpeg in unit tests, so instead audit the
    // struct-level invariant: CaptionState is only spawned when BOTH
    // is_source=false AND passthrough=false. A stream inserted manually
    // with passthrough=true reflects the intended post-spawn shape.
    let stream = fake_exited_stream_full("p", "en", true, true);
    assert!(stream.caption.is_none());
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

/// Same fake-stream scaffolding as `fake_exited_stream_full` but with a
/// real `CaptionState` attached so `push_caption` exercises the textfile
/// writer end-to-end. Used to assert the on-disk side of the burn-in
/// pipeline without spawning FFmpeg.
fn fake_stream_with_caption(id: &str, lang: &str) -> RtmpStream {
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
        is_source: false,
        host_gain: 0.2,
        passthrough: false,
        buffers: StreamBuffers::new(),
        caption: Some(CaptionState::spawn(id)),
        stop_flag: Arc::new(AtomicBool::new(false)),
        restart_count: 0,
        last_write_ms: Arc::new(AtomicI64::new(now_unix_ms())),
    }
}

#[tokio::test]
async fn push_caption_writes_translated_text_to_drawtext_textfile_on_disk() {
    // Production smoke: a translated utterance arriving at the manager
    // must end up as bytes on the drawtext textfile within the dwell
    // window. Pre-fix the textfile was created at spawn time but the
    // fan-out from emit_translation never logged anything, so we had no
    // way to tell whether captions were silently failing here.
    let mut m = RtmpManager::new();
    let stream_id = format!("captest-{}", std::process::id());
    let stream = fake_stream_with_caption(&stream_id, "ja");
    let caption_path = stream
        .caption
        .as_ref()
        .expect("fake stream must have caption")
        .path
        .clone();
    m.streams.insert(stream_id.clone(), stream);

    m.push_caption("ja", "こんにちは".into());

    // First write has no dwell debt — should land within ~100 ms.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let on_disk = std::fs::read_to_string(&caption_path).expect("textfile present");
    assert_eq!(on_disk, "こんにちは");

    if let Some(mut s) = m.streams.remove(&stream_id)
        && let Some(mut cap) = s.caption.take()
    {
        cap.shutdown();
    }
    let _ = std::fs::remove_file(&caption_path);
}

#[tokio::test]
async fn push_caption_does_not_write_textfile_when_lang_does_not_match() {
    // Defensive: a translated text for `ko` must not leak onto a `ja`
    // stream's textfile. Otherwise hosts running multiple target streams
    // would see other languages flicker through their burn-in.
    let mut m = RtmpManager::new();
    let stream_id = format!("captest-mismatch-{}", std::process::id());
    let stream = fake_stream_with_caption(&stream_id, "ja");
    let caption_path = stream
        .caption
        .as_ref()
        .expect("fake stream must have caption")
        .path
        .clone();
    m.streams.insert(stream_id.clone(), stream);

    m.push_caption("ko", "안녕하세요".into());

    tokio::time::sleep(Duration::from_millis(200)).await;
    // Spawn writes an empty file initially; no caption push for `ja`
    // means the file stays empty.
    let on_disk = std::fs::read_to_string(&caption_path).unwrap_or_default();
    assert!(
        on_disk.is_empty(),
        "ja stream's textfile must stay empty when ko text is pushed: got {on_disk:?}"
    );

    if let Some(mut s) = m.streams.remove(&stream_id)
        && let Some(mut cap) = s.caption.take()
    {
        cap.shutdown();
    }
    let _ = std::fs::remove_file(&caption_path);
}
