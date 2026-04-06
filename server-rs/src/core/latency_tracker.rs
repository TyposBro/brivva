//! Rolling-average latency tracker.
//!
//! Records latency samples and computes a rolling average over the last N samples.
//! Pure logic, no domain knowledge.

use std::collections::VecDeque;
use std::sync::Mutex;

const DEFAULT_WINDOW_SIZE: usize = 20;

pub struct LatencyTracker {
    samples: Mutex<VecDeque<u64>>,
    window_size: usize,
}

impl LatencyTracker {
    pub fn new() -> Self {
        Self::with_window(DEFAULT_WINDOW_SIZE)
    }

    pub fn with_window(window_size: usize) -> Self {
        Self {
            samples: Mutex::new(VecDeque::with_capacity(window_size)),
            window_size,
        }
    }

    /// Record a latency sample in milliseconds.
    pub fn record(&self, latency_ms: u64) {
        let mut samples = self.samples.lock().unwrap();
        if samples.len() >= self.window_size {
            samples.pop_front();
        }
        samples.push_back(latency_ms);
    }

    /// Compute the rolling average of recorded samples. Returns 0 if no samples.
    pub fn rolling_average_ms(&self) -> u64 {
        let samples = self.samples.lock().unwrap();
        if samples.is_empty() {
            return 0;
        }
        let sum: u64 = samples.iter().sum();
        sum / samples.len() as u64
    }

    /// Number of samples currently stored.
    pub fn sample_count(&self) -> usize {
        self.samples.lock().unwrap().len()
    }
}

impl Default for LatencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_return_zero_with_no_samples() {
        let tracker = LatencyTracker::new();

        assert_eq!(tracker.rolling_average_ms(), 0);
    }

    #[test]
    fn should_return_single_sample_as_average() {
        let tracker = LatencyTracker::new();

        tracker.record(100);

        assert_eq!(tracker.rolling_average_ms(), 100);
    }

    #[test]
    fn should_compute_average_of_multiple_samples() {
        let tracker = LatencyTracker::new();

        tracker.record(100);
        tracker.record(200);
        tracker.record(300);

        assert_eq!(tracker.rolling_average_ms(), 200);
    }

    #[test]
    fn should_evict_oldest_when_window_is_full() {
        let tracker = LatencyTracker::with_window(3);

        tracker.record(100);
        tracker.record(200);
        tracker.record(300);
        tracker.record(400);

        // Window: [200, 300, 400]
        assert_eq!(tracker.rolling_average_ms(), 300);
        assert_eq!(tracker.sample_count(), 3);
    }

    #[test]
    fn should_respect_custom_window_size() {
        let tracker = LatencyTracker::with_window(2);

        tracker.record(10);
        tracker.record(20);
        tracker.record(30);

        assert_eq!(tracker.sample_count(), 2);
        assert_eq!(tracker.rolling_average_ms(), 25);
    }

    #[test]
    fn should_start_with_zero_sample_count() {
        let tracker = LatencyTracker::new();

        assert_eq!(tracker.sample_count(), 0);
    }

    #[test]
    fn should_default_same_as_new() {
        let from_default = LatencyTracker::default();
        let from_new = LatencyTracker::new();

        assert_eq!(from_default.window_size, from_new.window_size);
    }
}
