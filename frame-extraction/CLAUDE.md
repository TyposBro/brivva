# brivva-frames — Real-Time Video Frame Extraction & Face Cropping Pipeline

## Purpose

Rust CLI tool that simulates the preprocessing pipeline for MuseTalk lip-sync integration.
Reads video → extracts frames in real-time → crops face regions (256x256) → outputs processed frames.
Built to demonstrate Rust proficiency for Brivva interview and to prototype the lip-sync preprocessing step.

## What It Does

```
brivva-frames extract video.mp4 --fps 25 --face-size 256
```

1. Spawns FFmpeg as a child process to decode video into raw RGB frames (piped to stdout)
2. Reads raw RGB byte chunks from FFmpeg's stdout (width × height × 3 bytes per frame)
3. Detects face region in each frame (start with center crop, upgrade to detection later)
4. Crops face to 256×256 (MuseTalk's required input size)
5. Outputs cropped frames to `output/` directory
6. Prints real-time stats: FPS, latency per frame, total frames processed

## Architecture

```
FFmpeg (child process)
  → stdout pipe (raw RGB24 frames)
  → Rust: read fixed-size chunks into Vec<u8> buffer
  → Rust: convert to image::RgbImage
  → Rust: crop face region (256×256)
  → Rust: save to output/ (or stream via channel)
  → Stats: fps counter, per-frame timing
```

## Build & Run

```bash
# Prerequisites: FFmpeg installed and in PATH
# On NixOS: nix-shell -p ffmpeg
# On Mac: brew install ffmpeg

# Build
cargo build --release

# Run — extract + crop from video
cargo run --release -- extract input.mp4 --fps 25 --face-size 256 --output output/

# Run — webcam mode (Linux/Mac)
cargo run --release -- webcam --fps 25 --face-size 256 --output output/

# Run — benchmark mode (process as fast as possible, no saving)
cargo run --release -- benchmark input.mp4
```

## Project Structure

```
brivva-frames/
├── CLAUDE.md          ← you are here
├── Cargo.toml
└── src/
    ├── main.rs        ← CLI entry point (clap), dispatches to commands
    ├── ffmpeg.rs      ← spawn FFmpeg, read raw frames from stdout pipe
    ├── face.rs        ← face region detection + cropping
    ├── pipeline.rs    ← orchestrates: read frame → detect → crop → output
    └── stats.rs       ← FPS counter, per-frame latency tracking
```

## Implementation Plan — Build in This Order

### Step 1: FFmpeg frame reader (ffmpeg.rs)

Spawn FFmpeg as child process, pipe stdout, read raw RGB frames.

```rust
use std::process::{Command, Stdio};
use std::io::{BufReader, Read};

pub struct FrameReader {
    reader: BufReader<std::process::ChildStdout>,
    width: u32,
    height: u32,
    frame_size: usize, // width * height * 3 (RGB24)
    buffer: Vec<u8>,
}

impl FrameReader {
    pub fn from_file(path: &str, fps: u32) -> anyhow::Result<Self> {
        // First, probe video dimensions using ffprobe
        // Then spawn:
        // ffmpeg -i {path} -f rawvideo -pix_fmt rgb24 -r {fps} -v quiet pipe:1
        // Read from child.stdout
    }

    pub fn from_webcam(fps: u32) -> anyhow::Result<Self> {
        // Linux: ffmpeg -f v4l2 -i /dev/video0 -f rawvideo -pix_fmt rgb24 -r {fps} pipe:1
        // Mac: ffmpeg -f avfoundation -i "0" -f rawvideo -pix_fmt rgb24 -r {fps} pipe:1
    }

    pub fn next_frame(&mut self) -> anyhow::Result<Option<image::RgbImage>> {
        // Read exactly frame_size bytes from reader
        // Convert to RgbImage::from_raw(width, height, buffer)
        // Return None on EOF
    }
}
```

Key Rust concepts:

- `Command::new("ffmpeg")` with `.stdout(Stdio::piped())` — ownership of child process
- `BufReader` wrapping `ChildStdout` — buffered reading from pipe
- `Vec<u8>` buffer — owned heap allocation for frame data
- `read_exact()` — reads exactly N bytes or returns error
- `image::RgbImage::from_raw()` — takes ownership of the Vec<u8>

### Step 2: Face cropping (face.rs)

Start simple with center crop. No ML face detection yet.

```rust
use image::{RgbImage, imageops};

pub struct FaceCropper {
    face_size: u32,
}

impl FaceCropper {
    pub fn new(face_size: u32) -> Self {
        Self { face_size }
    }

    pub fn crop_center(&self, frame: &RgbImage) -> RgbImage {
        // Calculate center crop coordinates
        // Use imageops::crop_imm() — borrows the image (no copy)
        // Then resize to face_size × face_size
        // Return owned RgbImage
    }
}
```

Key Rust concepts:

- `&RgbImage` — immutable borrow (we don't take ownership of the frame)
- `crop_imm()` returns a view (borrow), `to_image()` creates owned copy
- `imageops::resize()` — returns new owned RgbImage

### Step 3: Pipeline orchestrator (pipeline.rs)

Wire frame reader → face cropper → output.

```rust
pub fn run_extract(
    source: &str,
    fps: u32,
    face_size: u32,
    output_dir: &str,
) -> anyhow::Result<PipelineStats> {
    let mut reader = FrameReader::from_file(source, fps)?;
    let cropper = FaceCropper::new(face_size);
    let mut stats = Stats::new();
    let mut frame_num = 0u32;

    while let Some(frame) = reader.next_frame()? {
        let start = Instant::now();
        let face = cropper.crop_center(&frame);
        face.save(format!("{output_dir}/{frame_num:04}.png"))?;
        stats.record(start.elapsed());
        frame_num += 1;
    }

    Ok(stats.finalize())
}
```

Key Rust concepts:

- `while let Some(frame) = ...` — pattern matching on Option
- `?` operator — propagate errors up
- `&frame` passed to cropper — borrow, pipeline keeps ownership
- `Instant::now()` + `elapsed()` — zero-cost timing

### Step 4: Stats tracker (stats.rs)

```rust
pub struct Stats {
    frame_times: Vec<Duration>,
    start: Instant,
}

impl Stats {
    pub fn new() -> Self { ... }
    pub fn record(&mut self, elapsed: Duration) { ... }
    pub fn finalize(self) -> PipelineStats { ... } // takes ownership (self, not &self)
    // Print: total frames, avg fps, avg/min/max latency per frame
}
```

Key Rust concepts:

- `finalize(self)` — takes ownership, consumes the Stats (can't use it after)
- `&mut self` on record — mutable borrow (we're modifying internal state)

### Step 5: CLI with clap (main.rs)

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "brivva-frames")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Extract {
        input: String,
        #[arg(long, default_value_t = 25)]
        fps: u32,
        #[arg(long, default_value_t = 256)]
        face_size: u32,
        #[arg(long, default_value = "output")]
        output: String,
    },
    Webcam { ... },
    Benchmark { input: String },
}
```

### Step 6 (stretch): Parallel frame processing with rayon

```rust
use rayon::prelude::*;

// Read all frames first, then process in parallel
let frames: Vec<RgbImage> = reader.collect_all()?;
let faces: Vec<RgbImage> = frames
    .par_iter()        // parallel iterator
    .map(|frame| cropper.crop_center(frame))
    .collect();
```

Key Rust concepts:

- `par_iter()` — rayon's parallel iterator, requires items to be Send + Sync
- `.map()` on parallel iterator — each closure runs on a different thread
- Cropper must be `Sync` (safe to share between threads via &reference)

### Step 7 (stretch): Channel-based streaming pipeline

```rust
use std::sync::mpsc;
use std::thread;

let (tx, rx) = mpsc::channel::<RgbImage>();

// Reader thread — reads frames and sends through channel
let reader_handle = thread::spawn(move || {
    while let Some(frame) = reader.next_frame().unwrap() {
        tx.send(frame).unwrap(); // moves frame into channel
    }
    // tx dropped here → channel closes → rx returns None
});

// Processor — receives frames and crops
for frame in rx {
    let face = cropper.crop_center(&frame);
    face.save(...)?;
}
```

Key Rust concepts:

- `mpsc::channel` — same pattern as your Brivva server (mpsc channels for WebSocket)
- `move` closure — takes ownership of reader and tx
- `tx.send(frame)` — moves frame through channel (ownership transfer)
- When tx drops, rx iterator ends — same cleanup pattern as host disconnect in server-rs

## Dependencies (Cargo.toml)

```toml
[package]
name = "brivva-frames"
version = "0.1.0"
edition = "2021"

[dependencies]
image = "0.25"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
rayon = "1.10"

[profile.release]
opt-level = 3
```

## Key Rust Concepts This Project Teaches

| Concept            | Where You'll Use It                                                      |
| ------------------ | ------------------------------------------------------------------------ |
| Ownership + Move   | Frame buffers moving from reader → pipeline → output                     |
| Borrowing (&)      | Cropper borrows frames without taking ownership                          |
| &mut self          | Stats.record() mutates internal state                                    |
| self (consuming)   | Stats.finalize() consumes the struct                                     |
| Error handling (?) | Every FFmpeg/IO/image operation can fail                                 |
| Pattern matching   | while let Some(frame), match on CLI commands                             |
| Enums              | CLI subcommands, could extend to CropStrategy enum                       |
| Traits (Send/Sync) | rayon parallel processing requires thread-safe types                     |
| Channels (mpsc)    | Streaming pipeline between reader and processor threads                  |
| Process spawning   | FFmpeg as child process with piped stdout                                |
| Drop semantics     | FFmpeg child killed when FrameReader drops, channel closes when tx drops |

## Rules for the Developer

1. **Write the code yourself first.** Use Claude Code only when stuck for > 15 minutes.
2. **Understand every line.** If Claude Code writes something, read it and make sure you can explain it.
3. **Run after every step.** Don't build all 7 steps then test. Build step 1, test, step 2, test, etc.
4. **Paper trace the ownership.** For each function, know: who owns the frame buffer? Who borrows it? When is it dropped?
5. **No unsafe code.** If you think you need unsafe, you're doing it wrong.

## Why This Matters for Brivva Interview

"I built a real-time frame extraction pipeline in Rust. It spawns FFmpeg, reads raw RGB frames from a pipe, crops face regions at 256×256 for MuseTalk input, and measures throughput. I used channels for streaming between reader and processor — same pattern as the mpsc channels in my translation server. The ownership model made it easy to reason about frame buffer lifetimes — each frame is read, borrowed for cropping, then dropped automatically."

That's a 20-second answer that demonstrates real Rust understanding.
