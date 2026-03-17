mod face;
mod ffmpeg;
mod pipeline;
mod stats;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "brivva-frames")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract frames from a video file, crop faces, save to output dir
    Extract {
        input: String,
        #[arg(long, default_value_t = 25)]
        fps: u32,
        #[arg(long, default_value_t = 256)]
        face_size: u32,
        #[arg(long, default_value = "output")]
        output: String,
    },
    /// Process a video as fast as possible, no saving (measure throughput)
    Benchmark {
        input: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Extract {
            input,
            fps,
            face_size,
            output,
        } => {
            let stats = pipeline::run_extract(&input, fps, face_size, &output)?;
            println!("\n{stats}");
        }
        Commands::Benchmark { input } => {
            let stats = pipeline::run_benchmark(&input)?;
            println!("\n{stats}");
        }
    }

    Ok(())
}
