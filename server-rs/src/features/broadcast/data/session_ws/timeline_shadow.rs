use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::features::broadcast::domain::media_timeline::{MediaTime, RtpMediaClock};

#[derive(Debug, Default)]
pub(super) struct VideoTimelineShadow {
    clock: Option<RtpMediaClock>,
    base_arrival: Option<Instant>,
    last_rtp_timestamp: Option<u32>,
    last_arrival_pts: Option<Duration>,
    last_log_at: Option<Instant>,
    jitter_us_q4: i128,
    frame_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VideoTimelineShadowSample {
    pub rtp_timestamp: u32,
    pub sequence_number: u16,
    pub media_pts_us: u64,
    pub arrival_pts_us: u64,
    pub transit_delta_us: i128,
    pub jitter_us: u64,
    pub frame_count: u64,
    pub should_log: bool,
}

impl VideoTimelineShadow {
    pub fn observe_access_unit(
        &mut self,
        rtp_timestamp: u32,
        sequence_number: u16,
        arrival: Instant,
    ) -> VideoTimelineShadowSample {
        let clock = *self
            .clock
            .get_or_insert_with(|| RtpMediaClock::video_90khz(rtp_timestamp));
        let base_arrival = *self.base_arrival.get_or_insert(arrival);
        let media_pts = clock.media_time_for(rtp_timestamp);
        let arrival_pts = arrival.saturating_duration_since(base_arrival);
        let media_pts_us = media_pts.as_micros();
        let arrival_pts_us = duration_to_micros(arrival_pts);
        let transit_delta_us = match (self.last_rtp_timestamp, self.last_arrival_pts) {
            (Some(last_rtp), Some(last_arrival_pts)) => {
                let previous_media_pts = clock.media_time_for(last_rtp);
                let media_delta_us = media_pts
                    .saturating_duration_since(previous_media_pts)
                    .as_micros() as i128;
                let arrival_delta_us =
                    arrival_pts.saturating_sub(last_arrival_pts).as_micros() as i128;
                arrival_delta_us - media_delta_us
            }
            _ => 0,
        };
        let abs_delta = transit_delta_us.unsigned_abs().min(i128::MAX as u128) as i128;
        self.jitter_us_q4 += abs_delta - ((self.jitter_us_q4 + 8) >> 4);
        let should_log = timeline_shadow_log_due(&mut self.last_log_at, arrival);
        self.last_rtp_timestamp = Some(rtp_timestamp);
        self.last_arrival_pts = Some(arrival_pts);
        self.frame_count += 1;

        VideoTimelineShadowSample {
            rtp_timestamp,
            sequence_number,
            media_pts_us,
            arrival_pts_us,
            transit_delta_us,
            jitter_us: ((self.jitter_us_q4 + 8) >> 4).max(0) as u64,
            frame_count: self.frame_count,
            should_log,
        }
    }
}

impl VideoTimelineShadowSample {
    pub fn to_log_payload(&self) -> Value {
        json!({
            "mode": "shadow",
            "kind": "video_rtp_authoritative",
            "rtp_timestamp": self.rtp_timestamp,
            "sequence_number": self.sequence_number,
            "media_pts_us": self.media_pts_us,
            "arrival_pts_us": self.arrival_pts_us,
            "transit_delta_us": self.transit_delta_us,
            "jitter_us": self.jitter_us,
            "frame_count": self.frame_count,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AudioTimelineShadowSample {
    pub chunk_bytes: usize,
    pub queue_was_initialized: bool,
    pub arrival_age_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimestampedAudioTimelineShadowSample {
    pub chunk_bytes: usize,
    pub sample_index: u64,
    pub sample_rate: u32,
    pub client_capture_time_us: u64,
    pub media_pts_us: u64,
}

impl TimestampedAudioTimelineShadowSample {
    pub fn from_bridge(
        chunk_bytes: usize,
        sample_index: u64,
        sample_rate: u32,
        client_capture_time_us: u64,
        media_pts: MediaTime,
    ) -> Self {
        Self {
            chunk_bytes,
            sample_index,
            sample_rate,
            client_capture_time_us,
            media_pts_us: media_pts.as_micros(),
        }
    }

    pub fn to_log_payload(&self) -> Value {
        json!({
            "mode": "shadow",
            "kind": "audio_timestamped_pcm_bridge_derived",
            "authoritative": false,
            "bridge_derived": true,
            "note": "Phase 2 audio timing is derived from timestamped WS PCM sample_index/sample_rate; not WebRTC RTP authoritative yet.",
            "chunk_bytes": self.chunk_bytes,
            "sample_index": self.sample_index,
            "sample_rate": self.sample_rate,
            "client_capture_time_us": self.client_capture_time_us,
            "media_pts_us": self.media_pts_us,
        })
    }
}

impl AudioTimelineShadowSample {
    pub fn from_arrival(
        chunk_bytes: usize,
        queue_was_initialized: bool,
        session_started_at: Instant,
        now: Instant,
    ) -> Self {
        Self {
            chunk_bytes,
            queue_was_initialized,
            arrival_age_ms: duration_to_millis(now.saturating_duration_since(session_started_at)),
        }
    }

    pub fn to_log_payload(&self) -> Value {
        json!({
            "mode": "shadow",
            "kind": "audio_arrival_non_authoritative",
            "authoritative": false,
            "note": "Phase 1 audio timing is approximate arrival/queue timing only; no RTP/sample PTS yet.",
            "chunk_bytes": self.chunk_bytes,
            "queue_was_initialized": self.queue_was_initialized,
            "arrival_age_ms": self.arrival_age_ms,
        })
    }
}

pub(super) fn timeline_shadow_enabled(enabled: bool) -> bool {
    enabled
}

pub(super) fn timeline_shadow_log_due(last_log_at: &mut Option<Instant>, now: Instant) -> bool {
    const MIN_LOG_INTERVAL: Duration = Duration::from_secs(1);
    match *last_log_at {
        None => {
            *last_log_at = Some(now);
            true
        }
        Some(last) if now.saturating_duration_since(last) >= MIN_LOG_INTERVAL => {
            *last_log_at = Some(now);
            true
        }
        Some(_) => false,
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

    #[test]
    fn video_shadow_maps_rtp_pts_and_tracks_jitter() {
        let start = Instant::now();
        let mut shadow = VideoTimelineShadow::default();
        let first = shadow.observe_access_unit(10_000, 1, start);
        let second = shadow.observe_access_unit(13_000, 2, start + Duration::from_millis(40));

        assert_eq!(first.media_pts_us, 0);
        assert_eq!(second.media_pts_us, 33_333);
        assert_eq!(second.arrival_pts_us, 40_000);
        assert_eq!(second.transit_delta_us, 6_667);
        assert!(second.jitter_us > 0);
    }

    #[test]
    fn video_shadow_handles_rtp_wraparound() {
        let start = Instant::now();
        let mut shadow = VideoTimelineShadow::default();
        shadow.observe_access_unit(u32::MAX - 10, 1, start);
        let sample = shadow.observe_access_unit(20, 2, start + Duration::from_micros(344));

        assert_eq!(sample.media_pts_us, 344);
    }

    #[test]
    fn timeline_shadow_flag_is_direct_injected_gate() {
        assert!(!timeline_shadow_enabled(false));
        assert!(timeline_shadow_enabled(true));
    }

    #[test]
    fn timeline_shadow_logs_at_most_once_per_second() {
        let start = Instant::now();
        let mut last = None;

        assert!(timeline_shadow_log_due(&mut last, start));
        assert!(!timeline_shadow_log_due(
            &mut last,
            start + Duration::from_millis(999)
        ));
        assert!(timeline_shadow_log_due(
            &mut last,
            start + Duration::from_secs(1)
        ));
    }

    #[test]
    fn video_shadow_marks_only_throttled_samples_for_logging() {
        let start = Instant::now();
        let mut shadow = VideoTimelineShadow::default();

        assert!(shadow.observe_access_unit(10_000, 1, start).should_log);
        assert!(
            !shadow
                .observe_access_unit(13_000, 2, start + Duration::from_millis(500))
                .should_log
        );
        assert!(
            shadow
                .observe_access_unit(16_000, 3, start + Duration::from_secs(1))
                .should_log
        );
    }

    #[test]
    fn video_log_payload_is_shadow_only_and_authoritative() {
        let sample = VideoTimelineShadowSample {
            rtp_timestamp: 42,
            sequence_number: 7,
            media_pts_us: 1_000,
            arrival_pts_us: 1_100,
            transit_delta_us: 100,
            jitter_us: 6,
            frame_count: 2,
            should_log: true,
        };

        let payload = sample.to_log_payload();
        assert_eq!(payload["mode"], "shadow");
        assert_eq!(payload["kind"], "video_rtp_authoritative");
        assert_eq!(payload["media_pts_us"], 1_000);
        assert_eq!(payload["jitter_us"], 6);
    }

    #[test]
    fn timestamped_audio_log_payload_is_bridge_derived() {
        let sample = TimestampedAudioTimelineShadowSample::from_bridge(
            1_764,
            44_100,
            44_100,
            123_456,
            MediaTime::from_micros(1_000_000),
        );

        let payload = sample.to_log_payload();
        assert_eq!(payload["mode"], "shadow");
        assert_eq!(payload["kind"], "audio_timestamped_pcm_bridge_derived");
        assert_eq!(payload["authoritative"], false);
        assert_eq!(payload["bridge_derived"], true);
        assert_eq!(payload["media_pts_us"], 1_000_000);
    }

    #[test]
    fn audio_log_payload_is_clearly_non_authoritative() {
        let start = Instant::now();
        let sample = AudioTimelineShadowSample::from_arrival(
            3_840,
            false,
            start,
            start + Duration::from_millis(25),
        );

        let payload = sample.to_log_payload();
        assert_eq!(payload["mode"], "shadow");
        assert_eq!(payload["kind"], "audio_arrival_non_authoritative");
        assert_eq!(payload["authoritative"], false);
        assert!(payload["note"].as_str().unwrap().contains("approximate"));
        assert_eq!(payload["arrival_age_ms"], 25);
    }
}
