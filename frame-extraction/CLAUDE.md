# brivva-frames — Real-Time Video Frame Extraction & Face Cropping Pipeline

## Purpose

Rust CLI tool that simulates the preprocessing pipeline for MuseTalk lip-sync integration.
Reads video → extracts frames in real-time → crops face regions (256x256) → outputs processed frames.
Built to demonstrate Rust proficiency for Brivva interview and to prototype the lip-sync preprocessing step.

## Current Status (Mar 17 2026)

**All 7 steps implemented and working.**

- Steps 1-5: Core pipeline (FFmpeg reader, face cropper, pipeline, stats, CLI)
- Step 6: Parallel batch processing with rayon (for `extract` command)
- Step 7: Channel-based streaming pipeline (for `webcam` command)
- Live preview window via minifb with side-by-side view
- Webcam works at 640x480 and 1280x720 at 30fps
- 1080p has rendering issues (minifb/macOS — frames process but window stays dark)

## What It Does

```
# Extract + crop from video file (parallel with rayon)
brivva-frames extract video.mp4 --fps 25 --face-size 256 --output output/

# Live webcam with real-time preview window
brivva-frames webcam --fps 30 --face-size 256 --width 640 --height 480

# Benchmark (process as fast as possible, no saving, no display)
brivva-frames benchmark input.mp4
```

## Architecture

### Extract mode (batch, parallel)
```
FFmpeg (child process) → stdout pipe (raw RGB24)
  → Rust: read ALL frames into Vec<RgbImage>
  → rayon par_iter: crop + save in parallel across all CPU cores
  → Stats: total time, per-frame latency
```

### Webcam mode (streaming, channel-based)
```
Reader Thread                          Main Thread
  FFmpeg (avfoundation/v4l2)             ┌─ try_recv (drain to latest)
  → read raw RGB24 frames               │  ↓
  → pre-resize thumbnail                │  crop_center (256×256)
  → sync_channel(2) ──────────────────► │  compose side-by-side preview
                                         │  update minifb window
                                         │  update title bar stats
                                         └─ loop until ESC
```

### Benchmark mode
```
FFmpeg → read frames → crop (no save, no display) → stats
```

## Build & Run

```bash
# Prerequisites: FFmpeg installed and in PATH
# Mac: brew install ffmpeg

cargo build --release

# Extract from video (parallel)
cargo run --release -- extract input.mp4 --fps 25 --face-size 256 --output output/

# Webcam (640x480, live preview window)
cargo run --release -- webcam

# Webcam 720p
cargo run --release -- webcam --width 1280 --height 720

# Benchmark
cargo run --release -- benchmark input.mp4
```

## Project Structure

```
brivva-frames/
├── CLAUDE.md          ← you are here
├── Cargo.toml
├── .gitignore
└── src/
    ├── main.rs        ← CLI (clap): extract, webcam, benchmark commands
    ├── ffmpeg.rs      ← FrameReader: spawn FFmpeg, ffprobe, read raw RGB frames
    ├── face.rs        ← FaceCropper: center crop + resize, draw_rect helper
    ├── pipeline.rs    ← run_extract (rayon), run_webcam (mpsc + minifb), run_benchmark
    └── stats.rs       ← Stats/PipelineStats: per-frame timing, FPS, Display impl
```

## Dependencies

```toml
image = "0.25"          # RgbImage, crop, resize
clap = "4" (derive)     # CLI argument parsing
anyhow = "1"            # Error handling
rayon = "1.11"          # Parallel batch processing (Step 6)
minifb = "0.28"         # Live preview window
```

## Measured Performance

| Resolution | FPS   | Avg Latency | Min    | Max     |
| ---------- | ----- | ----------- | ------ | ------- |
| 640x480    | ~28   | 5.86ms      | 1.95ms | 9.86ms  |
| 1280x720   | ~28   | 7.95ms      | 5.80ms | 9.48ms  |

Latency = crop + compose time per frame (excludes FFmpeg decode).

## Key Implementation Details

### FFmpeg integration (ffmpeg.rs)
- `ffprobe` probes video dimensions before spawning ffmpeg (for file mode)
- `-f rawvideo -pix_fmt rgb24` outputs uncompressed RGB bytes to pipe
- macOS webcam: `-f avfoundation -pixel_format uyvy422 -video_size WxH -i "0:none"`
- Low-latency flags: `-fflags nobuffer -flags low_delay`
- `BufReader` wraps `ChildStdout` for efficient pipe reads
- `Drop` impl kills child process on cleanup
- Zero-copy frame handoff via `mem::swap` (avoids 900KB clone per frame)

### Face cropping (face.rs)
- Center crop: takes min(width, height) square from center
- `crop_imm()` borrows frame (no copy), `to_image()` creates owned copy
- Resize to face_size with Triangle filter
- `crop_region()` returns coordinates for drawing the preview rectangle
- `draw_rect()` draws colored borders directly on RgbImage (no extra deps)

### Pipeline (pipeline.rs)
- **Extract**: reads all frames → `par_iter().map(crop).collect()` via rayon
- **Webcam**: `sync_channel(2)` between reader thread and main thread
  - Reader thread: reads frames + pre-resizes thumbnails
  - Main thread: `try_recv` with drain-to-latest (skips stale frames)
  - Side-by-side preview: [full frame + green crop rect | cropped face]
  - Stats in window title bar (FPS, crop latency, frame count)
  - 60fps display cap gives macOS compositor time to paint

### Stats (stats.rs)
- `Stats` collects per-frame `Duration` measurements
- `finalize(self)` consumes Stats → returns `PipelineStats` (total, avg, min, max, fps)
- `Display` impl for pretty terminal output

## macOS Camera Quirks
- FaceTime HD Camera supports: 15fps or 30fps only (not 25)
- Supported resolutions: 640x480, 1280x720, 1920x1080, 1760x1328, 1552x1552
- Must use `-pixel_format uyvy422` for avfoundation (default yuv420p not supported)
- Must use `"0:none"` as input (video_device:audio_device, none disables audio)
- Camera needs `-video_size` flag explicitly
- 1080p: frames process correctly but minifb preview window renders dark (known issue)

## Known Issues
- **1080p preview dark**: minifb window stays black at 1920x1080 input despite frames processing. 720p and below work fine. Likely macOS/minifb compositing issue.
- **Workspace resolver warning**: parent Cargo.toml uses edition 2024, this crate uses 2021

## Key Rust Concepts Demonstrated

| Concept            | Where Used                                                               |
| ------------------ | ------------------------------------------------------------------------ |
| Ownership + Move   | Frame buffers moving from reader → channel → pipeline → output           |
| Borrowing (&)      | Cropper borrows frames without taking ownership                          |
| &mut self          | Stats.record() mutates internal state                                    |
| self (consuming)   | Stats.finalize() consumes the struct                                     |
| Error handling (?) | Every FFmpeg/IO/image operation can fail                                 |
| Pattern matching   | while let Some(frame), match on CLI commands, try_recv results           |
| Enums              | CLI subcommands (Extract, Webcam, Benchmark)                             |
| Traits (Send/Sync) | rayon parallel processing requires thread-safe types                     |
| Channels (mpsc)    | sync_channel between reader thread and display thread                    |
| Process spawning   | FFmpeg as child process with piped stdout                                |
| Drop semantics     | FFmpeg child killed when FrameReader drops, channel closes when tx drops |
| mem::swap          | Zero-copy buffer handoff in frame reader                                 |
| Closures (move)    | Reader thread closure takes ownership of FrameReader and tx              |

## Why This Matters for Brivva Interview

"I built a real-time frame extraction pipeline in Rust. It spawns FFmpeg, reads raw RGB frames from a pipe, crops face regions at 256×256 for MuseTalk input, and displays a live preview with the crop region highlighted. For batch processing I use rayon's parallel iterators across all CPU cores. For real-time webcam mode I use mpsc channels between a reader thread and display thread — same pattern as the mpsc channels in my translation server. The ownership model made it easy to reason about frame buffer lifetimes — each frame is read, moved through the channel, borrowed for cropping, then dropped automatically."
