//! Prosody extraction and emotion classification.

use super::config::*;

// ── Prosody Extraction ────────────────────────────────────

pub struct Prosody {
    pub pitch_mean: f32,
    pub pitch_std: f32,
    pub energy_rms: f32,
    pub speaking_rate_wpm: u32,
    pub pause_density: f32,
    pub duration_s: f32,
}

pub fn extract_prosody(pcm: &[u8], sample_rate: u32) -> Prosody {
    let n_samples = pcm.len() / 2;
    let duration_s = n_samples as f32 / sample_rate as f32;

    if duration_s < MIN_DURATION_SECS || pcm.len() < MIN_AUDIO_BYTES {
        return Prosody {
            pitch_mean: 0.0, pitch_std: 0.0, energy_rms: 0.0,
            speaking_rate_wpm: 0, pause_density: 0.0, duration_s,
        };
    }

    let samples = decode_pcm_samples(pcm);
    let energy_rms = compute_energy_rms(&samples);
    let (pitch_mean, pitch_std) = compute_pitch_stats(&samples, sample_rate);
    let pause_density = compute_pause_density(&samples, sample_rate);

    Prosody { pitch_mean, pitch_std, energy_rms, speaking_rate_wpm: 0, pause_density, duration_s }
}

pub fn compute_speaking_rate(prosody: &mut Prosody, word_count: usize) {
    if prosody.duration_s > 0.0 && word_count > 0 {
        prosody.speaking_rate_wpm = (word_count as f32 / prosody.duration_s * 60.0) as u32;
    }
}

// ── Emotion Classification ────────────────────────────────

pub fn classify_emotion(prosody: &Prosody) -> &'static str {
    let is_loud = prosody.energy_rms > LOUD_ENERGY;
    let is_quiet = prosody.energy_rms < QUIET_ENERGY;
    let is_expressive = prosody.pitch_std > EXPRESSIVE_PITCH_STD;
    let is_monotone = prosody.pitch_std < MONOTONE_PITCH_STD;
    let is_high_pitch = prosody.pitch_mean > HIGH_PITCH_MEAN;
    let is_hesitant = prosody.pause_density > HESITANT_PAUSE_DENSITY;

    if is_loud && is_expressive && is_high_pitch { return "excited"; }
    if is_loud && is_expressive { return "angry"; }
    if is_loud && !is_monotone { return "happy"; }
    if is_quiet && is_monotone && is_hesitant { return "sad"; }
    if is_quiet && (is_hesitant || is_monotone) { return "sad"; }
    if !is_loud && !is_quiet && is_monotone { return "serious"; }
    if is_loud { return "happy"; }
    if is_expressive { return "happy"; }
    "neutral"
}

// ── PCM Decoding ──────────────────────────────────────────

fn decode_pcm_samples(pcm: &[u8]) -> Vec<f32> {
    pcm.chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect()
}

fn compute_energy_rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

// ── Pitch Analysis ────────────────────────────────────────

fn compute_pitch_stats(samples: &[f32], sample_rate: u32) -> (f32, f32) {
    let min_lag = sample_rate as usize / PITCH_MAX_HZ as usize;
    let max_lag = sample_rate as usize / PITCH_MIN_HZ as usize;
    let pitches = collect_frame_pitches(samples, sample_rate, min_lag, max_lag);
    mean_and_std(&pitches)
}

fn collect_frame_pitches(
    samples: &[f32], sample_rate: u32, min_lag: usize, max_lag: usize,
) -> Vec<f32> {
    let frame_len = (FRAME_DURATION_SECS * sample_rate as f32) as usize;
    let hop_len = (HOP_DURATION_SECS * sample_rate as f32) as usize;

    let mut pitches = Vec::new();
    let mut start = 0;
    while start + frame_len <= samples.len() {
        if let Some(p) = estimate_frame_pitch(&samples[start..start + frame_len], min_lag, max_lag, sample_rate) {
            pitches.push(p);
        }
        start += hop_len;
    }
    pitches
}

fn mean_and_std(values: &[f32]) -> (f32, f32) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32;
    (mean, variance.sqrt())
}

fn estimate_frame_pitch(frame: &[f32], min_lag: usize, max_lag: usize, sample_rate: u32) -> Option<f32> {
    let energy: f32 = frame.iter().map(|s| s * s).sum();
    if energy <= 1e-6 || max_lag >= frame.len() { return None; }

    let (best_lag, best_corr) = find_best_autocorrelation(frame, min_lag, max_lag);
    validate_pitch(best_lag, best_corr, energy, sample_rate)
}

fn find_best_autocorrelation(frame: &[f32], min_lag: usize, max_lag: usize) -> (usize, f32) {
    let mut best_corr = 0.0f32;
    let mut best_lag = min_lag;
    for lag in min_lag..max_lag.min(frame.len()) {
        let corr: f32 = (0..frame.len() - lag).map(|i| frame[i] * frame[i + lag]).sum();
        if corr > best_corr { best_corr = corr; best_lag = lag; }
    }
    (best_lag, best_corr)
}

fn validate_pitch(lag: usize, corr: f32, energy: f32, sample_rate: u32) -> Option<f32> {
    if energy <= 0.0 || corr / energy <= AUTOCORRELATION_THRESHOLD { return None; }
    let pitch_hz = sample_rate as f32 / lag as f32;
    if pitch_hz > PITCH_MIN_HZ && pitch_hz < PITCH_MAX_HZ { Some(pitch_hz) } else { None }
}

// ── Pause Density ─────────────────────────────────────────

fn compute_pause_density(samples: &[f32], sample_rate: u32) -> f32 {
    let frame_energies = collect_frame_energies(samples, sample_rate);
    if frame_energies.is_empty() { return 0.0; }

    let threshold = compute_silence_threshold(&frame_energies);
    count_silent_ratio(&frame_energies, threshold)
}

fn collect_frame_energies(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let frame_len = (FRAME_DURATION_SECS * sample_rate as f32) as usize;
    let hop_len = (HOP_DURATION_SECS * sample_rate as f32) as usize;

    let mut energies = Vec::new();
    let mut s = 0;
    while s + frame_len <= samples.len() {
        let e: f32 = samples[s..s + frame_len].iter().map(|x| x * x).sum::<f32>() / frame_len as f32;
        energies.push(e);
        s += hop_len;
    }
    energies
}

fn compute_silence_threshold(energies: &[f32]) -> f32 {
    let mut sorted = energies.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted[sorted.len() / 2] * PAUSE_ENERGY_FRACTION
}

fn count_silent_ratio(energies: &[f32], threshold: f32) -> f32 {
    let silent = energies.iter().filter(|e| **e < threshold).count();
    silent as f32 / energies.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── decode_pcm_samples ────────────────────────���─────

    #[test]
    fn should_decode_pcm_to_normalized_float() {
        // 16384 as i16 little-endian = [0x00, 0x40]
        let pcm = 16384i16.to_le_bytes();

        let samples = decode_pcm_samples(&pcm);

        assert_eq!(samples.len(), 1);
        assert!((samples[0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn should_decode_silence_to_zero() {
        let pcm = 0i16.to_le_bytes();

        let samples = decode_pcm_samples(&pcm);

        assert_eq!(samples[0], 0.0);
    }

    // ── compute_energy_rms ──────────────────────��───────

    #[test]
    fn should_compute_rms_for_known_values() {
        let samples = vec![0.5, -0.5, 0.5, -0.5];

        let rms = compute_energy_rms(&samples);

        assert!((rms - 0.5).abs() < 0.001);
    }

    #[test]
    fn should_return_zero_rms_for_silence() {
        let samples = vec![0.0; 100];

        let rms = compute_energy_rms(&samples);

        assert_eq!(rms, 0.0);
    }

    // ── mean_and_std ────────────────────────────────────

    #[test]
    fn should_compute_mean_and_std() {
        let values = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];

        let (mean, std) = mean_and_std(&values);

        assert!((mean - 5.0).abs() < 0.001);
        assert!(std > 0.0);
    }

    #[test]
    fn should_return_zero_for_empty_values() {
        let (mean, std) = mean_and_std(&[]);

        assert_eq!(mean, 0.0);
        assert_eq!(std, 0.0);
    }

    // ── classify_emotion ────────────────────────────────

    #[test]
    fn should_classify_excited() {
        let prosody = Prosody {
            pitch_mean: 250.0,
            pitch_std: 100.0,
            energy_rms: 0.05,
            speaking_rate_wpm: 0,
            pause_density: 0.0,
            duration_s: 1.0,
        };

        assert_eq!(classify_emotion(&prosody), "excited");
    }

    #[test]
    fn should_classify_angry() {
        let prosody = Prosody {
            pitch_mean: 150.0,
            pitch_std: 100.0,
            energy_rms: 0.05,
            speaking_rate_wpm: 0,
            pause_density: 0.0,
            duration_s: 1.0,
        };

        assert_eq!(classify_emotion(&prosody), "angry");
    }

    #[test]
    fn should_classify_sad() {
        let prosody = Prosody {
            pitch_mean: 100.0,
            pitch_std: 40.0,
            energy_rms: 0.01,
            speaking_rate_wpm: 0,
            pause_density: 0.5,
            duration_s: 1.0,
        };

        assert_eq!(classify_emotion(&prosody), "sad");
    }

    #[test]
    fn should_classify_neutral_for_moderate_values() {
        let prosody = Prosody {
            pitch_mean: 150.0,
            pitch_std: 70.0,
            energy_rms: 0.025,
            speaking_rate_wpm: 0,
            pause_density: 0.2,
            duration_s: 1.0,
        };

        assert_eq!(classify_emotion(&prosody), "neutral");
    }

    #[test]
    fn should_return_zero_prosody_for_short_audio() {
        let pcm = vec![0u8; 100]; // Too short

        let prosody = extract_prosody(&pcm, 44100);

        assert_eq!(prosody.pitch_mean, 0.0);
        assert_eq!(prosody.energy_rms, 0.0);
    }
}
