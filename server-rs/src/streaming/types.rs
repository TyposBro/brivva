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
/// Default broadcast delay (3s gives chunked utterances enough pipeline budget)
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

// ── FFmpeg Encoding ─────────────────────────────────────

/// H.264 Constant Rate Factor (quality; lower = better, 23 is ffmpeg default)
pub(super) const VIDEO_CRF: &str = "23";
/// Max video bitrate
pub(super) const VIDEO_MAX_BITRATE: &str = "8000k";
/// Video rate control buffer size
pub(super) const VIDEO_BUFSIZE: &str = "16000k";
/// GOP (Group of Pictures) size — keyframe interval
pub(super) const VIDEO_GOP_SIZE: &str = "60";
/// Output audio bitrate for AAC encoding
pub(super) const AUDIO_BITRATE: &str = "128k";
/// Output audio channels (stereo for RTMP)
pub(super) const AUDIO_CHANNELS_OUT: &str = "2";

// ── Stream Lifecycle ────────────────────────────────────

/// Timeout for joining drain threads during cleanup
pub(super) const THREAD_JOIN_TIMEOUT_SECS: u64 = 3;
/// Health check interval for crash detection
pub(super) const HEALTH_CHECK_INTERVAL_SECS: u64 = 2;

// ── Video Drain ─────────────────────────────────────────

/// Video chunk poll interval
pub(super) const VIDEO_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);
/// Log stats every N video chunks (~10s at 10 chunks/sec)
pub(super) const VIDEO_STATS_INTERVAL: u64 = 100;
/// Extra margin when flushing stale chunks on restart
pub(super) const STALE_CHUNK_MARGIN_SECS: u64 = 1;

// ── Audio Drain ─────────────────────────────────────────

/// Drift warning threshold in milliseconds
pub(crate) const DRIFT_WARN_THRESHOLD_MS: u64 = 50;
/// Check drift every N ticks (~5 seconds at 20ms ticks)
pub(crate) const DRIFT_CHECK_INTERVAL_TICKS: u64 = 250;
/// Rate-limit jitter warnings: log every Nth occurrence
pub(crate) const JITTER_WARN_LOG_INTERVAL: u64 = 25;

// ── Shared Types ──────────────────────────────────────────

/// Shared growing PCM buffer for streaming TTS audio.
/// TTS writes chunks as they arrive; the audio drain reads from the same buffer.
#[derive(Clone)]
pub struct StreamingPcm {
    pub pcm: Arc<StdMutex<Vec<u8>>>,
    pub complete: Arc<AtomicBool>,
}

impl Default for StreamingPcm {
    fn default() -> Self {
        Self::new()
    }
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
    apply_linear_fadeout(pcm);
}

/// Apply a linear fade-out over the last FADE_OUT_BYTES of a PCM buffer.
fn apply_linear_fadeout(pcm: &mut [u8]) {
    let fade_bytes = FADE_OUT_BYTES.min(pcm.len());
    let fade_start = pcm.len() - fade_bytes;
    let fade_samples = fade_bytes / 2;

    for i in 0..fade_samples {
        fade_sample(pcm, fade_start + i * 2, i, fade_samples);
    }
}

/// Attenuate a single 16-bit LE sample by a linear gain based on position.
fn fade_sample(pcm: &mut [u8], offset: usize, index: usize, total: usize) {
    if offset + 1 >= pcm.len() { return; }
    let sample = i16::from_le_bytes([pcm[offset], pcm[offset + 1]]);
    let gain = 1.0 - (index as f32 / total as f32);
    let faded = (sample as f32 * gain) as i16;
    let bytes = faded.to_le_bytes();
    pcm[offset] = bytes[0];
    pcm[offset + 1] = bytes[1];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_append_data_to_buffer() {
        let spcm = StreamingPcm::new();

        spcm.append(&[1, 2, 3]);

        let buf = spcm.pcm.lock().unwrap();
        assert_eq!(&*buf, &[1, 2, 3]);
    }

    #[test]
    fn should_start_not_complete() {
        let spcm = StreamingPcm::new();

        assert!(!spcm.complete.load(Ordering::Acquire));
    }

    #[test]
    fn should_mark_complete_on_finish() {
        let spcm = StreamingPcm::new();

        spcm.finish();

        assert!(spcm.complete.load(Ordering::Acquire));
    }

    #[test]
    fn should_respect_max_bytes_limit() {
        let spcm = StreamingPcm::new();

        spcm.append_with_limit(&[0u8; 20], 10);

        let buf = spcm.pcm.lock().unwrap();
        assert_eq!(buf.len(), 10);
        assert!(spcm.complete.load(Ordering::Acquire));
    }

    #[test]
    fn should_not_append_when_already_at_limit() {
        let spcm = StreamingPcm::new();
        spcm.append_with_limit(&[0u8; 10], 10);

        spcm.append_with_limit(&[1u8; 5], 10);

        let buf = spcm.pcm.lock().unwrap();
        assert_eq!(buf.len(), 10);
    }

    #[test]
    fn should_truncate_with_fadeout_when_over_max() {
        let max = 5000;
        let mut pcm = vec![0u8; 10000];
        // Fill with max-amplitude samples (0x7FFF = 32767)
        for chunk in pcm.chunks_exact_mut(2) {
            chunk.copy_from_slice(&0x7FFFi16.to_le_bytes());
        }

        truncate_with_fadeout(&mut pcm, max);

        assert_eq!(pcm.len(), max);
        // Last sample should be attenuated toward zero
        let last_sample = i16::from_le_bytes([pcm[max - 2], pcm[max - 1]]);
        assert!(last_sample.abs() < 100, "last sample should be near zero, got {last_sample}");
    }

    #[test]
    fn should_not_truncate_when_under_max() {
        let mut pcm = vec![0xAB; 100];

        truncate_with_fadeout(&mut pcm, 200);

        assert_eq!(pcm.len(), 100);
    }
}
