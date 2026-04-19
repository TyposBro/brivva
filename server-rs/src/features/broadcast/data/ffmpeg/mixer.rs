//! PCM s16le gain + mix primitives used by the audio drain loop.

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
pub(super) fn mix_pcm_s16le(a: &[u8], a_gain: f32, b: &[u8], b_gain: f32) -> Vec<u8> {
    let n = a.len().min(b.len());
    let n_aligned = n - (n % 2);
    let mut out = Vec::with_capacity(n_aligned);
    let mut i = 0;
    while i + 1 < n_aligned {
        let sa = i16::from_le_bytes([a[i], a[i + 1]]) as f32;
        let sb = i16::from_le_bytes([b[i], b[i + 1]]) as f32;
        let mixed = (sa * a_gain + sb * b_gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        out.extend_from_slice(&mixed.to_le_bytes());
        i += 2;
    }
    out
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
}
