//! Per-stream video + audio drain loops.
//!
//! These run on their own OS threads. They pull aged frames/chunks out of
//! the stream's delay buffers and feed them to the FFmpeg child — video
//! straight to stdin at 30 fps by repeating the latest browser JPEG when
//! necessary, audio to a FIFO at 20 ms ticks mixed with the translated TTS
//! queue.

use std::collections::VecDeque;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

use super::mixer::{apply_gain, mix_pcm_s16le};
use crate::features::broadcast::domain::SessionMetrics;

/// Video: 33.33 ms per frame at 30 fps.
const FRAME_INTERVAL: Duration = Duration::from_nanos(33_333_333);
/// Audio: 20 ms per tick.
const AUDIO_TICK: Duration = Duration::from_millis(20);
/// Audio: 1764 bytes = 44.1 kHz × 20 ms × s16le mono.
const AUDIO_BYTES_PER_TICK: usize = 1764;

type TimedChunk = (Instant, Vec<u8>);

// ── Video ─────────────────────────────────────────────────

pub(super) struct VideoDrainCtx {
    pub stream_id: String,
    pub video_buf: Arc<StdMutex<VecDeque<TimedChunk>>>,
    pub stdin: std::process::ChildStdin,
    pub delay: Duration,
    pub stop: Arc<AtomicBool>,
    /// Unix-ms wall clock updated after each successful write. Health monitor
    /// reads this to detect silent FFmpeg stalls on long live shows.
    pub last_write_ms: Arc<AtomicI64>,
    pub metrics: Option<Arc<SessionMetrics>>,
}

pub(super) fn video_drain_loop(ctx: VideoDrainCtx) {
    let VideoDrainCtx {
        stream_id,
        video_buf,
        mut stdin,
        delay,
        stop,
        last_write_ms,
        metrics,
    } = ctx;
    let mut last_frame: Option<Vec<u8>> = None;
    let mut tick_count: u64 = 0;
    let mut next_tick = Instant::now() + FRAME_INTERVAL;

    eprintln!(
        "[VIDEO:{}] drain started (30 fps, delay={}ms)",
        stream_id,
        delay.as_millis()
    );

    while !stop.load(Ordering::Acquire) {
        let actual = wait_video_tick(&mut next_tick);
        tick_count += 1;

        let ready = drain_ready_frame(&video_buf, actual, delay);
        let to_write = match ready {
            Some(f) => {
                last_frame = Some(f.clone());
                Some(f)
            }
            None => last_frame.clone(),
        };

        if let Some(f) = to_write {
            let n = f.len() as u64;
            if stdin.write_all(&f).is_err() {
                if !stop.load(Ordering::Acquire) {
                    eprintln!("[VIDEO:{}] write error, exiting", stream_id);
                }
                break;
            }
            last_write_ms.store(now_unix_ms(), Ordering::Release);
            if let Some(m) = &metrics {
                m.record_bytes_out(n);
            }
        }
    }

    drop(stdin);
    eprintln!(
        "[VIDEO:{}] drain exited after {} ticks",
        stream_id, tick_count
    );
}

fn wait_video_tick(next_tick: &mut Instant) -> Instant {
    let now = Instant::now();
    if *next_tick > now {
        thread::sleep(*next_tick - now);
    }
    let actual = Instant::now();
    *next_tick += FRAME_INTERVAL;
    actual
}

fn drain_ready_frame(
    video_buf: &StdMutex<VecDeque<TimedChunk>>,
    now: Instant,
    delay: Duration,
) -> Option<Vec<u8>> {
    let mut buf = video_buf.lock().unwrap();
    let mut latest: Option<Vec<u8>> = None;
    while let Some((ts, _)) = buf.front() {
        if *ts + delay <= now {
            let (_, f) = buf.pop_front().unwrap();
            latest = Some(f);
        } else {
            break;
        }
    }
    latest
}

// ── Audio ─────────────────────────────────────────────────

pub(super) struct AudioDrainCtx {
    pub stream_id: String,
    pub host_buf: Arc<StdMutex<VecDeque<TimedChunk>>>,
    pub tts_queue: Arc<StdMutex<VecDeque<u8>>>,
    pub fifo_path: String,
    pub delay: Duration,
    pub is_source: bool,
    pub host_gain: f32,
    pub stop: Arc<AtomicBool>,
    /// Unix-ms wall clock updated after each successful FIFO write.
    pub last_write_ms: Arc<AtomicI64>,
    pub metrics: Option<Arc<SessionMetrics>>,
}

pub(super) fn audio_drain_loop(ctx: AudioDrainCtx) {
    let AudioDrainCtx {
        stream_id,
        host_buf,
        tts_queue,
        fifo_path,
        delay,
        is_source,
        host_gain,
        stop,
        last_write_ms,
        metrics,
    } = ctx;

    let mut fifo = match open_audio_fifo(&fifo_path, &stream_id, &stop) {
        Some(f) => f,
        None => return,
    };

    let silence_chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
    let mut ready_host: Vec<u8> = Vec::with_capacity(AUDIO_BYTES_PER_TICK * 4);
    let mut tick_count: u64 = 0;
    let mut next_tick = Instant::now() + AUDIO_TICK;

    eprintln!(
        "[AUDIO:{}] drain started (20 ms, delay={}ms, source={}, host_gain={:.2})",
        stream_id,
        delay.as_millis(),
        is_source,
        host_gain
    );

    while !stop.load(Ordering::Acquire) {
        let actual = wait_audio_tick(&mut next_tick);
        tick_count += 1;

        drain_aged_host_audio(DrainHostArgs {
            host_buf: &host_buf,
            ready_host: &mut ready_host,
            now: actual,
            delay,
        });
        let host_chunk = take_tick_sample(&mut ready_host);
        let output = build_tick_output(TickOutputArgs {
            host_chunk,
            is_source,
            host_gain,
            tts_queue: &tts_queue,
        });

        let bytes = if output.is_empty() {
            &silence_chunk
        } else {
            &output
        };
        let n = bytes.len() as u64;
        match fifo_write_nonblocking(&mut fifo, bytes, &stop) {
            FifoWrite::Ok => {
                last_write_ms.store(now_unix_ms(), Ordering::Release);
                if let Some(m) = &metrics {
                    m.record_bytes_out(n);
                }
            }
            FifoWrite::Stopped => break,
            FifoWrite::Err => {
                if !stop.load(Ordering::Acquire) {
                    eprintln!("[AUDIO:{}] write error, exiting", stream_id);
                }
                break;
            }
        }
    }

    drop(fifo);
    eprintln!(
        "[AUDIO:{}] drain exited after {} ticks",
        stream_id, tick_count
    );
}

fn open_audio_fifo(path: &str, stream_id: &str, stop: &AtomicBool) -> Option<std::fs::File> {
    match std::fs::OpenOptions::new().write(true).open(path) {
        Ok(f) => {
            // Switch the writer fd to non-blocking so a dead reader (ffmpeg
            // killed mid-stream) cannot deadlock the drain thread on the next
            // tick. The drain loop polls stop_flag between WouldBlock retries.
            unsafe {
                let fd = f.as_raw_fd();
                let flags = libc::fcntl(fd, libc::F_GETFL);
                if flags >= 0 {
                    libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
            }
            Some(f)
        }
        Err(e) => {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[AUDIO:{}] failed to open FIFO: {}", stream_id, e);
            }
            None
        }
    }
}

enum FifoWrite {
    Ok,
    Stopped,
    Err,
}

/// Write all bytes to a non-blocking FIFO. On WouldBlock, sleeps briefly and
/// rechecks `stop` so the drain thread reacts to shutdown within ~1ms even if
/// the FIFO buffer is full. Returns `Stopped` if the flag flips mid-write.
fn fifo_write_nonblocking(fifo: &mut std::fs::File, bytes: &[u8], stop: &AtomicBool) -> FifoWrite {
    let mut written = 0usize;
    while written < bytes.len() {
        if stop.load(Ordering::Acquire) {
            return FifoWrite::Stopped;
        }
        match fifo.write(&bytes[written..]) {
            Ok(0) => return FifoWrite::Err,
            Ok(n) => written += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(_) => return FifoWrite::Err,
        }
    }
    FifoWrite::Ok
}

fn wait_audio_tick(next_tick: &mut Instant) -> Instant {
    let now = Instant::now();
    if *next_tick > now {
        thread::sleep(*next_tick - now);
    }
    let actual = Instant::now();
    *next_tick += AUDIO_TICK;
    actual
}

struct DrainHostArgs<'a> {
    host_buf: &'a StdMutex<VecDeque<TimedChunk>>,
    ready_host: &'a mut Vec<u8>,
    now: Instant,
    delay: Duration,
}

fn drain_aged_host_audio(args: DrainHostArgs<'_>) {
    let mut buf = args.host_buf.lock().unwrap();
    while let Some((ts, _)) = buf.front() {
        if *ts + args.delay <= args.now {
            let (_, pcm) = buf.pop_front().unwrap();
            args.ready_host.extend_from_slice(&pcm);
        } else {
            break;
        }
    }
}

fn take_tick_sample(ready_host: &mut Vec<u8>) -> Vec<u8> {
    let take = AUDIO_BYTES_PER_TICK.min(ready_host.len());
    let mut chunk: Vec<u8> = ready_host.drain(..take).collect();
    if chunk.len() < AUDIO_BYTES_PER_TICK {
        chunk.resize(AUDIO_BYTES_PER_TICK, 0);
    }
    chunk
}

struct TickOutputArgs<'a> {
    host_chunk: Vec<u8>,
    is_source: bool,
    host_gain: f32,
    tts_queue: &'a StdMutex<VecDeque<u8>>,
}

fn build_tick_output(args: TickOutputArgs<'_>) -> Vec<u8> {
    let TickOutputArgs {
        host_chunk,
        is_source,
        host_gain,
        tts_queue,
    } = args;
    if is_source {
        // Source streams never queue TTS — skip the mix entirely.
        if (host_gain - 1.0).abs() < f32::EPSILON {
            host_chunk
        } else {
            apply_gain(&host_chunk, host_gain)
        }
    } else {
        let mut tts_padded: Vec<u8> = {
            let mut q = tts_queue.lock().unwrap();
            let n = AUDIO_BYTES_PER_TICK.min(q.len());
            q.drain(..n).collect()
        };
        if tts_padded.len() < AUDIO_BYTES_PER_TICK {
            tts_padded.resize(AUDIO_BYTES_PER_TICK, 0);
        }
        mix_pcm_s16le(&host_chunk, host_gain, &tts_padded, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;
    use std::time::{Duration, Instant};

    #[test]
    fn take_tick_sample_drains_full_buffer_and_pads_with_zeros() {
        let mut ready = vec![1u8; 100];
        let sample = take_tick_sample(&mut ready);

        assert_eq!(sample.len(), AUDIO_BYTES_PER_TICK);
        assert_eq!(&sample[..100], &vec![1u8; 100][..]);
        assert!(sample[100..].iter().all(|&b| b == 0));
        assert!(ready.is_empty());
    }

    #[test]
    fn take_tick_sample_takes_exactly_one_tick_when_buffer_has_more() {
        let mut ready = vec![2u8; AUDIO_BYTES_PER_TICK + 500];
        let sample = take_tick_sample(&mut ready);

        assert_eq!(sample.len(), AUDIO_BYTES_PER_TICK);
        assert_eq!(sample, vec![2u8; AUDIO_BYTES_PER_TICK]);
        assert_eq!(ready.len(), 500);
    }

    #[test]
    fn take_tick_sample_returns_zero_filled_tick_when_ready_is_empty() {
        let mut ready: Vec<u8> = Vec::new();
        let sample = take_tick_sample(&mut ready);
        assert_eq!(sample, vec![0u8; AUDIO_BYTES_PER_TICK]);
    }

    #[test]
    fn build_tick_output_source_stream_bypasses_mix_at_unit_gain() {
        let q: StdMutex<VecDeque<u8>> = StdMutex::new(VecDeque::new());
        let host = vec![0x10, 0x27];
        let out = build_tick_output(TickOutputArgs {
            host_chunk: host.clone(),
            is_source: true,
            host_gain: 1.0,
            tts_queue: &q,
        });
        assert_eq!(out, host);
    }

    #[test]
    fn build_tick_output_source_stream_applies_gain_when_not_unit() {
        let q: StdMutex<VecDeque<u8>> = StdMutex::new(VecDeque::new());
        let out = build_tick_output(TickOutputArgs {
            host_chunk: vec![0x10, 0x27], // 10_000 → *0.5 = 5_000
            is_source: true,
            host_gain: 0.5,
            tts_queue: &q,
        });
        let sample = i16::from_le_bytes([out[0], out[1]]);
        assert_eq!(sample, 5_000);
    }

    #[test]
    fn build_tick_output_target_stream_mixes_host_with_tts_queue() {
        let mut q: VecDeque<u8> = VecDeque::new();
        // 5_000 (le) as a single sample in the TTS queue, then empty → resize
        // to full tick with zeros.
        q.extend(5_000_i16.to_le_bytes());
        let queue = StdMutex::new(q);

        let host_chunk = vec![0x10, 0x27]; // 10_000
        let out = build_tick_output(TickOutputArgs {
            host_chunk,
            is_source: false,
            host_gain: 0.2,
            tts_queue: &queue,
        });
        // host 10_000 * 0.2 = 2_000; plus TTS 5_000 = 7_000.
        assert_eq!(i16::from_le_bytes([out[0], out[1]]), 7_000);
        // Remaining bytes padded to tick width with tts zeros + host zeros.
        assert_eq!(out.len(), AUDIO_BYTES_PER_TICK.min(2));
    }

    #[test]
    fn build_tick_output_target_stream_drains_up_to_one_tick_from_tts_queue() {
        let queue = StdMutex::new(VecDeque::from(vec![0u8; AUDIO_BYTES_PER_TICK * 2]));
        let host_chunk = vec![0u8; AUDIO_BYTES_PER_TICK];
        let _ = build_tick_output(TickOutputArgs {
            host_chunk,
            is_source: false,
            host_gain: 0.2,
            tts_queue: &queue,
        });
        assert_eq!(queue.lock().unwrap().len(), AUDIO_BYTES_PER_TICK);
    }

    #[test]
    fn drain_ready_frame_returns_most_recent_aged_frame() {
        let buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        {
            let mut b = buf.lock().unwrap();
            b.push_back((now - Duration::from_millis(500), vec![1]));
            b.push_back((now - Duration::from_millis(200), vec![2]));
            b.push_back((now + Duration::from_millis(500), vec![3]));
        }
        let result = drain_ready_frame(&buf, now, delay);
        assert_eq!(result, Some(vec![2]));
        assert_eq!(buf.lock().unwrap().len(), 1);
    }

    #[test]
    fn drain_ready_frame_returns_none_when_nothing_has_aged() {
        let buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        {
            let mut b = buf.lock().unwrap();
            b.push_back((now + Duration::from_millis(10), vec![1]));
        }
        let result = drain_ready_frame(&buf, now, Duration::from_millis(100));
        assert_eq!(result, None);
        assert_eq!(buf.lock().unwrap().len(), 1);
    }

    #[test]
    fn drain_aged_host_audio_extends_ready_buffer_only_with_aged_chunks() {
        let host_buf = Arc::new(StdMutex::new(VecDeque::new()));
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        {
            let mut b = host_buf.lock().unwrap();
            b.push_back((now - Duration::from_millis(500), vec![0xaa]));
            b.push_back((now - Duration::from_millis(150), vec![0xbb]));
            b.push_back((now + Duration::from_millis(50), vec![0xcc]));
        }
        let mut ready = Vec::new();
        drain_aged_host_audio(DrainHostArgs {
            host_buf: &host_buf,
            ready_host: &mut ready,
            now,
            delay,
        });
        assert_eq!(ready, vec![0xaa, 0xbb]);
        assert_eq!(host_buf.lock().unwrap().len(), 1);
    }

    #[test]
    fn wait_audio_tick_advances_next_tick_by_one_audio_tick() {
        let start = Instant::now();
        let mut next = start + Duration::from_millis(1);
        let actual = wait_audio_tick(&mut next);
        assert!(actual >= start);
        assert_eq!(next - start, Duration::from_millis(1) + AUDIO_TICK);
    }

    #[test]
    fn wait_video_tick_advances_next_tick_by_one_frame_interval() {
        let start = Instant::now();
        let mut next = start + Duration::from_nanos(1);
        wait_video_tick(&mut next);
        assert_eq!(next - start, Duration::from_nanos(1) + FRAME_INTERVAL);
    }

    #[test]
    fn open_audio_fifo_returns_none_when_path_does_not_exist() {
        let stop = AtomicBool::new(false);
        let out = open_audio_fifo(
            "/tmp/definitely_not_a_real_brivva_fifo_0xdeadbeef",
            "sid",
            &stop,
        );
        assert!(out.is_none());
    }

    #[test]
    fn open_audio_fifo_returns_none_silently_when_stop_flag_already_set() {
        // stop=true suppresses the stderr log but still returns None on error.
        let stop = AtomicBool::new(true);
        let out = open_audio_fifo(
            "/tmp/definitely_not_a_real_brivva_fifo_0xcafebabe",
            "sid",
            &stop,
        );
        assert!(out.is_none());
    }

    #[test]
    fn open_audio_fifo_returns_some_when_path_is_writable() {
        let path =
            std::env::temp_dir().join(format!("brivva_drain_test_{}.bin", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        let path_str = path.to_str().unwrap();
        let stop = AtomicBool::new(false);
        let file = open_audio_fifo(path_str, "sid", &stop);
        assert!(file.is_some());
        drop(file);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn audio_drain_loop_exits_immediately_when_stop_flag_is_already_set() {
        // Use a real file as the "fifo" so open succeeds, then preset stop.
        let path = std::env::temp_dir().join(format!(
            "brivva_audio_drain_stop_{}.bin",
            std::process::id()
        ));
        std::fs::write(&path, b"").unwrap();

        let ctx = AudioDrainCtx {
            stream_id: "sid".into(),
            host_buf: Arc::new(StdMutex::new(VecDeque::new())),
            tts_queue: Arc::new(StdMutex::new(VecDeque::new())),
            fifo_path: path.to_str().unwrap().into(),
            delay: Duration::from_millis(0),
            is_source: false,
            host_gain: 1.0,
            stop: Arc::new(AtomicBool::new(true)),
            last_write_ms: Arc::new(AtomicI64::new(0)),
            metrics: None,
        };
        audio_drain_loop(ctx); // should return without entering the tick loop
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn audio_drain_loop_exits_when_fifo_cannot_be_opened() {
        // Nonexistent path; open_audio_fifo returns None and drain returns.
        let ctx = AudioDrainCtx {
            stream_id: "sid".into(),
            host_buf: Arc::new(StdMutex::new(VecDeque::new())),
            tts_queue: Arc::new(StdMutex::new(VecDeque::new())),
            fifo_path: "/tmp/brivva_audio_missing_0xfeedface".into(),
            delay: Duration::from_millis(0),
            is_source: true,
            host_gain: 1.0,
            stop: Arc::new(AtomicBool::new(false)),
            last_write_ms: Arc::new(AtomicI64::new(0)),
            metrics: None,
        };
        audio_drain_loop(ctx);
    }

    #[test]
    fn video_drain_loop_exits_when_stdin_write_fails() {
        // Spawn a short-lived child, take its stdin, let it exit so writes
        // fail immediately. The drain loop must detect the broken pipe and
        // exit without hanging.
        use std::process::Command;
        use std::process::Stdio;
        let mut child = Command::new("sh")
            .args(["-c", "exit 0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn short-lived child");
        let stdin = child.stdin.take().expect("stdin piped");
        // Wait for child to exit so write fails reliably.
        let _ = child.wait();

        let buf: Arc<StdMutex<VecDeque<TimedChunk>>> = Arc::new(StdMutex::new(VecDeque::new()));
        // Push a frame so the loop has something to write on the first tick.
        buf.lock()
            .unwrap()
            .push_back((Instant::now() - Duration::from_secs(1), vec![1, 2, 3]));

        let ctx = VideoDrainCtx {
            stream_id: "sid".into(),
            video_buf: buf,
            stdin,
            delay: Duration::from_millis(0),
            stop: Arc::new(AtomicBool::new(false)),
            last_write_ms: Arc::new(AtomicI64::new(0)),
            metrics: None,
        };
        video_drain_loop(ctx);
    }

    #[test]
    fn video_drain_loop_exits_immediately_when_stop_flag_preset() {
        use std::process::Command;
        use std::process::Stdio;
        let mut child = Command::new("sh")
            .args(["-c", "sleep 1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleeping child");
        let stdin = child.stdin.take().unwrap();

        let buf = Arc::new(StdMutex::new(VecDeque::new()));
        let ctx = VideoDrainCtx {
            stream_id: "sid".into(),
            video_buf: buf,
            stdin,
            delay: Duration::from_millis(0),
            stop: Arc::new(AtomicBool::new(true)),
            last_write_ms: Arc::new(AtomicI64::new(0)),
            metrics: None,
        };
        video_drain_loop(ctx);
        let _ = child.kill();
        let _ = child.wait();
    }
}
