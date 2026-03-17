use std::fs;
use std::time::Instant;

use minifb::{Key, Window, WindowOptions};

use crate::face::FaceCropper;
use crate::ffmpeg::FrameReader;
use crate::stats::{PipelineStats, Stats};

pub fn run_extract(
    source: &str,
    fps: u32,
    face_size: u32,
    output_dir: &str,
) -> anyhow::Result<PipelineStats> {
    fs::create_dir_all(output_dir)?;

    let mut reader = FrameReader::from_file(source, fps)?;
    let cropper = FaceCropper::new(face_size);
    let mut stats = Stats::new();
    let mut frame_num = 0u32;

    println!(
        "Extracting frames from {} ({}x{}) at {} fps → {}",
        source,
        reader.width(),
        reader.height(),
        fps,
        output_dir
    );

    while let Some(frame) = reader.next_frame()? {
        let start = Instant::now();
        let face = cropper.crop_center(&frame);
        face.save(format!("{output_dir}/frame_{frame_num:05}.png"))?;
        stats.record(start.elapsed());
        frame_num += 1;

        if frame_num % 100 == 0 {
            println!("  processed {frame_num} frames...");
        }
    }

    Ok(stats.finalize())
}

/// Convert RGB8 image to minifb u32 buffer (0x00RRGGBB)
fn rgb_to_u32(img: &image::RgbImage) -> Vec<u32> {
    img.pixels()
        .map(|p| ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32))
        .collect()
}

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
            scale: minifb::Scale::X2, // 2x so 256 shows as 512 on screen
            ..WindowOptions::default()
        },
    )?;

    // Cap update rate to match target fps
    window.set_target_fps(fps as usize);

    println!("Starting webcam capture ({width}x{height}) at {fps} fps");
    println!("Live preview window open. Press ESC to stop.\n");

    let mut reader = FrameReader::from_webcam(fps, width, height)?;
    let cropper = FaceCropper::new(face_size);
    let mut stats = Stats::new();
    let mut frame_num = 0u32;
    let mut last_report = Instant::now();
    let mut frames_since_report = 0u32;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let frame = match reader.next_frame()? {
            Some(f) => f,
            None => break,
        };

        let start = Instant::now();
        let face = cropper.crop_center(&frame);
        let buf = rgb_to_u32(&face);
        stats.record(start.elapsed());

        window.update_with_buffer(&buf, size, size)?;

        frame_num += 1;
        frames_since_report += 1;

        // Print live stats every second
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

    println!("\n\nStopped. Processed {frame_num} frames.");
    Ok(stats.finalize())
}

pub fn run_benchmark(source: &str) -> anyhow::Result<PipelineStats> {
    let mut reader = FrameReader::from_file(source, 0)?; // 0 = native fps
    let cropper = FaceCropper::new(256);
    let mut stats = Stats::new();
    let mut frame_num = 0u32;

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
        frame_num += 1;
    }

    Ok(stats.finalize())
}
