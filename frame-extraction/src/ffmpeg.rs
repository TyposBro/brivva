use anyhow::{bail, Context};
use std::io::{BufReader, Read};
use std::process::{Child, Command, Stdio};

pub struct FrameReader {
    reader: BufReader<std::process::ChildStdout>,
    child: Child,
    width: u32,
    height: u32,
    frame_size: usize,
    buffer: Vec<u8>,
}

fn probe_dimensions(path: &str) -> anyhow::Result<(u32, u32)> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0:s=x",
            path,
        ])
        .output()
        .context("failed to run ffprobe — is FFmpeg installed?")?;

    if !output.status.success() {
        bail!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    let (w, h) = text
        .split_once('x')
        .context("unexpected ffprobe output format")?;

    let width: u32 = w.parse().context("invalid width")?;
    let height: u32 = h.parse().context("invalid height")?;

    Ok((width, height))
}

impl FrameReader {
    pub fn from_file(path: &str, fps: u32) -> anyhow::Result<Self> {
        let (width, height) = probe_dimensions(path)?;

        let mut child = Command::new("ffmpeg")
            .args([
                "-i",
                path,
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-r",
                &fps.to_string(),
                "-v",
                "quiet",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to spawn ffmpeg")?;

        let stdout = child
            .stdout
            .take()
            .context("failed to capture ffmpeg stdout")?;
        let frame_size = (width as usize) * (height as usize) * 3;

        Ok(Self {
            reader: BufReader::new(stdout),
            child,
            width,
            height,
            frame_size,
            buffer: vec![0u8; frame_size],
        })
    }

    pub fn from_webcam(fps: u32, width: u32, height: u32) -> anyhow::Result<Self> {
        let fps_str = fps.to_string();
        let scale = format!("scale={}:{}", width, height);

        let mut args = if cfg!(target_os = "macos") {
            vec![
                "-f".to_string(),
                "avfoundation".to_string(),
                "-framerate".to_string(),
                fps_str.clone(),
                "-i".to_string(),
                "0".to_string(),
            ]
        } else {
            vec![
                "-f".to_string(),
                "v4l2".to_string(),
                "-framerate".to_string(),
                fps_str.clone(),
                "-i".to_string(),
                "/dev/video0".to_string(),
            ]
        };

        args.extend([
            "-vf".to_string(),
            scale,
            "-f".to_string(),
            "rawvideo".to_string(),
            "-pix_fmt".to_string(),
            "rgb24".to_string(),
            "-r".to_string(),
            fps_str,
            "-v".to_string(),
            "quiet".to_string(),
            "pipe:1".to_string(),
        ]);

        let mut child = Command::new("ffmpeg")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to spawn ffmpeg for webcam")?;

        let stdout = child
            .stdout
            .take()
            .context("failed to capture ffmpeg stdout")?;
        let frame_size = (width as usize) * (height as usize) * 3;

        Ok(Self {
            reader: BufReader::new(stdout),
            child,
            width,
            height,
            frame_size,
            buffer: vec![0u8; frame_size],
        })
    }

    pub fn next_frame(&mut self) -> anyhow::Result<Option<image::RgbImage>> {
        match self.reader.read_exact(&mut self.buffer) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let img = image::RgbImage::from_raw(self.width, self.height, self.buffer.clone())
            .context("failed to create RgbImage from raw buffer")?;

        Ok(Some(img))
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

impl Drop for FrameReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
