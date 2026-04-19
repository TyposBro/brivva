//! Per-stream video + audio drain loops.
//!
//! These run on their own OS threads. They pull aged frames/chunks out of
//! the stream's delay buffers and feed them to the FFmpeg child — video
//! straight to stdin at 30 fps, audio to a FIFO at 20 ms ticks mixed with
//! the translated TTS queue.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};

use super::mixer::{apply_gain, mix_pcm_s16le};

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
}

pub(super) fn video_drain_loop(ctx: VideoDrainCtx) {
    let VideoDrainCtx {
        stream_id,
        video_buf,
        mut stdin,
        delay,
        stop,
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

        if let Some(f) = to_write
            && stdin.write_all(&f).is_err()
        {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[VIDEO:{}] write error, exiting", stream_id);
            }
            break;
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
        if fifo.write_all(bytes).is_err() {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[AUDIO:{}] write error, exiting", stream_id);
            }
            break;
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
        Ok(f) => Some(f),
        Err(e) => {
            if !stop.load(Ordering::Acquire) {
                eprintln!("[AUDIO:{}] failed to open FIFO: {}", stream_id, e);
            }
            None
        }
    }
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
