//! Dubbing API response types and status enum.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub enum DubbingStatus {
    Pending,
    Dubbing,
    Dubbed,
    Failed(String),
}

/// Raw response from POST /v1/dubbing.
#[derive(Deserialize)]
pub(super) struct CreateDubbingResponse {
    pub dubbing_id: String,
}

/// Raw response from GET /v1/dubbing/{dubbing_id}.
#[derive(Deserialize)]
pub(super) struct DubbingStatusResponse {
    pub status: String,
    pub error: Option<String>,
}

impl DubbingStatusResponse {
    pub fn into_status(self) -> DubbingStatus {
        match self.status.as_str() {
            "dubbed" => DubbingStatus::Dubbed,
            "dubbing" => DubbingStatus::Dubbing,
            "pending" => DubbingStatus::Pending,
            _ => DubbingStatus::Failed(
                self.error.unwrap_or_else(|| format!("unknown status: {}", self.status)),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_parse_dubbed_status() {
        let resp = DubbingStatusResponse { status: "dubbed".into(), error: None };
        assert_eq!(resp.into_status(), DubbingStatus::Dubbed);
    }

    #[test]
    fn should_parse_dubbing_status() {
        let resp = DubbingStatusResponse { status: "dubbing".into(), error: None };
        assert_eq!(resp.into_status(), DubbingStatus::Dubbing);
    }

    #[test]
    fn should_parse_pending_status() {
        let resp = DubbingStatusResponse { status: "pending".into(), error: None };
        assert_eq!(resp.into_status(), DubbingStatus::Pending);
    }

    #[test]
    fn should_parse_failed_with_error_message() {
        let resp = DubbingStatusResponse {
            status: "failed".into(),
            error: Some("audio too short".into()),
        };
        assert_eq!(resp.into_status(), DubbingStatus::Failed("audio too short".into()));
    }

    #[test]
    fn should_parse_unknown_status_as_failed() {
        let resp = DubbingStatusResponse { status: "broken".into(), error: None };
        assert_eq!(resp.into_status(), DubbingStatus::Failed("unknown status: broken".into()));
    }
}
