//! SessionRecorder writes raw video (fMP4) and host audio (PCM) to disk.
//!
//! Recording is gated by tier: only tier >= 4 sessions produce output.
//! All write methods are no-ops when `enabled == false`.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing::{info, error};

use crate::core::config::RECORDING_DIR;

const BUF_CAPACITY: usize = 256 * 1024; // 256KB
const MIN_RECORDING_TIER: u8 = 4;

pub struct SessionRecorder {
    session_id: String,
    output_dir: PathBuf,
    video_file: Option<Mutex<BufWriter<File>>>,
    host_audio_file: Option<Mutex<BufWriter<File>>>,
    enabled: bool,
}

impl SessionRecorder {
    pub fn new(session_id: &str, tier: u8) -> Self {
        let output_dir = PathBuf::from(RECORDING_DIR).join(session_id);
        if tier < MIN_RECORDING_TIER {
            return Self::disabled(session_id, output_dir);
        }
        match Self::open_files(session_id, &output_dir) {
            Ok((video, audio)) => Self::enabled(session_id, output_dir, video, audio),
            Err(e) => {
                error!("[RECORDER:{}] failed to open files: {}", session_id, e);
                Self::disabled(session_id, output_dir)
            }
        }
    }

    pub fn write_video_chunk(&self, data: &[u8]) {
        if !self.enabled { return; }
        Self::append(&self.video_file, data, "video", &self.session_id);
    }

    pub fn write_host_audio(&self, pcm: &[u8]) {
        if !self.enabled { return; }
        Self::append(&self.host_audio_file, pcm, "audio", &self.session_id);
    }

    pub fn finalize(&self) {
        if !self.enabled { return; }
        Self::flush_and_sync(&self.video_file, "video", &self.session_id);
        Self::flush_and_sync(&self.host_audio_file, "audio", &self.session_id);
        info!("[RECORDER:{}] finalized at {}", self.session_id, self.output_dir.display());
    }

    pub fn output_dir(&self) -> &Path { &self.output_dir }

    pub fn is_enabled(&self) -> bool { self.enabled }
}

// ── Constructors ────────────────────────────────────────────────────────────

impl SessionRecorder {
    fn disabled(session_id: &str, output_dir: PathBuf) -> Self {
        Self {
            session_id: session_id.to_string(),
            output_dir,
            video_file: None,
            host_audio_file: None,
            enabled: false,
        }
    }

    fn enabled(session_id: &str, output_dir: PathBuf, video: File, audio: File) -> Self {
        info!("[RECORDER:{}] recording to {}", session_id, output_dir.display());
        Self {
            session_id: session_id.to_string(),
            output_dir,
            video_file: Some(Mutex::new(BufWriter::with_capacity(BUF_CAPACITY, video))),
            host_audio_file: Some(Mutex::new(BufWriter::with_capacity(BUF_CAPACITY, audio))),
            enabled: true,
        }
    }

    fn open_files(session_id: &str, dir: &Path) -> Result<(File, File), String> {
        fs::create_dir_all(dir)
            .map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
        let video = open_file(&dir.join("video.fmp4"), session_id)?;
        let audio = open_file(&dir.join("host_audio.pcm"), session_id)?;
        Ok((video, audio))
    }
}

// ── I/O helpers ─────────────────────────────────────────────────────────────

fn open_file(path: &Path, session_id: &str) -> Result<File, String> {
    File::create(path).map_err(|e| {
        format!("[RECORDER:{}] create {}: {}", session_id, path.display(), e)
    })
}

impl SessionRecorder {
    fn append(
        handle: &Option<Mutex<BufWriter<File>>>,
        data: &[u8],
        label: &str,
        session_id: &str,
    ) {
        let Some(lock) = handle else { return };
        match lock.lock() {
            Ok(mut w) => {
                if let Err(e) = w.write_all(data) {
                    error!("[RECORDER:{}] {} write error: {}", session_id, label, e);
                }
            }
            Err(e) => error!("[RECORDER:{}] {} lock poisoned: {}", session_id, label, e),
        }
    }

    fn flush_and_sync(
        handle: &Option<Mutex<BufWriter<File>>>,
        label: &str,
        session_id: &str,
    ) {
        let Some(lock) = handle else { return };
        match lock.lock() {
            Ok(mut w) => {
                let _ = w.flush();
                if let Err(e) = w.get_ref().sync_all() {
                    error!("[RECORDER:{}] {} sync error: {}", session_id, label, e);
                }
            }
            Err(e) => error!("[RECORDER:{}] {} lock poisoned: {}", session_id, label, e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_disable_recording_when_tier_below_minimum() {
        let rec = SessionRecorder::new("test-low-tier", 3);

        assert!(!rec.is_enabled());
        assert!(rec.video_file.is_none());
        assert!(rec.host_audio_file.is_none());
    }

    #[test]
    fn should_enable_recording_when_tier_meets_minimum() {
        let rec = SessionRecorder::new("test-tier4", 4);

        assert!(rec.is_enabled());
        assert!(rec.video_file.is_some());
        assert!(rec.host_audio_file.is_some());

        // Cleanup
        let _ = std::fs::remove_dir_all(rec.output_dir());
    }

    #[test]
    fn should_set_output_dir_from_session_id() {
        let rec = SessionRecorder::new("test-dir", 1);

        let expected = PathBuf::from(RECORDING_DIR).join("test-dir");
        assert_eq!(rec.output_dir(), expected);
    }

    #[test]
    fn should_write_video_chunk_to_file() {
        let rec = SessionRecorder::new("test-video-write", 4);
        let payload = b"fake-fmp4-data";

        rec.write_video_chunk(payload);
        rec.finalize();

        let written = std::fs::read(rec.output_dir().join("video.fmp4")).unwrap();
        assert_eq!(written, payload);

        let _ = std::fs::remove_dir_all(rec.output_dir());
    }

    #[test]
    fn should_write_host_audio_to_file() {
        let rec = SessionRecorder::new("test-audio-write", 4);
        let payload = b"fake-pcm-data";

        rec.write_host_audio(payload);
        rec.finalize();

        let written = std::fs::read(rec.output_dir().join("host_audio.pcm")).unwrap();
        assert_eq!(written, payload);

        let _ = std::fs::remove_dir_all(rec.output_dir());
    }

    #[test]
    fn should_noop_write_when_disabled() {
        let rec = SessionRecorder::new("test-noop", 1);

        rec.write_video_chunk(b"should-not-crash");
        rec.write_host_audio(b"should-not-crash");
        rec.finalize();
    }

    #[test]
    fn should_accumulate_multiple_writes() {
        let rec = SessionRecorder::new("test-multi-write", 4);

        rec.write_video_chunk(b"chunk1");
        rec.write_video_chunk(b"chunk2");
        rec.finalize();

        let written = std::fs::read(rec.output_dir().join("video.fmp4")).unwrap();
        assert_eq!(written, b"chunk1chunk2");

        let _ = std::fs::remove_dir_all(rec.output_dir());
    }
}
