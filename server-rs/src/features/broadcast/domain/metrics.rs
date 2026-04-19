//! Pure billing counters for a single live session.
//!
//! Held by `LiveSession` and shared into the RTMP drain threads. Kept in
//! `domain/` so the Axum/tokio-free type is available to any layer without
//! violating CLAUDE.md §1.2. The network reporter task lives in `data/`.

use dashmap::DashMap;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// 44.1 kHz s16le mono = 88_200 bytes/sec.
const PCM_BYTES_PER_SECOND: u64 = 88_200;

pub struct SessionMetrics {
    session_start: Instant,
    bytes_out_total: AtomicU64,
    output_ms_by_lang: DashMap<String, AtomicU64>,
}

impl SessionMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            session_start: Instant::now(),
            bytes_out_total: AtomicU64::new(0),
            output_ms_by_lang: DashMap::new(),
        })
    }

    pub fn record_bytes_out(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        self.bytes_out_total.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Bump a target language's delivered-TTS duration by the number of
    /// milliseconds implied by the PCM byte count.
    pub fn record_tts_pcm(&self, lang: &str, pcm_bytes: u64) {
        if pcm_bytes == 0 {
            return;
        }
        let ms = (pcm_bytes.saturating_mul(1000)) / PCM_BYTES_PER_SECOND;
        if ms == 0 {
            return;
        }
        let entry = self
            .output_ms_by_lang
            .entry(lang.to_string())
            .or_insert_with(|| AtomicU64::new(0));
        entry.value().fetch_add(ms, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsPayload {
        let source_seconds = self.session_start.elapsed().as_secs_f64();
        let mut output_minutes_by_lang = std::collections::BTreeMap::new();
        for entry in self.output_ms_by_lang.iter() {
            let ms = entry.value().load(Ordering::Relaxed) as f64;
            output_minutes_by_lang.insert(entry.key().clone(), ms / 60_000.0);
        }
        MetricsPayload {
            source_minutes: source_seconds / 60.0,
            output_minutes_by_lang,
            bytes_out_total: self.bytes_out_total.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MetricsPayload {
    pub source_minutes: f64,
    pub output_minutes_by_lang: std::collections::BTreeMap<String, f64>,
    pub bytes_out_total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_tts_pcm_converts_bytes_to_milliseconds_of_44_1k_mono_audio() {
        let m = SessionMetrics::new();
        // 88_200 bytes = 1 second of s16le 44.1 kHz mono = 1000 ms.
        m.record_tts_pcm("ja", PCM_BYTES_PER_SECOND);
        let snap = m.snapshot();
        let ja_min = snap
            .output_minutes_by_lang
            .get("ja")
            .copied()
            .unwrap_or(0.0);
        assert!((ja_min - 1.0 / 60.0).abs() < 1e-6);
    }

    #[test]
    fn record_bytes_out_sums_across_calls() {
        let m = SessionMetrics::new();
        m.record_bytes_out(100);
        m.record_bytes_out(250);
        assert_eq!(m.snapshot().bytes_out_total, 350);
    }

    #[test]
    fn sub_millisecond_tts_chunks_are_ignored_rather_than_rounded_up() {
        let m = SessionMetrics::new();
        // 10 bytes is ~0.11 ms — integer division floors to 0 and the
        // record becomes a no-op rather than inflating the minute count.
        m.record_tts_pcm("ja", 10);
        assert!(!m.snapshot().output_minutes_by_lang.contains_key("ja"));
    }
}
