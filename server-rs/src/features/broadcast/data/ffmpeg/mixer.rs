//! PCM s16le gain + mix primitives used by the audio drain loop.

pub(super) const DEFAULT_LIMITER_CEILING: i16 = 30_000;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct AudioStats {
    pub samples: usize,
    pub clipped_samples: usize,
    pub peak_abs: i16,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct MixStats {
    pub samples: usize,
    pub clipped_samples: usize,
    pub peak_abs: i16,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct LimitStats {
    pub samples: usize,
    pub limited_samples: usize,
    pub peak_before: i16,
    pub peak_after: i16,
}

/// Apply a uniform gain to a PCM s16le buffer and clip to the i16 range.
pub(super) fn apply_gain(pcm: &[u8], gain: f32) -> Vec<u8> {
    let n_aligned = pcm.len() - (pcm.len() % 2);
    let mut out = Vec::with_capacity(n_aligned);
    let mut i = 0;
    while i + 1 < n_aligned {
        let s = i16::from_le_bytes([pcm[i], pcm[i + 1]]) as f32;
        let scaled = (s * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        out.extend_from_slice(&scaled.to_le_bytes());
        i += 2;
    }
    out
}

/// Mix two PCM streams (s16le little-endian, same length) with per-source gain
/// and clip to the i16 range. Output length = min(a.len(), b.len()).
#[cfg(test)]
pub(super) fn mix_pcm_s16le(a: &[u8], a_gain: f32, b: &[u8], b_gain: f32) -> Vec<u8> {
    mix_pcm_s16le_with_stats(a, a_gain, b, b_gain).0
}

pub(super) fn mix_pcm_s16le_with_stats(
    a: &[u8],
    a_gain: f32,
    b: &[u8],
    b_gain: f32,
) -> (Vec<u8>, MixStats) {
    let n = a.len().min(b.len());
    let n_aligned = n - (n % 2);
    let mut out = Vec::with_capacity(n_aligned);
    let mut stats = MixStats::default();
    let mut i = 0;
    while i + 1 < n_aligned {
        let sa = i16::from_le_bytes([a[i], a[i + 1]]) as f32;
        let sb = i16::from_le_bytes([b[i], b[i + 1]]) as f32;
        let mixed = sa * a_gain + sb * b_gain;
        if mixed > i16::MAX as f32 || mixed < i16::MIN as f32 {
            stats.clipped_samples += 1;
        }
        let clipped = mixed.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        stats.samples += 1;
        stats.peak_abs = stats.peak_abs.max(sample_abs_i16(clipped));
        out.extend_from_slice(&clipped.to_le_bytes());
        i += 2;
    }
    (out, stats)
}

#[cfg(test)]
pub(super) fn count_clipped_samples(pcm: &[u8], threshold: i16) -> usize {
    let threshold = threshold.unsigned_abs();
    pcm.chunks_exact(2)
        .filter(|chunk| {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            sample.unsigned_abs() >= threshold
        })
        .count()
}

pub(super) fn analyze_pcm_s16le(pcm: &[u8], threshold: i16) -> AudioStats {
    let threshold = threshold.unsigned_abs();
    let mut stats = AudioStats::default();
    for chunk in pcm.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
        stats.samples += 1;
        stats.peak_abs = stats.peak_abs.max(sample_abs_i16(sample));
        if sample.unsigned_abs() >= threshold {
            stats.clipped_samples += 1;
        }
    }
    stats
}

pub(super) fn limit_pcm_s16le(pcm: &[u8], ceiling: i16) -> (Vec<u8>, LimitStats) {
    let ceiling = ceiling.unsigned_abs().min(i16::MAX as u16) as i16;
    let n_aligned = pcm.len() - (pcm.len() % 2);
    let mut out = Vec::with_capacity(n_aligned);
    let mut stats = LimitStats::default();
    for chunk in pcm[..n_aligned].chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
        let limited = sample.clamp(-ceiling, ceiling);
        stats.samples += 1;
        stats.peak_before = stats.peak_before.max(sample_abs_i16(sample));
        stats.peak_after = stats.peak_after.max(sample_abs_i16(limited));
        if limited != sample {
            stats.limited_samples += 1;
        }
        out.extend_from_slice(&limited.to_le_bytes());
    }
    (out, stats)
}

pub(super) fn duck_gain(tts_active: bool, base_host_gain: f32, duck_db: f32) -> f32 {
    if !tts_active {
        return base_host_gain;
    }
    let attenuation = 10_f32.powf(-(duck_db.max(0.0)) / 20.0);
    base_host_gain * attenuation
}

fn sample_abs_i16(sample: i16) -> i16 {
    if sample == i16::MIN {
        i16::MAX
    } else {
        sample.abs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    fn decode_samples(bytes: &[u8]) -> Vec<i16> {
        bytes
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect()
    }

    #[test]
    fn apply_gain_scales_samples_and_ignores_trailing_odd_byte() {
        let scaled = apply_gain(&[0x10, 0x27, 0xF0, 0xD8, 0xAA], 0.5);
        assert_eq!(decode_samples(&scaled), vec![5000, -5000]);
    }

    #[test]
    fn apply_gain_returns_empty_on_zero_length_input() {
        let out = apply_gain(&[], 1.0);
        assert!(out.is_empty());
    }

    #[test]
    fn apply_gain_returns_empty_when_only_odd_byte_supplied() {
        let out = apply_gain(&[0x42], 1.0);
        assert!(out.is_empty());
    }

    #[test]
    fn apply_gain_clips_to_i16_range_on_positive_overflow() {
        // 20_000 * 2.0 overflows positive → clips to i16::MAX.
        let out = apply_gain(&pcm(&[20_000]), 2.0);
        assert_eq!(decode_samples(&out), vec![i16::MAX]);
    }

    #[test]
    fn apply_gain_clips_to_i16_range_on_negative_overflow() {
        // -20_000 * 2.0 overflows negative → clips to i16::MIN.
        let out = apply_gain(&pcm(&[-20_000]), 2.0);
        assert_eq!(decode_samples(&out), vec![i16::MIN]);
    }

    #[test]
    fn apply_gain_zero_produces_silence() {
        let out = apply_gain(&pcm(&[10_000, -10_000, 30]), 0.0);
        assert_eq!(decode_samples(&out), vec![0, 0, 0]);
    }

    #[test]
    fn mix_pcm_s16le_clips_on_overflow() {
        let mixed = mix_pcm_s16le(&pcm(&[30_000, -30_000]), 1.0, &pcm(&[10_000, -10_000]), 1.0);
        assert_eq!(decode_samples(&mixed), vec![32_767, -32_768]);
    }

    #[test]
    fn mix_pcm_s16le_applies_independent_gains_then_sums() {
        let mixed = mix_pcm_s16le(&pcm(&[10_000]), 0.5, &pcm(&[10_000]), 1.0);
        assert_eq!(decode_samples(&mixed), vec![15_000]);
    }

    #[test]
    fn mix_pcm_s16le_truncates_to_shortest_even_byte_length() {
        // a has 2 samples (4 bytes), b has 1 sample (2 bytes) — output 1 sample.
        let mixed = mix_pcm_s16le(&pcm(&[5, 6]), 1.0, &pcm(&[7]), 1.0);
        assert_eq!(decode_samples(&mixed), vec![12]);
    }

    #[test]
    fn mix_pcm_s16le_returns_empty_when_either_stream_is_empty() {
        assert!(mix_pcm_s16le(&[], 1.0, &pcm(&[1]), 1.0).is_empty());
        assert!(mix_pcm_s16le(&pcm(&[1]), 1.0, &[], 1.0).is_empty());
    }

    #[test]
    fn mix_pcm_s16le_rounds_shared_length_down_to_even_byte_count() {
        // min length 5 bytes → rounds down to 4, emitting 2 samples.
        let a = vec![0x10, 0x00, 0x20, 0x00, 0xFF];
        let b = vec![0x05, 0x00, 0x03, 0x00, 0xFF];
        let mixed = mix_pcm_s16le(&a, 1.0, &b, 1.0);
        assert_eq!(decode_samples(&mixed), vec![0x15, 0x23]);
    }

    #[test]
    fn mix_pcm_s16le_with_stats_counts_pre_limiter_clipping_and_peak() {
        let (mixed, stats) = mix_pcm_s16le_with_stats(
            &pcm(&[30_000, -20_000, 1_000]),
            1.0,
            &pcm(&[10_000, -20_000, 2_000]),
            1.0,
        );

        assert_eq!(decode_samples(&mixed), vec![i16::MAX, i16::MIN, 3_000]);
        assert_eq!(stats.samples, 3);
        assert_eq!(stats.clipped_samples, 2);
        assert_eq!(stats.peak_abs, i16::MAX);
    }

    #[test]
    fn limit_pcm_s16le_caps_peaks_and_reports_stats() {
        let (limited, stats) = limit_pcm_s16le(&pcm(&[31_000, -31_000, 10_000]), 30_000);

        assert_eq!(decode_samples(&limited), vec![30_000, -30_000, 10_000]);
        assert_eq!(stats.samples, 3);
        assert_eq!(stats.limited_samples, 2);
        assert_eq!(stats.peak_before, 31_000);
        assert_eq!(stats.peak_after, 30_000);
    }

    #[test]
    fn limit_pcm_s16le_preserves_speech_range_and_ignores_odd_byte() {
        let mut input = pcm(&[1_000, -2_000, 15_000]);
        input.push(0xFF);
        let (limited, stats) = limit_pcm_s16le(&input, 30_000);

        assert_eq!(decode_samples(&limited), vec![1_000, -2_000, 15_000]);
        assert_eq!(limited.len(), 6);
        assert_eq!(stats.limited_samples, 0);
        assert_eq!(stats.peak_before, 15_000);
        assert_eq!(stats.peak_after, 15_000);
    }

    #[test]
    fn count_clipped_samples_counts_threshold_crossings() {
        assert_eq!(
            count_clipped_samples(&pcm(&[29_999, 30_000, -30_000]), 30_000),
            2
        );
    }

    #[test]
    fn analyze_pcm_s16le_counts_peak_and_ignores_trailing_odd_byte() {
        let mut input = pcm(&[0, 30_000, -32_000]);
        input.push(0xAA);
        let stats = analyze_pcm_s16le(&input, 30_000);

        assert_eq!(stats.samples, 3);
        assert_eq!(stats.clipped_samples, 2);
        assert_eq!(stats.peak_abs, 32_000);
    }

    #[test]
    fn duck_gain_applies_db_attenuation_only_when_tts_active() {
        assert_eq!(duck_gain(false, 0.5, 6.0), 0.5);
        let ducked = duck_gain(true, 1.0, 6.0);
        assert!((ducked - 0.501).abs() < 0.002);
    }
}
