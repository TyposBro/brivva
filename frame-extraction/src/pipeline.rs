use std::fs;
use std::time::Instant;

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
