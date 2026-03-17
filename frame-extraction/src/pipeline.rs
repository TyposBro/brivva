use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use image::{imageops, Rgb, RgbImage};
use minifb::{Key, Window, WindowOptions};
use rayon::prelude::*;

use crate::face::{self, FaceCropper};
use crate::ffmpeg::FrameReader;
use crate::stats::{PipelineStats, Stats};

/// Step 6: Parallel frame processing with rayon
/// Reads all frames first, then crops + saves in parallel across all CPU cores.
pub fn run_extract(
    source: &str,
    fps: u32,
    face_size: u32,
    output_dir: &str,
) -> anyhow::Result<PipelineStats> {
    fs::create_dir_all(output_dir)?;

    let mut reader = FrameReader::from_file(source, fps)?;
    let cropper = FaceCropper::new(face_size);

    println!(
        "Extracting frames from {} ({}x{}) at {} fps → {}",
        source,
        reader.width(),
        reader.height(),
        fps,
        output_dir
    );

    // Read all frames into memory
    println!("  reading frames...");
    let mut frames = Vec::new();
    while let Some(frame) = reader.next_frame()? {
        frames.push(frame);
    }
    println!("  read {} frames, processing in parallel...", frames.len());

    // Crop + save in parallel with rayon
    let start = Instant::now();
    let results: Vec<anyhow::Result<std::time::Duration>> = frames
        .par_iter()
        .enumerate()
        .map(|(i, frame)| {
            let t = Instant::now();
            let face = cropper.crop_center(frame);
            face.save(format!("{output_dir}/frame_{i:05}.png"))?;
            Ok(t.elapsed())
        })
        .collect();

    let mut stats = Stats::new();
    for result in results {
        stats.record(result?);
    }

    let total = start.elapsed();
    println!(
        "  done: {} frames in {:.2}s ({:.1} fps)",
        frames.len(),
        total.as_secs_f64(),
        frames.len() as f64 / total.as_secs_f64()
    );

    Ok(stats.finalize())
}

/// Convert RGB8 image to minifb u32 buffer (0x00RRGGBB)
fn rgb_to_u32(img: &RgbImage) -> Vec<u32> {
    img.pixels()
        .map(|p| ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32))
        .collect()
}

/// Compose side-by-side: [thumbnail with crop rect | cropped face]
/// Thumbnail is pre-resized in the reader thread for performance.
fn compose_with_thumb(
    frame: &RgbImage,
    thumb: &RgbImage,
    face: &RgbImage,
    cropper: &FaceCropper,
    panel_h: u32,
) -> (Vec<u32>, usize, usize) {
    let (_fw, fh) = frame.dimensions();
    let face_size = face.width();
    let thumb_w = thumb.width();

    let mut thumb = thumb.clone();

    // Draw green rectangle showing crop region (mapped to thumbnail coords)
    let scale = panel_h as f64 / fh as f64;
    let region = cropper.crop_region(frame);
    let rx = (region.x as f64 * scale) as u32;
    let ry = (region.y as f64 * scale) as u32;
    let rs = (region.size as f64 * scale) as u32;
    face::draw_rect(&mut thumb, rx, ry, rs.saturating_sub(1), rs.saturating_sub(1), Rgb([0, 255, 0]), 2);

    // Composite: [thumbnail | gap | face]
    let gap = 4u32;
    let total_w = thumb_w + gap + face_size;
    let total_h = panel_h;
    let mut composite = RgbImage::new(total_w, total_h);

    imageops::overlay(&mut composite, &thumb, 0, 0);

    let face_y = (total_h.saturating_sub(face_size)) / 2;
    imageops::overlay(&mut composite, face, (thumb_w + gap) as i64, face_y as i64);

    let buf = rgb_to_u32(&composite);
    (buf, total_w as usize, total_h as usize)
}

pub fn run_webcam(
    fps: u32,
    face_size: u32,
    width: u32,
    height: u32,
    output_dir: &str,
) -> anyhow::Result<PipelineStats> {
    fs::create_dir_all(output_dir)?;

    // Fixed preview height — keeps window a consistent size regardless of input
    let panel_h = 256u32;
    let scale_factor = panel_h as f64 / height as f64;
    let thumb_w = (width as f64 * scale_factor) as u32;
    let win_w = (thumb_w + 4 + face_size) as usize;
    let win_h = panel_h as usize;

    // Scale::X2 for small inputs, X1 for anything that would exceed ~1200px wide
    let scale = if win_w * 2 > 1200 {
        minifb::Scale::X1
    } else {
        minifb::Scale::X2
    };

    let mut window = Window::new(
        "brivva-frames (ESC to quit)",
        win_w,
        win_h,
        WindowOptions {
            scale,
            resize: false,
            ..WindowOptions::default()
        },
    )?;

    // Cap display rate — gives macOS compositor time to actually paint the window
    window.set_target_fps(60);

    println!("Starting webcam capture ({width}x{height}) at {fps} fps");
    println!("Live preview: left = full frame + crop rect, right = cropped face");
    println!("Press ESC to stop.\n");

    // Channel sends (full_frame, thumbnail) — thumbnail pre-resized in reader thread
    let (tx, rx) = mpsc::sync_channel::<(RgbImage, RgbImage)>(2);

    let reader_handle = thread::spawn(move || -> anyhow::Result<()> {
        let mut reader = FrameReader::from_webcam(fps, width, height)?;
        loop {
            match reader.next_frame()? {
                Some(frame) => {
                    // Pre-resize thumbnail in reader thread to avoid blocking main thread
                    let thumb = imageops::resize(
                        &frame,
                        thumb_w,
                        panel_h,
                        imageops::FilterType::Nearest,
                    );
                    if tx.send((frame, thumb)).is_err() {
                        break;
                    }
                }
                None => break,
            }
        }
        Ok(())
    });

    let cropper = FaceCropper::new(face_size);
    let mut stats = Stats::new();
    let mut frame_num = 0u32;
    let mut last_report = Instant::now();
    let mut frames_since_report = 0u32;
    let mut live_fps = 0.0f64;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // Drain to latest frame
        let (frame, thumb) = match rx.try_recv() {
            Ok(mut f) => {
                while let Ok(newer) = rx.try_recv() {
                    f = newer;
                }
                f
            }
            Err(mpsc::TryRecvError::Empty) => {
                window.update();
                continue;
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
        };

        let start = Instant::now();
        let face = cropper.crop_center(&frame);
        let (buf, bw, bh) = compose_with_thumb(&frame, &thumb, &face, &cropper, panel_h);
        stats.record(start.elapsed());

        window.update_with_buffer(&buf, bw, bh)?;

        frame_num += 1;
        frames_since_report += 1;

        if last_report.elapsed().as_secs_f64() >= 1.0 {
            live_fps = frames_since_report as f64 / last_report.elapsed().as_secs_f64();
            window.set_title(&format!(
                "brivva-frames | FPS: {:.1} | Crop: {:.1}ms | Frames: {}",
                live_fps,
                stats.last_latency_ms(),
                frame_num,
            ));
            frames_since_report = 0;
            last_report = Instant::now();
        }
    }

    drop(rx);
    if let Err(e) = reader_handle.join().expect("reader thread panicked") {
        eprintln!("reader thread error: {e}");
    }

    println!("\nStopped. Processed {frame_num} frames.");
    Ok(stats.finalize())
}

/// Benchmark: process as fast as possible, no saving, no display
pub fn run_benchmark(source: &str) -> anyhow::Result<PipelineStats> {
    let mut reader = FrameReader::from_file(source, 0)?;
    let cropper = FaceCropper::new(256);
    let mut stats = Stats::new();

    println!(
        "Benchmarking {} ({}x{}) — no saving, max speed",
        source,
        reader.width(),
        reader.height()
    );

    while let Some(frame) = reader.next_frame()? {
        let start = Instant::now();
        let _face = cropper.crop_center(&frame);
        stats.record(start.elapsed());
    }

    Ok(stats.finalize())
}
