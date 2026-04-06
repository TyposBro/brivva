//! Pipeline failure counters.
//!
//! Thread-safe atomic counters for tracking pipeline error events.
//! Core layer: no domain knowledge, just counter storage.

use std::sync::atomic::{AtomicU64, Ordering};

pub struct PipelineCounters {
    pub tts_failures: AtomicU64,
    pub tts_timeouts: AtomicU64,
    pub stt_disconnects: AtomicU64,
    pub translation_empty: AtomicU64,
}

impl PipelineCounters {
    pub fn new() -> Self {
        Self {
            tts_failures: AtomicU64::new(0),
            tts_timeouts: AtomicU64::new(0),
            stt_disconnects: AtomicU64::new(0),
            translation_empty: AtomicU64::new(0),
        }
    }

    pub fn tts_failures(&self) -> u64 {
        self.tts_failures.load(Ordering::Relaxed)
    }

    pub fn tts_timeouts(&self) -> u64 {
        self.tts_timeouts.load(Ordering::Relaxed)
    }

    pub fn stt_disconnects(&self) -> u64 {
        self.stt_disconnects.load(Ordering::Relaxed)
    }

    pub fn translation_empty(&self) -> u64 {
        self.translation_empty.load(Ordering::Relaxed)
    }

    pub fn increment_tts_failures(&self) {
        self.tts_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_tts_timeouts(&self) {
        self.tts_timeouts.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_stt_disconnects(&self) {
        self.stt_disconnects.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_translation_empty(&self) {
        self.translation_empty.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for PipelineCounters {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_start_all_counters_at_zero() {
        let c = PipelineCounters::new();

        assert_eq!(c.tts_failures(), 0);
        assert_eq!(c.tts_timeouts(), 0);
        assert_eq!(c.stt_disconnects(), 0);
        assert_eq!(c.translation_empty(), 0);
    }

    #[test]
    fn should_increment_tts_failures() {
        let c = PipelineCounters::new();

        c.increment_tts_failures();
        c.increment_tts_failures();

        assert_eq!(c.tts_failures(), 2);
    }

    #[test]
    fn should_increment_tts_timeouts() {
        let c = PipelineCounters::new();

        c.increment_tts_timeouts();

        assert_eq!(c.tts_timeouts(), 1);
    }

    #[test]
    fn should_increment_stt_disconnects() {
        let c = PipelineCounters::new();

        c.increment_stt_disconnects();
        c.increment_stt_disconnects();
        c.increment_stt_disconnects();

        assert_eq!(c.stt_disconnects(), 3);
    }

    #[test]
    fn should_increment_translation_empty() {
        let c = PipelineCounters::new();

        c.increment_translation_empty();

        assert_eq!(c.translation_empty(), 1);
    }

    #[test]
    fn should_increment_counters_independently() {
        let c = PipelineCounters::new();

        c.increment_tts_failures();
        c.increment_stt_disconnects();

        assert_eq!(c.tts_failures(), 1);
        assert_eq!(c.tts_timeouts(), 0);
        assert_eq!(c.stt_disconnects(), 1);
        assert_eq!(c.translation_empty(), 0);
    }

    #[test]
    fn should_default_same_as_new() {
        let from_default = PipelineCounters::default();

        assert_eq!(from_default.tts_failures(), 0);
        assert_eq!(from_default.tts_timeouts(), 0);
    }
}
