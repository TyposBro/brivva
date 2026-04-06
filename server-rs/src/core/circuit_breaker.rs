//! Generic circuit breaker for external API calls.
//!
//! States: Closed (normal) -> Open (failing, reject calls) -> HalfOpen (probe).
//! Pure logic, no domain knowledge.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const STATE_CLOSED: u8 = 0;
const STATE_OPEN: u8 = 1;
const STATE_HALF_OPEN: u8 = 2;

pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub reset_timeout: Duration,
}

pub struct CircuitBreaker {
    state: AtomicU8,
    consecutive_failures: AtomicU32,
    failure_threshold: u32,
    reset_timeout: Duration,
    last_failure_at: Mutex<Option<Instant>>,
}

/// Result of checking whether a call is allowed.
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: AtomicU8::new(STATE_CLOSED),
            consecutive_failures: AtomicU32::new(0),
            failure_threshold: config.failure_threshold,
            reset_timeout: config.reset_timeout,
            last_failure_at: Mutex::new(None),
        }
    }

    /// Check if a call should proceed. Returns the current state.
    pub fn check(&self) -> CircuitState {
        let state = self.state.load(Ordering::Acquire);
        match state {
            STATE_CLOSED => CircuitState::Closed,
            STATE_OPEN => self.check_open_timeout(),
            STATE_HALF_OPEN => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }

    /// Record a successful call. Resets the breaker to Closed.
    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Release);
        self.state.store(STATE_CLOSED, Ordering::Release);
    }

    /// Record a failed call. May trip the breaker to Open.
    pub fn record_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::AcqRel) + 1;
        *self.last_failure_at.lock().unwrap() = Some(Instant::now());
        if failures >= self.failure_threshold {
            self.state.store(STATE_OPEN, Ordering::Release);
        }
    }

    /// Returns true if calls should be rejected.
    pub fn is_open(&self) -> bool {
        matches!(self.check(), CircuitState::Open)
    }

    pub fn state_name(&self) -> &'static str {
        match self.state.load(Ordering::Acquire) {
            STATE_CLOSED => "closed",
            STATE_OPEN => "open",
            STATE_HALF_OPEN => "half_open",
            _ => "unknown",
        }
    }

    fn check_open_timeout(&self) -> CircuitState {
        let last = self.last_failure_at.lock().unwrap();
        if let Some(at) = *last {
            if at.elapsed() >= self.reset_timeout {
                drop(last);
                self.state.store(STATE_HALF_OPEN, Ordering::Release);
                return CircuitState::HalfOpen;
            }
        }
        CircuitState::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_breaker(threshold: u32, timeout: Duration) -> CircuitBreaker {
        CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: threshold,
            reset_timeout: timeout,
        })
    }

    #[test]
    fn should_start_in_closed_state() {
        let cb = make_breaker(3, Duration::from_secs(5));

        assert!(matches!(cb.check(), CircuitState::Closed));
        assert_eq!(cb.state_name(), "closed");
    }

    #[test]
    fn should_stay_closed_below_failure_threshold() {
        let cb = make_breaker(3, Duration::from_secs(5));

        cb.record_failure();
        cb.record_failure();

        assert!(matches!(cb.check(), CircuitState::Closed));
    }

    #[test]
    fn should_open_at_failure_threshold() {
        let cb = make_breaker(3, Duration::from_secs(5));

        cb.record_failure();
        cb.record_failure();
        cb.record_failure();

        assert!(matches!(cb.check(), CircuitState::Open));
        assert!(cb.is_open());
        assert_eq!(cb.state_name(), "open");
    }

    #[test]
    fn should_transition_to_half_open_after_timeout() {
        let cb = make_breaker(1, Duration::from_millis(50));

        cb.record_failure();
        // Should be open immediately after failure
        assert!(matches!(cb.check(), CircuitState::Open));

        // Wait for reset timeout to expire
        std::thread::sleep(Duration::from_millis(60));
        assert!(matches!(cb.check(), CircuitState::HalfOpen));
    }

    #[test]
    fn should_close_on_success_in_half_open() {
        let cb = make_breaker(1, Duration::from_millis(50));
        cb.record_failure();
        std::thread::sleep(Duration::from_millis(60));
        assert!(matches!(cb.check(), CircuitState::HalfOpen));

        cb.record_success();

        assert!(matches!(cb.check(), CircuitState::Closed));
        assert!(!cb.is_open());
    }

    #[test]
    fn should_reopen_on_failure_in_half_open() {
        let cb = make_breaker(1, Duration::from_millis(50));
        cb.record_failure();
        std::thread::sleep(Duration::from_millis(60));
        assert!(matches!(cb.check(), CircuitState::HalfOpen));

        cb.record_failure();

        // After failure in half-open, it goes back to open
        // The consecutive_failures is now 2 (>= threshold of 1), so state is open
        assert!(matches!(cb.check(), CircuitState::Open));
    }

    #[test]
    fn should_reset_failure_count_on_success() {
        let cb = make_breaker(3, Duration::from_secs(5));
        cb.record_failure();
        cb.record_failure();

        cb.record_success();
        cb.record_failure();

        assert!(matches!(cb.check(), CircuitState::Closed));
    }

    #[test]
    fn should_not_be_open_when_closed() {
        let cb = make_breaker(3, Duration::from_secs(5));

        assert!(!cb.is_open());
    }
}
