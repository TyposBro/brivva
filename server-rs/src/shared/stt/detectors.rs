//! Chunk detection for STT utterances.

use std::time::{Duration, Instant};
use super::markers;

// ── Chunk Detection ───────────────────────────────────────

pub trait ChunkDetector: Send {
    fn check(&mut self, transcript: &str) -> bool;
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
        if elapsed >= self.max_duration { return true; }
        if elapsed < self.min_duration { return false; }
        find_clause_boundary(transcript, self.markers, self.min_chars_after)
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

pub fn get_detector(lang: &str) -> Box<dyn ChunkDetector> {
    let config = markers::detector_config(lang);
    if config.markers.is_empty() {
        Box::new(FallbackDetector {
            max_duration: config.max_duration,
            started_at: None,
        })
    } else {
        Box::new(MarkerDetector {
            markers: config.markers,
            min_chars_after: config.min_chars_after,
            min_duration: config.min_duration,
            max_duration: config.max_duration,
            started_at: None,
        })
    }
}

fn find_clause_boundary(transcript: &str, markers: &[&str], min_chars_after: usize) -> bool {
    let lower = transcript.to_lowercase();
    for marker in markers {
        let m = marker.to_lowercase();
        if let Some(pos) = lower.rfind(&m) {
            let after_pos = pos + marker.len();
            let after = transcript[after_pos..].trim();
            if after.len() >= min_chars_after {
                return true;
            }
        }
    }
    false
}

// ── Progressive Chunk Detection ──────────────────────────

pub struct ChunkBoundary {
    pub split_pos: usize,
    pub chunk_text: String,
}

pub struct ProgressiveChunkDetector {
    _lang: String,
    markers: &'static [&'static str],
    min_chars_after: usize,
    min_duration: Duration,
    max_duration: Duration,
    emitted_text: String,
    prev_chunk_text: Option<String>,
    utterance_start: Option<Instant>,
    last_chunk_time: Option<Instant>,
}

impl ProgressiveChunkDetector {
    pub fn new(lang: &str) -> Self {
        let config = markers::progressive_config(lang);
        Self {
            _lang: lang.to_string(),
            markers: config.markers,
            min_chars_after: config.min_chars_after,
            min_duration: config.min_duration,
            max_duration: config.max_duration,
            emitted_text: String::new(),
            prev_chunk_text: None,
            utterance_start: None,
            last_chunk_time: None,
        }
    }

    pub fn start(&mut self) {
        if self.utterance_start.is_none() {
            self.utterance_start = Some(Instant::now());
            self.last_chunk_time = Some(Instant::now());
        }
    }

    fn derive_position(&self, transcript: &str) -> usize {
        if self.emitted_text.is_empty() { return 0; }
        if transcript.starts_with(&self.emitted_text) { return self.emitted_text.len(); }
        derive_normalized_position(&self.emitted_text, transcript)
    }

    pub fn check(&mut self, transcript: &str) -> Option<ChunkBoundary> {
        self.start();
        let since_last = self.last_chunk_time.unwrap().elapsed();
        let emitted_pos = self.derive_position(transcript);
        if emitted_pos >= transcript.len() { return None; }

        let new_text = &transcript[emitted_pos..];
        if new_text.trim().is_empty() { return None; }

        if since_last >= self.max_duration && new_text.trim().chars().count() >= super::config::MIN_CHARS_FOR_FORCE_SPLIT {
            let split_at = find_last_word_boundary(new_text, emitted_pos);
            return self.emit(transcript, emitted_pos, split_at);
        }
        if since_last < self.min_duration { return None; }

        find_progressive_boundary(new_text, self.markers, self.min_chars_after, emitted_pos, transcript)
            .and_then(|abs_pos| self.emit(transcript, emitted_pos, abs_pos))
    }

    pub fn flush(&mut self, transcript: &str) -> Option<ChunkBoundary> {
        let emitted_pos = self.derive_position(transcript);
        if emitted_pos >= transcript.len() { return None; }
        let remaining = transcript[emitted_pos..].trim();
        if remaining.is_empty() { return None; }
        self.emit(transcript, emitted_pos, transcript.len())
    }

    pub fn context(&self) -> Option<&str> {
        self.prev_chunk_text.as_deref()
    }

    pub fn reset(&mut self) {
        self.emitted_text.clear();
        self.prev_chunk_text = None;
        self.utterance_start = None;
        self.last_chunk_time = None;
    }

    fn emit(&mut self, transcript: &str, emitted_pos: usize, split_pos: usize) -> Option<ChunkBoundary> {
        let chunk_text = transcript[emitted_pos..split_pos].trim().to_string();
        if chunk_text.is_empty() { return None; }
        self.prev_chunk_text = Some(chunk_text.clone());
        self.emitted_text = transcript[..split_pos].to_string();
        self.last_chunk_time = Some(Instant::now());
        Some(ChunkBoundary { split_pos, chunk_text })
    }
}

fn derive_normalized_position(emitted: &str, transcript: &str) -> usize {
    let emitted_norm: String = emitted.chars()
        .filter(|c| !c.is_ascii_punctuation() && !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect();
    let mut matched = 0;
    let mut pos = 0;
    for ch in transcript.chars() {
        if matched >= emitted_norm.len() { break; }
        let norm_ch: String = ch.to_lowercase().collect();
        if !ch.is_ascii_punctuation() && !ch.is_whitespace()
            && emitted_norm[matched..].starts_with(&norm_ch) {
                matched += norm_ch.len();
            }
        pos += ch.len_utf8();
    }
    if matched >= emitted_norm.len() * super::config::NORMALIZED_MATCH_THRESHOLD_PCT / 100 { pos } else { 0 }
}

fn find_last_word_boundary(new_text: &str, emitted_pos: usize) -> usize {
    match new_text.rfind(char::is_whitespace) {
        Some(rel_pos) => emitted_pos + rel_pos,
        None => emitted_pos + new_text.len(),
    }
}

fn find_progressive_boundary(
    new_text: &str,
    markers: &[&str],
    min_chars_after: usize,
    emitted_pos: usize,
    transcript: &str,
) -> Option<usize> {
    let lower_new = new_text.to_lowercase();
    for marker in markers {
        let m = marker.to_lowercase();
        if let Some(rel_pos) = lower_new.rfind(&m) {
            let abs_pos = emitted_pos + rel_pos + m.len();
            if abs_pos <= transcript.len() {
                let after = transcript[abs_pos..].trim();
                if after.chars().count() >= min_chars_after {
                    return Some(abs_pos);
                }
            }
        }
    }
    None
}

// ── Adaptive Speed Classification ─────────────────────────

pub fn classify_speaking_speed(avg_wpm: f32) -> (&'static str, f64, f64) {
    use super::config::*;
    if avg_wpm >= FAST_WPM_THRESHOLD {
        ("fast", FAST_ENDPOINTING_SECS, FAST_MAX_DURATION_SECS)
    } else if avg_wpm < SLOW_WPM_THRESHOLD {
        ("slow", SLOW_ENDPOINTING_SECS, SLOW_MAX_DURATION_SECS)
    } else {
        ("normal", DEFAULT_ENDPOINTING_SECS, DEFAULT_MAX_DURATION_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── find_clause_boundary ──────────────��──────────────

    #[test]
    fn should_find_english_clause_boundary() {
        let result = find_clause_boundary(
            "Hello, and then the rest of the story",
            markers::ENGLISH_MARKERS,
            3,
        );

        assert!(result);
    }

    #[test]
    fn should_not_find_boundary_when_not_enough_chars_after() {
        let result = find_clause_boundary(
            "Hello, and ab",
            markers::ENGLISH_MARKERS,
            3,
        );

        assert!(!result);
    }

    #[test]
    fn should_return_false_when_no_marker_present() {
        let result = find_clause_boundary(
            "Hello world",
            markers::ENGLISH_MARKERS,
            3,
        );

        assert!(!result);
    }

    // ── classify_speaking_speed ──────────────────────────

    #[test]
    fn should_classify_fast_speaking() {
        let (label, _, _) = classify_speaking_speed(200.0);

        assert_eq!(label, "fast");
    }

    #[test]
    fn should_classify_slow_speaking() {
        let (label, _, _) = classify_speaking_speed(100.0);

        assert_eq!(label, "slow");
    }

    #[test]
    fn should_classify_normal_speaking() {
        let (label, _, _) = classify_speaking_speed(150.0);

        assert_eq!(label, "normal");
    }

    // ── derive_normalized_position ──────────────────────

    #[test]
    fn should_derive_position_for_exact_prefix() {
        let pos = derive_normalized_position("Hello", "Hello world");

        assert_eq!(pos, 5);
    }

    #[test]
    fn should_derive_position_ignoring_punctuation() {
        let pos = derive_normalized_position("Hello,", "Hello world");

        assert_eq!(pos, 5);
    }

    #[test]
    fn should_return_zero_when_no_match() {
        let pos = derive_normalized_position("zzzzz", "Hello world");

        assert_eq!(pos, 0);
    }

    // ── find_last_word_boundary ────────────────────────

    #[test]
    fn should_split_at_last_whitespace() {
        let pos = find_last_word_boundary("hello world foo", 10);

        assert_eq!(pos, 10 + 11);
    }

    #[test]
    fn should_fallback_to_end_when_no_whitespace() {
        let pos = find_last_word_boundary("superlongword", 5);

        assert_eq!(pos, 5 + 13);
    }

    #[test]
    fn should_split_at_only_whitespace() {
        let pos = find_last_word_boundary("one two", 0);

        assert_eq!(pos, 3);
    }

    #[test]
    fn should_handle_empty_text() {
        let pos = find_last_word_boundary("", 7);

        assert_eq!(pos, 7);
    }

    // ── find_progressive_boundary ───────────────────────

    #[test]
    fn should_find_progressive_boundary_after_marker() {
        let result = find_progressive_boundary(
            "first clause, and then some more text here",
            markers::ENGLISH_MARKERS,
            3,
            0,
            "first clause, and then some more text here",
        );

        assert!(result.is_some());
    }

    #[test]
    fn should_not_find_progressive_boundary_without_enough_trailing_text() {
        let result = find_progressive_boundary(
            "first clause, and ab",
            markers::ENGLISH_MARKERS,
            3,
            0,
            "first clause, and ab",
        );

        assert!(result.is_none());
    }
}
