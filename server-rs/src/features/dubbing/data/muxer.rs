//! FFmpeg muxing operations for dubbing workflow.

use std::path::Path;
use std::process::Stdio;
use tokio::process::Command as TokioCommand;

use crate::features::broadcast::data::streaming::FFMPEG_BIN;

/// Mux raw video (fMP4) with host audio (PCM s16le 44100Hz mono) into an MP4.
pub async fn mux_fmp4_with_audio(
    video_fmp4: &Path,
    host_audio_pcm: &Path,
    output_mp4: &Path,
) -> Result<(), String> {
    let output = TokioCommand::new(&*FFMPEG_BIN)
        .args([
            "-y",
            "-i", &video_fmp4.to_string_lossy(),
            "-f", "s16le", "-ar", "44100", "-ac", "1",
            "-i", &host_audio_pcm.to_string_lossy(),
            "-c:v", "copy",
            "-c:a", "aac",
            &output_mp4.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("ffmpeg mux spawn failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg mux failed: {stderr}"));
    }
    Ok(())
}

/// Mux original video with dubbed audio (MP3) into a final MP4.
pub async fn mux_video_with_dubbed_audio(
    video_fmp4: &Path,
    dubbed_audio: &Path,
    output: &Path,
) -> Result<(), String> {
    let result = TokioCommand::new(&*FFMPEG_BIN)
        .args([
            "-y",
            "-i", &video_fmp4.to_string_lossy(),
            "-i", &dubbed_audio.to_string_lossy(),
            "-c:v", "copy",
            "-c:a", "copy",
            "-map", "0:v",
            "-map", "1:a",
            &output.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("ffmpeg final mux spawn failed: {e}"))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("ffmpeg final mux failed: {stderr}"));
    }
    Ok(())
}
