use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use minifb::{Key, Window, WindowOptions};
use rayon::prelude::*;

use crate::face::FaceCropper;
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
fn rgb_to_u32(img: &image::RgbImage) -> Vec<u32> {
    img.pixels()
        .map(|p| ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32))
        .collect()
}

/// Step 7: Channel-based streaming pipeline for webcam
/// Reader thread reads frames and sends through mpsc channel.
/// Main thread receives, crops, and displays — decoupling read from process.
pub fn run_webcam(
    fps: u32,
    face_size: u32,
    width: u32,
    height: u32,
    output_dir: &str,
) -> anyhow::Result<PipelineStats> {
    fs::create_dir_all(output_dir)?;

    let size = face_size as usize;
    let mut window = Window::new(
        "brivva-frames — face crop (ESC to quit)",
        size,
        size,
        WindowOptions {
            scale: minifb::Scale::X2,
            ..WindowOptions::default()
        },
    )?;
    window.set_target_fps(fps as usize);

    println!("Starting webcam capture ({width}x{height}) at {fps} fps");
    println!("Live preview window open. Press ESC to stop.\n");

    // Channel: reader thread → main thread
    let (tx, rx) = mpsc::sync_channel::<image::RgbImage>(4); // buffer 4 frames

    // Reader thread — owns the FrameReader, sends frames through channel
    let reader_handle = thread::spawn(move || -> anyhow::Result<()> {
        let mut reader = FrameReader::from_webcam(fps, width, height)?;
        loop {
            match reader.next_frame()? {
                Some(frame) => {
                    // If channel is full, this blocks until main thread consumes
                    // If receiver dropped (window closed), send fails and we exit
                    if tx.send(frame).is_err() {
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

    // Main thread — receives frames, crops, displays
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let frame = match rx.try_recv() {
            Ok(f) => f,
            Err(mpsc::TryRecvError::Empty) => {
                // No frame ready yet, just update window to keep it responsive
                window.update();
                continue;
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
        };

        let start = Instant::now();
        let face = cropper.crop_center(&frame);
        let buf = rgb_to_u32(&face);
        stats.record(start.elapsed());

        window.update_with_buffer(&buf, size, size)?;

        frame_num += 1;
        frames_since_report += 1;

        if last_report.elapsed().as_secs_f64() >= 1.0 {
            let live_fps = frames_since_report as f64 / last_report.elapsed().as_secs_f64();
            print!(
                "\r  frames: {} | live fps: {:.1} | crop: {:.2}ms    ",
                frame_num,
                live_fps,
                stats.last_latency_ms(),
            );
            frames_since_report = 0;
            last_report = Instant::now();
        }
    }

    // Drop rx to signal reader thread to stop
    drop(rx);
    if let Err(e) = reader_handle.join().expect("reader thread panicked") {
        eprintln!("reader thread error: {e}");
    }

    println!("\n\nStopped. Processed {frame_num} frames.");
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
