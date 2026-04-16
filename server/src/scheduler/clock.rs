use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct SchedulerClock {
    server_session_start: Instant,
    configured_delay_ms: u64,
}

impl SchedulerClock {
    pub fn new(server_session_start: Instant, configured_delay_ms: u64) -> Self {
        Self {
            server_session_start,
            configured_delay_ms,
        }
    }

    pub fn play_deadline(&self, capture_ts_ms: u64) -> Instant {
        self.server_session_start
            + Duration::from_millis(capture_ts_ms.saturating_add(self.configured_delay_ms))
    }

    pub fn play_cursor_ms(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.server_session_start)
            .as_millis()
            .saturating_sub(self.configured_delay_ms as u128) as u64
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn computes_deadline() {
        let start = Instant::now();
        let clock = SchedulerClock::new(start, 1_000);

        let deadline = clock.play_deadline(500);

        assert_eq!(deadline.duration_since(start), Duration::from_millis(1_500));
    }
}
