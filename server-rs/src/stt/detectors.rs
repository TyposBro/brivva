//! Chunk detection for STT utterances.
//!
//! Forces STT to finalize long utterances at natural clause boundaries,
//! preventing audio/video desync in the translation pipeline.

use std::time::{Duration, Instant};

// ── Chunk Detection ───────────────────────────────────────

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
    // Live commerce patterns
    "ですね、", "なんですけど、", "ということで、",
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

// ── Progressive Chunk Detection ──────────────────────────
//
// Detects clause boundaries within ongoing speech (interims), emitting
// sub-utterance chunks for progressive translation. Unlike ChunkDetector
// which returns a bool for the entire utterance, this returns the split
// position so that the pipeline can translate each chunk independently.

/// A detected clause boundary within an interim transcript.
pub struct ChunkBoundary {
    /// Byte position in the transcript where the chunk ends (exclusive)
    pub split_pos: usize,
    /// The chunk text (from last emitted boundary to split_pos)
    pub chunk_text: String,
}

pub struct ProgressiveChunkDetector {
    lang: String,
    markers: &'static [&'static str],
    min_chars_after: usize,
    min_duration: Duration,
    max_duration: Duration,
    /// Concatenated text of all emitted chunks so far (used to re-derive position
    /// on each call, resilient to Gladia revising interim transcripts)
    emitted_text: String,
    /// Text of the previously emitted chunk (for translation context)
    prev_chunk_text: Option<String>,
    /// When this utterance started (first interim received)
    utterance_start: Option<Instant>,
    /// When the last chunk was emitted
    last_chunk_time: Option<Instant>,
}

impl ProgressiveChunkDetector {
    pub fn new(lang: &str) -> Self {
        let (markers, min_chars_after, min_duration, max_duration) = match lang {
            "en" => (ENGLISH_MARKERS as &[&str], 3usize, Duration::from_millis(1000), Duration::from_millis(2000)),
            "ja" => (JAPANESE_MARKERS as &[&str], 2, Duration::from_millis(800), Duration::from_millis(2000)),
            "ko" => (KOREAN_MARKERS as &[&str], 2, Duration::from_millis(800), Duration::from_millis(2000)),
            "zh" => (CHINESE_MARKERS as &[&str], 2, Duration::from_millis(800), Duration::from_millis(2000)),
            _ => (&[] as &[&str], 3, Duration::from_millis(1000), Duration::from_millis(2000)),
        };
        Self {
            lang: lang.to_string(),
            markers,
            min_chars_after,
            min_duration,
            max_duration,
            emitted_text: String::new(),
            prev_chunk_text: None,
            utterance_start: None,
            last_chunk_time: None,
        }
    }

    /// Start tracking a new utterance (call when first interim arrives).
    pub fn start(&mut self) {
        if self.utterance_start.is_none() {
            self.utterance_start = Some(Instant::now());
            self.last_chunk_time = Some(Instant::now());
        }
    }

    /// Find where previously emitted text ends in the current transcript.
    /// Handles Gladia revising text (adding commas, correcting words) by
    /// searching for the emitted text or falling back to a normalized match.
    fn derive_position(&self, transcript: &str) -> usize {
        if self.emitted_text.is_empty() {
            return 0;
        }
        // Exact match — fast path
        if transcript.starts_with(&self.emitted_text) {
            return self.emitted_text.len();
        }
        // Gladia adds punctuation between interims — try normalized matching.
        // Strip punctuation/whitespace and match character-by-character.
        let emitted_norm: String = self.emitted_text.chars()
            .filter(|c| !c.is_ascii_punctuation() && !c.is_whitespace())
            .flat_map(|c| c.to_lowercase())
            .collect();
        let mut matched = 0;
        let mut transcript_pos = 0;
        for ch in transcript.chars() {
            if matched >= emitted_norm.len() {
                break;
            }
            let norm_ch: String = ch.to_lowercase().collect();
            if !ch.is_ascii_punctuation() && !ch.is_whitespace() {
                if emitted_norm[matched..].starts_with(&norm_ch) {
                    matched += norm_ch.len();
                }
            }
            transcript_pos += ch.len_utf8();
        }
        // If we matched most of the emitted text, use this position
        if matched >= emitted_norm.len() * 80 / 100 {
            transcript_pos
        } else {
            // Can't find our emitted text — transcript was heavily revised.
            // Return 0 so the whole transcript is treated as new text,
            // but we won't emit (min_duration check will prevent re-emission).
            0
        }
    }

    /// Check the full interim transcript for a clause boundary after the last emitted position.
    /// Returns Some(ChunkBoundary) if a chunk should be emitted.
    pub fn check(&mut self, transcript: &str) -> Option<ChunkBoundary> {
        self.start();

        let since_last_chunk = self.last_chunk_time.unwrap().elapsed();
        let emitted_pos = self.derive_position(transcript);

        // Guard against position beyond transcript length
        if emitted_pos >= transcript.len() {
            return None;
        }

        let new_text = &transcript[emitted_pos..];

        // Not enough new text yet
        if new_text.trim().is_empty() {
            return None;
        }

        // Hard timeout: force split regardless of boundary
        if since_last_chunk >= self.max_duration && new_text.trim().chars().count() >= 4 {
            return self.emit(transcript, emitted_pos, transcript.len());
        }

        // Too early for a split
        if since_last_chunk < self.min_duration {
            return None;
        }

        // Look for clause boundary markers in the new text
        let lower_new = new_text.to_lowercase();
        for marker in self.markers {
            let m = marker.to_lowercase();
            if let Some(rel_pos) = lower_new.rfind(&m) {
                let abs_pos = emitted_pos + rel_pos + m.len();
                if abs_pos <= transcript.len() {
                    let after = transcript[abs_pos..].trim();
                    if after.chars().count() >= self.min_chars_after {
                        return self.emit(transcript, emitted_pos, abs_pos);
                    }
                }
            }
        }

        None
    }

    /// Emit whatever text remains after the last boundary (call on Gladia FINAL).
    pub fn flush(&mut self, transcript: &str) -> Option<ChunkBoundary> {
        let emitted_pos = self.derive_position(transcript);
        if emitted_pos >= transcript.len() {
            return None;
        }
        let remaining = transcript[emitted_pos..].trim();
        if remaining.is_empty() {
            return None;
        }
        self.emit(transcript, emitted_pos, transcript.len())
    }

    /// Get the previous chunk's text for context-aware translation.
    pub fn context(&self) -> Option<&str> {
        self.prev_chunk_text.as_deref()
    }

    /// How many chunks have been emitted for this utterance.
    pub fn chunk_count(&self) -> u16 {
        // Counted by tracking prev_chunk_text changes; use a counter instead
        // This is a simplified version — the pipeline tracks chunk_index externally
        0
    }

    /// Reset for a new utterance.
    pub fn reset(&mut self) {
        self.emitted_text.clear();
        self.prev_chunk_text = None;
        self.utterance_start = None;
        self.last_chunk_time = None;
    }

    fn emit(&mut self, transcript: &str, emitted_pos: usize, split_pos: usize) -> Option<ChunkBoundary> {
        let chunk_text = transcript[emitted_pos..split_pos].trim().to_string();
        if chunk_text.is_empty() {
            return None;
        }
        self.prev_chunk_text = Some(chunk_text.clone());
        // Accumulate the raw text up to split_pos (not just the chunk) so that
        // derive_position can find it in revised transcripts
        self.emitted_text = transcript[..split_pos].to_string();
        self.last_chunk_time = Some(Instant::now());
        Some(ChunkBoundary { split_pos, chunk_text })
    }
}

// ── Adaptive Speed Classification ─────────────────────────

/// Returns (label, endpointing_secs, max_duration_secs) for Gladia.
pub fn classify_speaking_speed(avg_wpm: f32) -> (&'static str, f64, f64) {
    if avg_wpm >= 180.0 {
        ("fast", 0.20, 5.0)
    } else if avg_wpm < 120.0 {
        ("slow", 0.35, 10.0)
    } else {
        ("normal", 0.25, 5.0)
    }
}
