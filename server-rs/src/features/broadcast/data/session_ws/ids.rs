use uuid::Uuid;

use crate::features::broadcast::domain::LiveSession;

/// Generate a fresh 6-char uppercase hex code for a live-session identifier.
pub(super) fn generate_live_session_id() -> String {
    Uuid::new_v4().to_string()[..6].to_uppercase()
}

/// Pick a live-session id that does not collide with any existing row in the
/// map. Tries 16 short candidates; if we truly can't find a free one, falls
/// back to a 10-char id (collision probability is negligible at that point).
pub(super) fn next_available_live_session_id(
    live_sessions: &dashmap::DashMap<String, LiveSession>,
) -> String {
    for _ in 0..16 {
        let candidate = generate_live_session_id();
        if !live_sessions.contains_key(&candidate) {
            return candidate;
        }
    }

    loop {
        let candidate = Uuid::new_v4().simple().to_string()[..10].to_uppercase();
        if !live_sessions.contains_key(&candidate) {
            return candidate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::broadcast::domain::{Lang, PipelineConfig};
    use std::sync::Arc;

    #[test]
    fn next_available_live_session_id_skips_existing_entries() {
        let live_sessions = dashmap::DashMap::new();
        live_sessions.insert(
            "ABC123".into(),
            LiveSession::new(
                "ABC123".into(),
                Lang::En,
                None,
                Arc::new(PipelineConfig::default()),
            ),
        );

        for _ in 0..32 {
            let id = next_available_live_session_id(&live_sessions);
            assert_ne!(id, "ABC123");
            assert!(!id.is_empty());
        }
    }

    #[test]
    fn generate_live_session_id_produces_6_char_uppercase_hex() {
        for _ in 0..8 {
            let id = generate_live_session_id();
            assert_eq!(id.len(), 6);
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
            );
        }
    }
}
