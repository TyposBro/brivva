//! Prosody extraction and emotion classification.
//!
//! Extracts pitch, energy, and pause density from raw PCM audio.
//! Used to classify emotion and map to TTS voice style parameters.

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

    if duration_s < 0.1 || pcm.len() < 640 {
        return Prosody {
            pitch_mean: 0.0, pitch_std: 0.0, energy_rms: 0.0,
            speaking_rate_wpm: 0, pause_density: 0.0, duration_s,
        };
    }

    // Decode i16 PCM to float samples
    let samples: Vec<f32> = pcm.chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect();

    // Energy RMS
    let energy_rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();

    // Pitch estimation via autocorrelation
    let frame_len = (0.03 * sample_rate as f32) as usize;
    let hop_len = (0.01 * sample_rate as f32) as usize;
    let min_lag = sample_rate as usize / 500; // 500Hz max
    let max_lag = sample_rate as usize / 50;  // 50Hz min
    let mut pitches = Vec::new();

    let mut start = 0;
    while start + frame_len <= samples.len() {
        let frame = &samples[start..start + frame_len];
        let frame_energy: f32 = frame.iter().map(|s| s * s).sum();

        if frame_energy > 1e-6 && max_lag < frame_len {
            let corr_0: f32 = frame_energy;
            let mut best_corr = 0.0f32;
            let mut best_lag = min_lag;

            for lag in min_lag..max_lag.min(frame_len) {
                let corr: f32 = (0..frame_len - lag)
                    .map(|i| frame[i] * frame[i + lag])
                    .sum();
                if corr > best_corr {
                    best_corr = corr;
                    best_lag = lag;
                }
            }

            if corr_0 > 0.0 && best_corr / corr_0 > 0.3 {
                let pitch_hz = sample_rate as f32 / best_lag as f32;
                if pitch_hz > 50.0 && pitch_hz < 500.0 {
                    pitches.push(pitch_hz);
                }
            }
        }
        start += hop_len;
    }

    let pitch_mean = if pitches.is_empty() {
        0.0
    } else {
        pitches.iter().sum::<f32>() / pitches.len() as f32
    };
    let pitch_std = if pitches.is_empty() {
        0.0
    } else {
        (pitches.iter().map(|p| (p - pitch_mean).powi(2)).sum::<f32>()
            / pitches.len() as f32)
            .sqrt()
    };

    // Pause density: fraction of frames below 10% of median energy
    let mut frame_energies = Vec::new();
    let mut s = 0;
    while s + frame_len <= samples.len() {
        let e: f32 = samples[s..s + frame_len].iter().map(|x| x * x).sum::<f32>()
            / frame_len as f32;
        frame_energies.push(e);
        s += hop_len;
    }

    let pause_density = if frame_energies.is_empty() {
        0.0
    } else {
        let mut sorted = frame_energies.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = sorted[sorted.len() / 2];
        let threshold = median * 0.1;
        let silent = frame_energies.iter().filter(|e| **e < threshold).count();
        silent as f32 / frame_energies.len() as f32
    };

    Prosody { pitch_mean, pitch_std, energy_rms, speaking_rate_wpm: 0, pause_density, duration_s }
}

pub fn compute_speaking_rate(prosody: &mut Prosody, word_count: usize) {
    if prosody.duration_s > 0.0 && word_count > 0 {
        prosody.speaking_rate_wpm = (word_count as f32 / prosody.duration_s * 60.0) as u32;
    }
}

// ── Emotion Classification ────────────────────────────────
//
// Calibrated for web mic at arm's length (energy 0.01-0.05 RMS range).

pub fn classify_emotion(prosody: &Prosody) -> &'static str {
    let energy = prosody.energy_rms;
    let pitch_std = prosody.pitch_std;
    let pitch_mean = prosody.pitch_mean;
    let pause_density = prosody.pause_density;

    let is_loud = energy > 0.035;
    let is_quiet = energy < 0.018;
    let is_expressive = pitch_std > 85.0;
    let is_monotone = pitch_std < 55.0;
    let is_high_pitch = pitch_mean > 200.0;
    let is_hesitant = pause_density > 0.4;

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

// ── Style Param Mapping ───────────────────────────────────
//
// Maps emotion to TTS voice style parameters.
// Returns (stability, similarity_boost, style, speed).

pub fn map_style(emotion: &str) -> (f64, f64, f64, f64) {
    match emotion {
        "excited" => (0.20, 0.50, 0.90, 1.20),
        "happy"   => (0.30, 0.60, 0.70, 1.10),
        "angry"   => (0.25, 0.70, 0.85, 1.05),
        "sad"     => (0.70, 0.80, 0.40, 0.85),
        "serious" => (0.60, 0.80, 0.30, 0.95),
        _         => (0.50, 0.75, 0.00, 1.00),
    }
}
