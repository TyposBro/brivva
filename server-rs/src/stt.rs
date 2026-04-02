//! Direct Deepgram Nova-3 integration.
//!
//! Replaces the Python stt-wrapper: connects directly to Deepgram WebSocket,
//! handles clause-boundary chunking, prosody analysis, and emotion classification.
//! Eliminates the Python dependency — everything runs in the single Tauri binary.

use std::time::{Duration, Instant};

// ── Deepgram URL ──────────────────────────────────────────

pub fn build_deepgram_url(
    lang: &str,
    sample_rate: u32,
    endpointing: u32,
    utterance_end_ms: u32,
) -> String {
    format!(
        "wss://api.deepgram.com/v1/listen\
         ?model=nova-3\
         &encoding=linear16\
         &sample_rate={sample_rate}\
         &channels=1\
         &language={lang}\
         &punctuate=true\
         &smart_format=true\
         &interim_results=true\
         &endpointing={endpointing}\
         &vad_events=true\
         &utterance_end_ms={utterance_end_ms}"
    )
}

// ── Deepgram Response Types ───────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct DgResponse {
    #[serde(rename = "type", default)]
    pub msg_type: String,
    #[serde(default)]
    pub channel: Option<DgChannel>,
    #[serde(default)]
    pub is_final: bool,
    #[serde(default)]
    pub speech_final: bool,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub request_id: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct DgChannel {
    #[serde(default)]
    pub alternatives: Vec<DgAlternative>,
}

#[derive(Debug, serde::Deserialize)]
pub struct DgAlternative {
    #[serde(default)]
    pub transcript: String,
}

impl DgResponse {
    /// Extract the transcript text from a Results message.
    pub fn transcript(&self) -> Option<String> {
        self.channel
            .as_ref()
            .and_then(|ch| ch.alternatives.first())
            .map(|alt| alt.transcript.trim().to_string())
            .filter(|t| !t.is_empty())
    }
}

// ── Chunk Detection ───────────────────────────────────────
//
// Forces STT to finalize long utterances at natural clause boundaries,
// preventing audio/video desync in the translation pipeline.

pub trait ChunkDetector: Send {
    /// Returns true when the utterance should be force-finalized.
    fn check(&mut self, transcript: &str) -> bool;
    /// Reset state after a final is emitted.
    fn reset(&mut self);
}

struct MarkerDetector {
    markers: &'static [&'static str],
    min_chars_after: usize,
    min_duration: Duration,
    max_duration: Duration,
    started_at: Option<Instant>,
}

impl ChunkDetector for MarkerDetector {
    fn check(&mut self, transcript: &str) -> bool {
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
            return false;
        }
        let elapsed = self.started_at.unwrap().elapsed();

        // Hard timeout: always finalize
        if elapsed >= self.max_duration {
            return true;
        }
        // Too early
        if elapsed < self.min_duration {
            return false;
        }
        // Check for clause boundary markers
        let lower = transcript.to_lowercase();
        for marker in self.markers {
            let m = marker.to_lowercase();
            if let Some(pos) = lower.rfind(&m) {
                let after_pos = pos + marker.len();
                let after = transcript[after_pos..].trim();
                if after.len() >= self.min_chars_after {
                    return true;
                }
            }
        }
        false
    }

    fn reset(&mut self) {
        self.started_at = None;
    }
}

struct FallbackDetector {
    max_duration: Duration,
    started_at: Option<Instant>,
}

impl ChunkDetector for FallbackDetector {
    fn check(&mut self, _transcript: &str) -> bool {
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
            return false;
        }
        self.started_at.unwrap().elapsed() >= self.max_duration
    }

    fn reset(&mut self) {
        self.started_at = None;
    }
}

// ── Language-Specific Markers ─────────────────────────────

const ENGLISH_MARKERS: &[&str] = &[
    ", and ", ", but ", ", or ", ", so ", ", yet ", ", nor ",
    ", because ", ", since ", ", although ", ", while ", ", whereas ", ", unless ",
    ", which ", ", where ", ", when ",
    ", however ", ", therefore ", ", meanwhile ",
    "; ",
    ". And ", ". But ", ". So ", ". However ", ". Also ", ". Then ", ". Now ",
];

const JAPANESE_MARKERS: &[&str] = &[
    "けれども、", "けど、", "ですが、", "ますが、",
    "ので、", "から、", "ため、", "のに、", "ながら、",
    "しまして、", "まして、", "して、", "って、", "んで、",
    "そして ", "でも ", "だから ", "しかし ", "それから ",
    "ところが ", "それで ", "また ", "つまり ", "ただ ",
    "一方 ", "実は ", "ちなみに ",
];

const KOREAN_MARKERS: &[&str] = &[
    "는데요 ", "은데요 ", "인데요 ",
    "거든요 ", "니까요 ", "고요 ",
    "지만 ", "때문에 ", "면서 ", "어서 ", "아서 ", "하고 ",
    "는데 ", "은데 ", "인데 ",
    " 그리고 ", " 그런데 ", " 그래서 ", " 하지만 ",
    " 그래도 ", " 그러면 ", " 그러니까 ", " 또한 ", " 그다음에 ",
];

const CHINESE_MARKERS: &[&str] = &[
    "\u{FF0C}但是", "\u{FF0C}因为", "\u{FF0C}所以", "\u{FF0C}然后",
    "\u{FF0C}而且", "\u{FF0C}不过", "\u{FF0C}可是", "\u{FF0C}虽然",
    "\u{FF0C}如果", "\u{FF0C}因此", "\u{FF0C}于是", "\u{FF0C}而",
    "\u{FF0C}同时", "\u{FF0C}另外", "\u{FF0C}接着",
    ",但是", ",因为", ",所以", ",然后", ",而且", ",不过", ",可是",
    "\u{FF0C}", // Chinese comma (weaker signal)
];

pub fn get_detector(lang: &str) -> Box<dyn ChunkDetector> {
    match lang {
        "en" => Box::new(MarkerDetector {
            markers: ENGLISH_MARKERS,
            min_chars_after: 3,
            min_duration: Duration::from_millis(1500),
            max_duration: Duration::from_secs(3),
            started_at: None,
        }),
        "ja" => Box::new(MarkerDetector {
            markers: JAPANESE_MARKERS,
            min_chars_after: 2,
            min_duration: Duration::from_secs(1),
            max_duration: Duration::from_millis(2500),
            started_at: None,
        }),
        "ko" => Box::new(MarkerDetector {
            markers: KOREAN_MARKERS,
            min_chars_after: 2,
            min_duration: Duration::from_secs(1),
            max_duration: Duration::from_millis(2500),
            started_at: None,
        }),
        "zh" => Box::new(MarkerDetector {
            markers: CHINESE_MARKERS,
            min_chars_after: 2,
            min_duration: Duration::from_secs(1),
            max_duration: Duration::from_secs(3),
            started_at: None,
        }),
        _ => Box::new(FallbackDetector {
            max_duration: Duration::from_secs(3),
            started_at: None,
        }),
    }
}

// ── Prosody Extraction ────────────────────────────────────
//
// Extracts pitch, energy, and pause density from raw PCM audio.
// Used to classify emotion and map to ElevenLabs voice settings.

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
// Maps emotion to ElevenLabs voice_settings.
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

// ── Adaptive Speed Classification ─────────────────────────

pub fn classify_speaking_speed(avg_wpm: f32) -> (&'static str, u32, u32) {
    if avg_wpm >= 180.0 {
        ("fast", 800, 300)
    } else if avg_wpm < 120.0 {
        ("slow", 2000, 500)
    } else {
        ("normal", 1500, 400)
    }
}
