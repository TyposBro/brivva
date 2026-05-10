use std::time::{Duration, Instant};

use crate::features::broadcast::domain::media_timeline::MediaTime;

const BACKWARD_TIMESTAMP_TOLERANCE: Duration = Duration::from_millis(100);
const FUTURE_CAPTURE_TOLERANCE: Duration = Duration::from_secs(1);
const STALE_CAPTURE_TOLERANCE: Duration = Duration::from_secs(2);
const LARGE_MEDIA_GAP_TOLERANCE: Duration = Duration::from_secs(2);

const MAGIC: &[u8; 4] = b"BTA2";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 28;
const MIN_SAMPLE_RATE: u32 = 8_000;
const MAX_SAMPLE_RATE: u32 = 192_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimestampedPcmFrame<'a> {
    pub sample_index: u64,
    pub sample_rate: u32,
    pub client_capture_time_us: u64,
    pub pcm: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BinaryAudioPayload<'a> {
    RawPcm(&'a [u8]),
    TimestampedPcm(TimestampedPcmFrame<'a>),
    RejectedTimestamped(TimestampedPcmParseError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimestampedPcmParseError {
    NotTimestamped,
    TooShort,
    UnsupportedVersion(u8),
    ReservedBytesNonZero,
    InvalidSampleRate(u32),
    EmptyPcm,
    OddPcmLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HostAudioClockResyncReason {
    BackwardJump,
    LargeMediaGap,
    MappedTooFarFuture,
    MappedTooStale,
}

impl HostAudioClockResyncReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BackwardJump => "backward_jump",
            Self::LargeMediaGap => "large_media_gap",
            Self::MappedTooFarFuture => "mapped_too_far_future",
            Self::MappedTooStale => "mapped_too_stale",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HostAudioClockMapping {
    pub captured_at: Instant,
    pub media_pts: MediaTime,
    pub mapped_capture_age: Duration,
    pub resync_count: u64,
    pub resync_reason: Option<HostAudioClockResyncReason>,
}

/// Maps browser audio media PTS (`BTA2` sample index/sample rate) onto server
/// `Instant`s used by the existing RTMP delay buffers. Arrival time only
/// anchors or resyncs the clock; steady-state cadence comes from media PTS.
#[derive(Debug, Default, Clone)]
pub(super) struct HostAudioClockMapper {
    base_media_pts: Option<MediaTime>,
    base_server_instant: Option<Instant>,
    last_media_pts: Option<MediaTime>,
    resync_count: u64,
}

impl HostAudioClockMapper {
    pub fn map(&mut self, media_pts: MediaTime, now: Instant) -> HostAudioClockMapping {
        if self.base_media_pts.is_none() || self.base_server_instant.is_none() {
            self.anchor(media_pts, now);
            return self.mapping(media_pts, now, now, None);
        }

        if let Some(last) = self.last_media_pts {
            if media_pts < last
                && last.saturating_duration_since(media_pts) > BACKWARD_TIMESTAMP_TOLERANCE
            {
                return self.resync(media_pts, now, HostAudioClockResyncReason::BackwardJump);
            }
            if media_pts.saturating_duration_since(last) > LARGE_MEDIA_GAP_TOLERANCE {
                return self.resync(media_pts, now, HostAudioClockResyncReason::LargeMediaGap);
            }
        }

        let base_media_pts = self.base_media_pts.expect("checked above");
        let base_server_instant = self.base_server_instant.expect("checked above");
        let captured_at = base_server_instant
            .checked_add(media_pts.saturating_duration_since(base_media_pts))
            .unwrap_or(now);

        if captured_at > now && captured_at.duration_since(now) > FUTURE_CAPTURE_TOLERANCE {
            return self.resync(
                media_pts,
                now,
                HostAudioClockResyncReason::MappedTooFarFuture,
            );
        }
        if now > captured_at && now.duration_since(captured_at) > STALE_CAPTURE_TOLERANCE {
            return self.resync(media_pts, now, HostAudioClockResyncReason::MappedTooStale);
        }

        self.last_media_pts = Some(media_pts);
        self.mapping(media_pts, captured_at, now, None)
    }

    fn anchor(&mut self, media_pts: MediaTime, now: Instant) {
        self.base_media_pts = Some(media_pts);
        self.base_server_instant = Some(now);
        self.last_media_pts = Some(media_pts);
    }

    fn resync(
        &mut self,
        media_pts: MediaTime,
        now: Instant,
        reason: HostAudioClockResyncReason,
    ) -> HostAudioClockMapping {
        self.resync_count = self.resync_count.saturating_add(1);
        self.anchor(media_pts, now);
        self.mapping(media_pts, now, now, Some(reason))
    }

    fn mapping(
        &self,
        media_pts: MediaTime,
        captured_at: Instant,
        now: Instant,
        resync_reason: Option<HostAudioClockResyncReason>,
    ) -> HostAudioClockMapping {
        HostAudioClockMapping {
            captured_at,
            media_pts,
            mapped_capture_age: now.saturating_duration_since(captured_at),
            resync_count: self.resync_count,
            resync_reason,
        }
    }
}

impl<'a> TimestampedPcmFrame<'a> {
    pub fn media_pts(&self) -> MediaTime {
        let micros = self.sample_index.saturating_mul(1_000_000) / self.sample_rate as u64;
        MediaTime::from_micros(micros)
    }
}

pub(super) fn decode_binary_audio(
    bytes: &[u8],
    timestamped_enabled: bool,
) -> BinaryAudioPayload<'_> {
    if !timestamped_enabled {
        return BinaryAudioPayload::RawPcm(bytes);
    }
    match parse_timestamped_pcm(bytes) {
        Ok(frame) => BinaryAudioPayload::TimestampedPcm(frame),
        Err(TimestampedPcmParseError::NotTimestamped) => BinaryAudioPayload::RawPcm(bytes),
        Err(error) => BinaryAudioPayload::RejectedTimestamped(error),
    }
}

pub(super) fn parse_timestamped_pcm(
    bytes: &[u8],
) -> Result<TimestampedPcmFrame<'_>, TimestampedPcmParseError> {
    if !bytes.starts_with(MAGIC) {
        return Err(TimestampedPcmParseError::NotTimestamped);
    }
    if bytes.len() < HEADER_LEN {
        return Err(TimestampedPcmParseError::TooShort);
    }
    let version = bytes[4];
    if version != VERSION {
        return Err(TimestampedPcmParseError::UnsupportedVersion(version));
    }
    if bytes[5..8] != [0, 0, 0] {
        return Err(TimestampedPcmParseError::ReservedBytesNonZero);
    }
    let sample_index = u64::from_le_bytes(bytes[8..16].try_into().expect("slice len checked"));
    let sample_rate = u32::from_le_bytes(bytes[16..20].try_into().expect("slice len checked"));
    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return Err(TimestampedPcmParseError::InvalidSampleRate(sample_rate));
    }
    let client_capture_time_us =
        u64::from_le_bytes(bytes[20..28].try_into().expect("slice len checked"));
    let pcm = &bytes[HEADER_LEN..];
    if pcm.is_empty() {
        return Err(TimestampedPcmParseError::EmptyPcm);
    }
    if !pcm.len().is_multiple_of(2) {
        return Err(TimestampedPcmParseError::OddPcmLength);
    }

    Ok(TimestampedPcmFrame {
        sample_index,
        sample_rate,
        client_capture_time_us,
        pcm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        sample_index: u64,
        sample_rate: u32,
        client_capture_time_us: u64,
        pcm: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.push(VERSION);
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.extend_from_slice(&sample_index.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&client_capture_time_us.to_le_bytes());
        bytes.extend_from_slice(pcm);
        bytes
    }

    #[test]
    fn non_magic_bytes_are_old_raw_pcm() {
        assert_eq!(
            parse_timestamped_pcm(&[1, 2, 3, 4, 5]).unwrap_err(),
            TimestampedPcmParseError::NotTimestamped
        );
    }

    #[test]
    fn parses_valid_timestamped_pcm_frame() {
        let bytes = frame(88_200, 44_100, 123_456, &[1, 0, 2, 0]);
        let parsed = parse_timestamped_pcm(&bytes).unwrap();

        assert_eq!(parsed.sample_index, 88_200);
        assert_eq!(parsed.sample_rate, 44_100);
        assert_eq!(parsed.client_capture_time_us, 123_456);
        assert_eq!(parsed.pcm, &[1, 0, 2, 0]);
    }

    #[test]
    fn maps_sample_index_and_rate_to_media_pts() {
        let bytes = frame(22_050, 44_100, 0, &[1, 0]);
        let parsed = parse_timestamped_pcm(&bytes).unwrap();

        assert_eq!(parsed.media_pts().as_micros(), 500_000);
    }

    #[test]
    fn flag_off_keeps_timestamped_looking_bytes_as_raw_pcm() {
        let bytes = frame(88_200, 44_100, 123_456, &[1, 0, 2, 0]);

        assert_eq!(
            decode_binary_audio(&bytes, false),
            BinaryAudioPayload::RawPcm(&bytes)
        );
    }

    #[test]
    fn flag_on_accepts_timestamped_frames_and_preserves_old_raw_pcm() {
        let bytes = frame(88_200, 44_100, 123_456, &[1, 0, 2, 0]);
        match decode_binary_audio(&bytes, true) {
            BinaryAudioPayload::TimestampedPcm(frame) => assert_eq!(frame.pcm, &[1, 0, 2, 0]),
            other => panic!("unexpected payload: {other:?}"),
        }

        assert_eq!(
            decode_binary_audio(&[9, 8, 7, 6], true),
            BinaryAudioPayload::RawPcm(&[9, 8, 7, 6])
        );
    }

    #[test]
    fn flag_on_rejects_malformed_timestamped_frames() {
        assert_eq!(
            decode_binary_audio(b"BTA2", true),
            BinaryAudioPayload::RejectedTimestamped(TimestampedPcmParseError::TooShort)
        );
    }

    #[test]
    fn rejects_malformed_timestamped_frames() {
        assert_eq!(
            parse_timestamped_pcm(b"BTA2").unwrap_err(),
            TimestampedPcmParseError::TooShort
        );

        let mut bad_version = frame(0, 44_100, 0, &[1, 0]);
        bad_version[4] = 2;
        assert_eq!(
            parse_timestamped_pcm(&bad_version).unwrap_err(),
            TimestampedPcmParseError::UnsupportedVersion(2)
        );

        assert_eq!(
            parse_timestamped_pcm(&frame(0, 1, 0, &[1, 0])).unwrap_err(),
            TimestampedPcmParseError::InvalidSampleRate(1)
        );

        assert_eq!(
            parse_timestamped_pcm(&frame(0, 44_100, 0, &[])).unwrap_err(),
            TimestampedPcmParseError::EmptyPcm
        );

        assert_eq!(
            parse_timestamped_pcm(&frame(0, 44_100, 0, &[1])).unwrap_err(),
            TimestampedPcmParseError::OddPcmLength
        );
    }

    #[test]
    fn host_audio_clock_maps_media_pts_independent_of_arrival_jitter() {
        let mut mapper = HostAudioClockMapper::default();
        let now = Instant::now();

        let first = mapper.map(MediaTime::from_micros(1_000_000), now);
        let second = mapper.map(
            MediaTime::from_micros(1_020_000),
            now + Duration::from_millis(80),
        );
        let third = mapper.map(
            MediaTime::from_micros(1_040_000),
            now + Duration::from_millis(10),
        );

        assert_eq!(first.captured_at, now);
        assert_eq!(
            second.captured_at.duration_since(first.captured_at),
            Duration::from_millis(20)
        );
        assert_eq!(
            third.captured_at.duration_since(second.captured_at),
            Duration::from_millis(20)
        );
        assert_eq!(third.resync_count, 0);
        assert_eq!(third.resync_reason, None);
    }

    #[test]
    fn host_audio_clock_resyncs_on_large_backward_jump() {
        let mut mapper = HostAudioClockMapper::default();
        let now = Instant::now();

        mapper.map(MediaTime::from_micros(1_000_000), now);
        mapper.map(
            MediaTime::from_micros(1_040_000),
            now + Duration::from_millis(40),
        );
        let reset = mapper.map(
            MediaTime::from_micros(100_000),
            now + Duration::from_millis(60),
        );

        assert_eq!(reset.captured_at, now + Duration::from_millis(60));
        assert_eq!(reset.resync_count, 1);
        assert_eq!(
            reset.resync_reason,
            Some(HostAudioClockResyncReason::BackwardJump)
        );
    }

    #[test]
    fn host_audio_clock_resyncs_on_large_forward_gap() {
        let mut mapper = HostAudioClockMapper::default();
        let now = Instant::now();

        mapper.map(MediaTime::from_micros(1_000_000), now);
        let jumped = mapper.map(
            MediaTime::from_micros(4_000_000),
            now + Duration::from_millis(20),
        );

        assert_eq!(jumped.captured_at, now + Duration::from_millis(20));
        assert_eq!(jumped.resync_count, 1);
        assert_eq!(
            jumped.resync_reason,
            Some(HostAudioClockResyncReason::LargeMediaGap)
        );
    }

    #[test]
    fn host_audio_clock_resyncs_when_mapping_is_too_stale() {
        let mut mapper = HostAudioClockMapper::default();
        let now = Instant::now();

        mapper.map(MediaTime::from_micros(1_000_000), now);
        let stale = mapper.map(
            MediaTime::from_micros(1_020_000),
            now + Duration::from_secs(3),
        );

        assert_eq!(stale.captured_at, now + Duration::from_secs(3));
        assert_eq!(stale.resync_count, 1);
        assert_eq!(
            stale.resync_reason,
            Some(HostAudioClockResyncReason::MappedTooStale)
        );
    }

    #[test]
    fn host_audio_clock_resyncs_when_mapping_is_too_far_future() {
        let mut mapper = HostAudioClockMapper::default();
        let now = Instant::now();

        mapper.map(MediaTime::from_micros(1_000_000), now);
        let future = mapper.map(
            MediaTime::from_micros(2_500_000),
            now + Duration::from_millis(10),
        );

        assert_eq!(future.captured_at, now + Duration::from_millis(10));
        assert_eq!(future.resync_count, 1);
        assert_eq!(
            future.resync_reason,
            Some(HostAudioClockResyncReason::MappedTooFarFuture)
        );
    }
}
