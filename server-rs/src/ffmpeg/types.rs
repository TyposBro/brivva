use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

// ── Constants ─────────────────────────────────────────────

/// Audio: 20ms per tick
pub(crate) const AUDIO_TICK: Duration = Duration::from_millis(20);
/// Audio bytes per 20ms tick: 44100Hz × 2 bytes/sample × 1 channel × 0.02s = 1764 bytes
pub(crate) const AUDIO_BYTES_PER_TICK: usize = 1764;
/// Max video chunks to buffer (~60s at 10 chunks/sec)
pub(super) const MAX_VIDEO_CHUNKS: usize = 600;
/// Default broadcast delay (5s gives chunked utterances enough pipeline budget)
pub(super) const DEFAULT_DELAY_MS: u64 = 3000;
/// Max FFmpeg restart attempts per stream
pub(super) const MAX_FFMPEG_RESTARTS: u32 = 50;
/// Delay between FFmpeg restart attempts
pub(super) const FFMPEG_RESTART_DELAY: Duration = Duration::from_secs(2);
/// Jitter warning threshold (100ms avoids log spam)
pub(crate) const JITTER_WARN_THRESHOLD: Duration = Duration::from_millis(100);
/// Jitter recovery threshold — if we fall this far behind, reset the tick anchor
/// rather than trying to catch up (which causes a cascade of late writes).
pub(crate) const JITTER_RECOVERY_THRESHOLD: Duration = Duration::from_millis(500);
/// Fade-out duration in bytes: 50ms at 44100Hz mono 16-bit = 4410 bytes
const FADE_OUT_BYTES: usize = 4410;
/// Max ticks to write during jitter recovery (2s). Beyond this the stream is
/// already broken; dumping megabytes of silence only crashes RTMP.
pub(crate) const MAX_RECOVERY_TICKS: usize = 100;

// ── Shared Types ──────────────────────────────────────────

/// Shared growing PCM buffer for streaming TTS audio.
/// TTS writes chunks as they arrive; the audio drain reads from the same buffer.
#[derive(Clone)]
pub struct StreamingPcm {
    pub pcm: Arc<StdMutex<Vec<u8>>>,
    pub complete: Arc<AtomicBool>,
}

impl StreamingPcm {
    pub fn new() -> Self {
        Self {
            pcm: Arc::new(StdMutex::new(Vec::new())),
            complete: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Append a PCM chunk (called from TTS as each WebSocket chunk arrives)
    pub fn append(&self, data: &[u8]) {
        let mut buf = self.pcm.lock().unwrap();
        buf.extend_from_slice(data);
    }

    /// Mark the stream as complete (all TTS chunks received or error)
    pub fn finish(&self) {
        self.complete.store(true, Ordering::Release);
    }

    /// Append data and apply truncation + fadeout if over max_bytes
    pub fn append_with_limit(&self, data: &[u8], max_bytes: usize) {
        let mut buf = self.pcm.lock().unwrap();
        let remaining_capacity = max_bytes.saturating_sub(buf.len());
        if remaining_capacity == 0 {
            return;
        }
        let to_add = data.len().min(remaining_capacity);
        buf.extend_from_slice(&data[..to_add]);
        if buf.len() >= max_bytes {
            truncate_with_fadeout(&mut buf, max_bytes);
            drop(buf);
            self.complete.store(true, Ordering::Release);
        }
    }
}

/// Audio waiting to be played at the right point in the delayed timeline
pub(crate) struct QueuedAudio {
    /// Source timestamp when this utterance started (host speaking)
    pub(crate) play_at: Instant,
    /// Shared PCM buffer (may still be growing if TTS is streaming)
    pub(crate) pcm: Arc<StdMutex<Vec<u8>>>,
    /// True when all audio data has been written
    pub(crate) complete: Arc<AtomicBool>,
}

// ── Shared Helpers ─────────────────────────────────────────

/// Truncate PCM audio to max_bytes and apply a 50ms fade-out at the cut point.
/// Operates on s16le (16-bit signed little-endian, mono) samples.
pub fn truncate_with_fadeout(pcm: &mut Vec<u8>, max_bytes: usize) {
    if pcm.len() <= max_bytes {
        return;
    }
    pcm.truncate(max_bytes);

    // Apply linear fade-out to the last FADE_OUT_BYTES
    let fade_bytes = FADE_OUT_BYTES.min(pcm.len());
    let fade_start = pcm.len() - fade_bytes;
    let fade_samples = fade_bytes / 2; // 16-bit = 2 bytes per sample

    for i in 0..fade_samples {
        let byte_offset = fade_start + i * 2;
        if byte_offset + 1 >= pcm.len() {
            break;
        }
        let sample = i16::from_le_bytes([pcm[byte_offset], pcm[byte_offset + 1]]);
        // Linear fade: 1.0 at start of fade region → 0.0 at end
        let gain = 1.0 - (i as f32 / fade_samples as f32);
        let faded = (sample as f32 * gain) as i16;
        let bytes = faded.to_le_bytes();
        pcm[byte_offset] = bytes[0];
        pcm[byte_offset + 1] = bytes[1];
    }
}
