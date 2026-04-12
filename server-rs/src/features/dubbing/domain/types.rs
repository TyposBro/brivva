//! Domain types for dubbing jobs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DubbingJob {
    pub id: String,
    pub session_id: String,
    pub lang: String,
    pub status: DubbingJobStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dubbing_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DubbingJobStatus {
    Pending,
    Muxing,
    Uploading,
    Dubbing,
    Downloading,
    MuxingFinal,
    Complete,
    Failed,
}

pub type DubbingJobs = Arc<Mutex<HashMap<String, DubbingJob>>>;

impl DubbingJob {
    pub fn new(id: String, session_id: String, lang: String) -> Self {
        Self {
            id,
            session_id,
            lang,
            status: DubbingJobStatus::Pending,
            dubbing_id: None,
            output_path: None,
            error: None,
        }
    }

    pub fn is_active(&self) -> bool {
        !matches!(
            self.status,
            DubbingJobStatus::Complete | DubbingJobStatus::Failed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_create_pending_job() {
        let job = DubbingJob::new("j1".into(), "s1".into(), "ja".into());

        assert_eq!(job.status, DubbingJobStatus::Pending);
        assert!(job.dubbing_id.is_none());
        assert!(job.output_path.is_none());
        assert!(job.error.is_none());
    }

    #[test]
    fn should_report_active_for_pending_job() {
        let job = DubbingJob::new("j1".into(), "s1".into(), "ja".into());

        assert!(job.is_active());
    }

    #[test]
    fn should_report_inactive_for_complete_job() {
        let mut job = DubbingJob::new("j1".into(), "s1".into(), "ja".into());
        job.status = DubbingJobStatus::Complete;

        assert!(!job.is_active());
    }

    #[test]
    fn should_report_inactive_for_failed_job() {
        let mut job = DubbingJob::new("j1".into(), "s1".into(), "ja".into());
        job.status = DubbingJobStatus::Failed;

        assert!(!job.is_active());
    }

    #[test]
    fn should_serialize_status_as_snake_case() {
        let job = DubbingJob::new("j1".into(), "s1".into(), "ja".into());
        let json = serde_json::to_string(&job).unwrap();

        assert!(json.contains("\"pending\""));
    }
}
