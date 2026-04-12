//! REST handlers for the dubbing API.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio_util::io::ReaderStream;

use crate::core::config::RECORDING_DIR;
use crate::features::dubbing::domain::{DubbingJob, DubbingJobStatus, DubbingJobs};
use super::job_runner;

/// Dependencies injected from orchestration.
pub struct DubbingDeps {
    pub tts_api_key: String,
    pub http_client: reqwest::Client,
}

#[derive(Deserialize)]
pub struct StartDubbingRequest {
    pub session_id: String,
    pub lang: String,
    pub source_lang: String,
}

#[derive(Serialize)]
pub struct StartDubbingResponse {
    pub job_id: String,
    pub status: DubbingJobStatus,
}

/// POST /api/dubbing/start — spawn a new dubbing job for the given language.
pub async fn start_handler(
    axum::Extension(deps): axum::Extension<Arc<DubbingDeps>>,
    axum::Extension(jobs): axum::Extension<DubbingJobs>,
    Json(body): Json<StartDubbingRequest>,
) -> Result<Json<StartDubbingResponse>, (StatusCode, String)> {
    let recording_dir = PathBuf::from(RECORDING_DIR).join(&body.session_id);
    validate_recording_dir(&recording_dir)?;

    let job_id = format!("{}_{}", body.session_id, body.lang);
    let job = DubbingJob::new(job_id.clone(), body.session_id.clone(), body.lang.clone());
    insert_job(&jobs, &job_id, job);

    spawn_job(
        job_id.clone(),
        recording_dir,
        body.source_lang,
        body.lang,
        deps,
        jobs,
    );

    Ok(Json(StartDubbingResponse {
        job_id,
        status: DubbingJobStatus::Pending,
    }))
}

/// GET /api/dubbing/status/:job_id — return current status of a single job.
pub async fn status_handler(
    axum::Extension(jobs): axum::Extension<DubbingJobs>,
    Path(job_id): Path<String>,
) -> Result<Json<DubbingJob>, (StatusCode, String)> {
    let map = jobs.lock().map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("lock error: {e}"))
    })?;
    let job = map.get(&job_id).ok_or((
        StatusCode::NOT_FOUND,
        format!("job not found: {job_id}"),
    ))?;
    Ok(Json(job.clone()))
}

/// GET /api/dubbing/jobs/:session_id — return all jobs for a session.
pub async fn session_jobs_handler(
    axum::Extension(jobs): axum::Extension<DubbingJobs>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<DubbingJob>>, (StatusCode, String)> {
    let map = jobs.lock().map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("lock error: {e}"))
    })?;
    let session_jobs: Vec<DubbingJob> = map
        .values()
        .filter(|j| j.session_id == session_id)
        .cloned()
        .collect();
    Ok(Json(session_jobs))
}

/// GET /api/dubbing/download/:job_id — stream the output MP4 file.
pub async fn download_handler(
    axum::Extension(jobs): axum::Extension<DubbingJobs>,
    Path(job_id): Path<String>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let output_path = get_output_path(&jobs, &job_id)?;
    let file = tokio::fs::File::open(&output_path).await.map_err(|e| {
        (StatusCode::NOT_FOUND, format!("file not found: {e}"))
    })?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let filename = output_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("dubbed.mp4");

    Ok(axum::response::Response::builder()
        .header("Content-Type", "video/mp4")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(body)
        .unwrap())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn validate_recording_dir(dir: &std::path::Path) -> Result<(), (StatusCode, String)> {
    if !dir.join("video.fmp4").exists() {
        return Err((
            StatusCode::BAD_REQUEST,
            "recording not found for session".into(),
        ));
    }
    Ok(())
}

fn insert_job(jobs: &DubbingJobs, job_id: &str, job: DubbingJob) {
    if let Ok(mut map) = jobs.lock() {
        map.insert(job_id.to_string(), job);
    }
}

fn spawn_job(
    job_id: String,
    recording_dir: PathBuf,
    source_lang: String,
    target_lang: String,
    deps: Arc<DubbingDeps>,
    jobs: DubbingJobs,
) {
    tokio::spawn(async move {
        let result = job_runner::run_dubbing_job(
            &job_id,
            &recording_dir,
            &source_lang,
            &target_lang,
            &deps.tts_api_key,
            &deps.http_client,
            &jobs,
        )
        .await;
        finalize_job(&jobs, &job_id, result);
    });
}

fn finalize_job(jobs: &DubbingJobs, job_id: &str, result: Result<PathBuf, String>) {
    if let Ok(mut map) = jobs.lock() {
        if let Some(job) = map.get_mut(job_id) {
            match result {
                Ok(path) => {
                    tracing::info!("[DUBBING:{}] complete: {}", job_id, path.display());
                    job.status = DubbingJobStatus::Complete;
                    job.output_path = Some(path);
                }
                Err(e) => {
                    tracing::error!("[DUBBING:{}] failed: {}", job_id, e);
                    job.status = DubbingJobStatus::Failed;
                    job.error = Some(e);
                }
            }
        }
    }
}

fn get_output_path(
    jobs: &DubbingJobs,
    job_id: &str,
) -> Result<PathBuf, (StatusCode, String)> {
    let map = jobs.lock().map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("lock error: {e}"))
    })?;
    let job = map.get(job_id).ok_or((
        StatusCode::NOT_FOUND,
        format!("job not found: {job_id}"),
    ))?;
    job.output_path.clone().ok_or((
        StatusCode::BAD_REQUEST,
        "dubbing not yet complete".into(),
    ))
}
