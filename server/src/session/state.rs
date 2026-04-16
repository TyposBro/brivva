#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    pub delay_ms: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self { delay_ms: 1_000 }
    }
}
