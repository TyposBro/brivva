//! Dubbing job runner — orchestrates the full dubbing pipeline for one language.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::core::config::DUBBING_DIR;
use crate::shared::dubbing::{create_dubbing, poll_status, download_audio, DubbingStatus};
use super::muxer;
use crate::features::dubbing::domain::{DubbingJobStatus, DubbingJobs};

const POLL_INTERVAL: Duration = Duration::from_secs(15);
const MAX_POLL_DURATION: Duration = Duration::from_secs(60 * 60);

pub async fn run_dubbing_job(
    job_id: &str,
    recording_dir: &Path,
    source_lang: &str,
    target_lang: &str,
    api_key: &str,
    client: &reqwest::Client,
    jobs: &DubbingJobs,
) -> Result<PathBuf, String> {
    let dub_dir = PathBuf::from(DUBBING_DIR).join(job_id);
    std::fs::create_dir_all(&dub_dir)
        .map_err(|e| format!("mkdir dubbing dir: {e}"))?;

    let video_fmp4 = recording_dir.join("video.fmp4");
    let host_pcm = recording_dir.join("host_audio.pcm");
    let muxed_mp4 = recording_dir.join("video.mp4");

    // Step 1: Mux video + host audio into MP4 (skip if already exists)
    if !muxed_mp4.exists() {
        update_status(jobs, job_id, DubbingJobStatus::Muxing);
        muxer::mux_fmp4_with_audio(&video_fmp4, &host_pcm, &muxed_mp4).await?;
    }

    // Step 2: Upload to ElevenLabs Dubbing API
    update_status(jobs, job_id, DubbingJobStatus::Uploading);
    let dubbing_id = create_dubbing(
        client, api_key, &muxed_mp4, source_lang, target_lang,
    ).await?;
    set_dubbing_id(jobs, job_id, &dubbing_id);

    // Step 3: Poll until done or failed
    update_status(jobs, job_id, DubbingJobStatus::Dubbing);
    let final_status = poll_until_complete(
        client, api_key, &dubbing_id,
    ).await?;

    if let DubbingStatus::Failed(reason) = final_status {
        return Err(reason);
    }

    // Step 4: Download dubbed audio
    update_status(jobs, job_id, DubbingJobStatus::Downloading);
    let dubbed_audio_path = dub_dir.join(format!("{target_lang}.mp3"));
    download_audio(
        client, api_key, &dubbing_id, target_lang, &dubbed_audio_path,
    ).await?;

    // Step 5: Mux original video + dubbed audio into final output
    update_status(jobs, job_id, DubbingJobStatus::MuxingFinal);
    let output = dub_dir.join(format!("{target_lang}_dubbed.mp4"));
    muxer::mux_video_with_dubbed_audio(&video_fmp4, &dubbed_audio_path, &output).await?;

    Ok(output)
}

async fn poll_until_complete(
    client: &reqwest::Client,
    api_key: &str,
    dubbing_id: &str,
) -> Result<DubbingStatus, String> {
    let start = std::time::Instant::now();
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        let status = poll_status(client, api_key, dubbing_id).await?;
        match &status {
            DubbingStatus::Dubbed | DubbingStatus::Failed(_) => return Ok(status),
            _ => {}
        }
        if start.elapsed() > MAX_POLL_DURATION {
            return Err("dubbing timed out after 60 minutes".into());
        }
    }
}

fn update_status(jobs: &DubbingJobs, job_id: &str, status: DubbingJobStatus) {
    if let Ok(mut map) = jobs.lock() {
        if let Some(job) = map.get_mut(job_id) {
            tracing::info!("[DUBBING:{}] status -> {:?}", job_id, status);
            job.status = status;
        }
    }
}

fn set_dubbing_id(jobs: &DubbingJobs, job_id: &str, dubbing_id: &str) {
    if let Ok(mut map) = jobs.lock() {
        if let Some(job) = map.get_mut(job_id) {
            job.dubbing_id = Some(dubbing_id.to_string());
        }
    }
}
