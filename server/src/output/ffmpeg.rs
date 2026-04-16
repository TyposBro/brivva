use std::{
    fs::File,
    io,
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
};

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct FfmpegProcessConfig {
    pub ffmpeg_bin: String,
    pub video_input: String,
    pub audio_fifo: PathBuf,
    pub output_url: String,
    pub copy_video: bool,
}

impl Default for FfmpegProcessConfig {
    fn default() -> Self {
        Self {
            ffmpeg_bin: "ffmpeg".to_string(),
            video_input: "pipe:0".to_string(),
            audio_fifo: PathBuf::from("/tmp/brivva_audio_default"),
            output_url: "rtmp://example.com/live/key".to_string(),
            copy_video: true,
        }
    }
}

pub struct FfmpegProcess {
    pub child: Child,
    pub video_stdin: ChildStdin,
    pub audio_writer: File,
    pub audio_fifo: PathBuf,
}

#[derive(Debug, Error)]
pub enum FfmpegError {
    #[error("failed to create audio fifo: {0}")]
    Fifo(io::Error),
    #[error("failed to spawn ffmpeg: {0}")]
    Spawn(io::Error),
    #[error("ffmpeg missing stdin")]
    MissingStdin,
    #[error("failed to open audio fifo: {0}")]
    OpenAudio(io::Error),
}

impl FfmpegProcessConfig {
    pub fn args(&self) -> Vec<String> {
        // Audio FIFO must be input 0 so FFmpeg opens it before probing stdin.
        // FFmpeg probes inputs sequentially — if stdin (video) is input 0,
        // it blocks waiting for data before opening the FIFO, deadlocking
        // against our write-open of the FIFO.
        let mut args = vec![
            "-y".to_string(),
            "-loglevel".to_string(),
            "warning".to_string(),
            "-f".to_string(),
            "s16le".to_string(),
            "-ar".to_string(),
            "44100".to_string(),
            "-ac".to_string(),
            "1".to_string(),
            "-i".to_string(),
            self.audio_fifo.display().to_string(),
            "-i".to_string(),
            self.video_input.clone(),
        ];

        if self.copy_video {
            args.extend(["-c:v".to_string(), "copy".to_string()]);
        } else {
            args.extend([
                "-c:v".to_string(),
                "libx264".to_string(),
                "-preset".to_string(),
                "ultrafast".to_string(),
                "-tune".to_string(),
                "zerolatency".to_string(),
            ]);
        }

        args.extend([
            "-c:a".to_string(),
            "aac".to_string(),
            "-ac:a".to_string(),
            "2".to_string(),
            "-b:a".to_string(),
            "128k".to_string(),
            "-map".to_string(),
            "1:v".to_string(),
            "-map".to_string(),
            "0:a".to_string(),
            "-f".to_string(),
            "flv".to_string(),
            "-flvflags".to_string(),
            "no_duration_filesize".to_string(),
            "-rtmp_live".to_string(),
            "live".to_string(),
            self.output_url.clone(),
        ]);

        args
    }

    pub fn spawn(&self) -> Result<FfmpegProcess, FfmpegError> {
        create_fifo(&self.audio_fifo)?;

        let mut child = Command::new(&self.ffmpeg_bin)
            .args(self.args())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(FfmpegError::Spawn)?;

        let video_stdin = child.stdin.take().ok_or(FfmpegError::MissingStdin)?;
        let audio_writer = File::options()
            .write(true)
            .open(&self.audio_fifo)
            .map_err(FfmpegError::OpenAudio)?;

        Ok(FfmpegProcess {
            child,
            video_stdin,
            audio_writer,
            audio_fifo: self.audio_fifo.clone(),
        })
    }
}

impl FfmpegProcess {
    pub fn shutdown(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.audio_fifo);
    }
}

fn create_fifo(path: &PathBuf) -> Result<(), FfmpegError> {
    let _ = std::fs::remove_file(path);
    let status = Command::new("mkfifo")
        .arg(path)
        .status()
        .map_err(FfmpegError::Fifo)?;
    if status.success() {
        Ok(())
    } else {
        Err(FfmpegError::Fifo(io::Error::other(format!(
            "mkfifo exited with status {status}"
        ))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_copy_args() {
        let cfg = FfmpegProcessConfig::default();
        let args = cfg.args();

        assert!(args.contains(&"copy".to_string()));
        assert!(args.contains(&"aac".to_string()));
        assert!(args.contains(&"1:v".to_string()));
        assert!(args.contains(&"0:a".to_string()));
        assert_eq!(args.last().unwrap(), &cfg.output_url);
    }

    #[test]
    fn builds_reencode_args() {
        let cfg = FfmpegProcessConfig {
            copy_video: false,
            ..FfmpegProcessConfig::default()
        };
        let args = cfg.args();

        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"ultrafast".to_string()));
    }

    #[test]
    fn audio_fifo_is_input_zero() {
        let cfg = FfmpegProcessConfig::default();
        let args = cfg.args();
        let fifo_pos = args.iter().position(|a| a == &cfg.audio_fifo.display().to_string()).unwrap();
        let stdin_pos = args.iter().position(|a| a == "pipe:0").unwrap();
        assert!(fifo_pos < stdin_pos);
    }
}
