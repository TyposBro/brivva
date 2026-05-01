use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::time::Duration;

/// Monotonic media timeline timestamp, relative to a session-owned clock.
///
/// This is not wall clock. It represents source media PTS after transport
/// timestamps have been mapped into a common timeline.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaTime {
    micros: u64,
}

impl MediaTime {
    pub const ZERO: Self = Self { micros: 0 };

    pub fn from_micros(micros: u64) -> Self {
        Self { micros }
    }

    pub fn from_duration(duration: Duration) -> Self {
        Self {
            micros: duration_to_micros(duration),
        }
    }

    pub fn as_micros(self) -> u64 {
        self.micros
    }

    pub fn saturating_add_duration(self, duration: Duration) -> Self {
        Self {
            micros: self.micros.saturating_add(duration_to_micros(duration)),
        }
    }

    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::from_micros(self.micros.saturating_sub(earlier.micros))
    }
}

/// Payload plus authoritative source-media timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedMedia<T> {
    pub pts: MediaTime,
    pub payload: T,
}

impl<T> TimedMedia<T> {
    pub fn new(pts: MediaTime, payload: T) -> Self {
        Self { pts, payload }
    }
}

/// Maps RTP timestamps into the session media timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpMediaClock {
    base_rtp_timestamp: u32,
    rtp_hz: NonZeroU32,
}

impl RtpMediaClock {
    pub fn new(base_rtp_timestamp: u32, rtp_hz: NonZeroU32) -> Self {
        Self {
            base_rtp_timestamp,
            rtp_hz,
        }
    }

    pub fn video_90khz(base_rtp_timestamp: u32) -> Self {
        Self::new(
            base_rtp_timestamp,
            NonZeroU32::new(90_000).expect("RTP video clock rate is non-zero"),
        )
    }

    pub fn media_time_for(self, rtp_timestamp: u32) -> MediaTime {
        let delta_ticks = rtp_timestamp.wrapping_sub(self.base_rtp_timestamp) as u64;
        let micros = delta_ticks.saturating_mul(1_000_000) / self.rtp_hz.get() as u64;
        MediaTime::from_micros(micros)
    }
}

/// Readiness buffer keyed by media PTS, not packet arrival order.
pub struct JitterBuffer<T> {
    target_delay: Duration,
    max_span: Duration,
    queue: VecDeque<TimedMedia<T>>,
    dropped_late: u64,
    dropped_overflow: u64,
    emitted: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JitterBufferStats {
    pub queued: usize,
    pub depth_ms: u64,
    pub dropped_late: u64,
    pub dropped_overflow: u64,
    pub emitted: u64,
}

impl<T> JitterBuffer<T> {
    pub fn new(target_delay: Duration, max_span: Duration) -> Self {
        Self {
            target_delay,
            max_span,
            queue: VecDeque::new(),
            dropped_late: 0,
            dropped_overflow: 0,
            emitted: 0,
        }
    }

    pub fn insert(&mut self, frame: TimedMedia<T>) {
        let pos = self
            .queue
            .iter()
            .position(|queued| frame.pts < queued.pts)
            .unwrap_or(self.queue.len());
        self.queue.insert(pos, frame);
        self.drop_overflow();
    }

    pub fn pop_ready(&mut self, playhead: MediaTime) -> Option<TimedMedia<T>> {
        let first = self.queue.front()?;
        if first.pts.saturating_add_duration(self.target_delay) > playhead {
            return None;
        }
        self.emitted += 1;
        self.queue.pop_front()
    }

    pub fn drop_before(&mut self, cutoff: MediaTime) -> usize {
        let mut dropped = 0;
        while self.queue.front().is_some_and(|frame| frame.pts < cutoff) {
            self.queue.pop_front();
            dropped += 1;
        }
        self.dropped_late += dropped;
        dropped as usize
    }

    pub fn stats(&self) -> JitterBufferStats {
        JitterBufferStats {
            queued: self.queue.len(),
            depth_ms: duration_to_millis(self.depth()),
            dropped_late: self.dropped_late,
            dropped_overflow: self.dropped_overflow,
            emitted: self.emitted,
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    fn depth(&self) -> Duration {
        match (self.queue.front(), self.queue.back()) {
            (Some(first), Some(last)) => last.pts.saturating_duration_since(first.pts),
            _ => Duration::ZERO,
        }
    }

    fn drop_overflow(&mut self) {
        while self.queue.len() > 1 && self.depth() > self.max_span {
            self.queue.pop_front();
            self.dropped_overflow += 1;
        }
    }
}

fn duration_to_micros(duration: Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}

fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> MediaTime {
        MediaTime::from_micros(value * 1_000)
    }

    #[test]
    fn rtp_clock_maps_video_ticks_to_media_time() {
        let clock = RtpMediaClock::video_90khz(10_000);

        assert_eq!(clock.media_time_for(13_000).as_micros(), 33_333);
    }

    #[test]
    fn rtp_clock_handles_u32_wraparound() {
        let clock = RtpMediaClock::video_90khz(u32::MAX - 10);

        assert_eq!(clock.media_time_for(20).as_micros(), 344);
    }

    #[test]
    fn jitter_buffer_emits_by_pts_after_target_delay() {
        let mut buffer = JitterBuffer::new(Duration::from_millis(100), Duration::from_secs(1));
        buffer.insert(TimedMedia::new(ms(30), "b"));
        buffer.insert(TimedMedia::new(ms(10), "a"));

        assert!(buffer.pop_ready(ms(109)).is_none());
        assert_eq!(buffer.pop_ready(ms(110)).unwrap().payload, "a");
        assert_eq!(buffer.pop_ready(ms(130)).unwrap().payload, "b");
    }

    #[test]
    fn jitter_buffer_tracks_emitted_count() {
        let mut buffer = JitterBuffer::new(Duration::ZERO, Duration::from_secs(1));
        buffer.insert(TimedMedia::new(ms(1), 1));
        buffer.insert(TimedMedia::new(ms(2), 2));

        buffer.pop_ready(ms(2));

        assert_eq!(buffer.stats().emitted, 1);
    }

    #[test]
    fn jitter_buffer_drops_oldest_when_span_exceeds_cap() {
        let mut buffer = JitterBuffer::new(Duration::ZERO, Duration::from_millis(50));
        buffer.insert(TimedMedia::new(ms(0), "old"));
        buffer.insert(TimedMedia::new(ms(40), "mid"));
        buffer.insert(TimedMedia::new(ms(80), "new"));

        let stats = buffer.stats();
        assert_eq!(stats.queued, 2);
        assert_eq!(stats.depth_ms, 40);
        assert_eq!(stats.dropped_overflow, 1);
    }

    #[test]
    fn jitter_buffer_drops_late_frames_before_cutoff() {
        let mut buffer = JitterBuffer::new(Duration::ZERO, Duration::from_secs(1));
        buffer.insert(TimedMedia::new(ms(10), "late-1"));
        buffer.insert(TimedMedia::new(ms(20), "late-2"));
        buffer.insert(TimedMedia::new(ms(30), "keep"));

        assert_eq!(buffer.drop_before(ms(25)), 2);
        assert_eq!(buffer.stats().dropped_late, 2);
        assert_eq!(buffer.pop_ready(ms(30)).unwrap().payload, "keep");
    }
}
