use std::time::{Duration, Instant};

pub struct PipelineStats {
    pub total_frames: u32,
    pub total_time: Duration,
    pub avg_fps: f64,
    pub avg_latency: Duration,
    pub min_latency: Duration,
    pub max_latency: Duration,
}

impl std::fmt::Display for PipelineStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "--- Pipeline Stats ---")?;
        writeln!(f, "Total frames:  {}", self.total_frames)?;
        writeln!(f, "Total time:    {:.2}s", self.total_time.as_secs_f64())?;
        writeln!(f, "Avg FPS:       {:.1}", self.avg_fps)?;
        writeln!(f, "Avg latency:   {:.2}ms", self.avg_latency.as_secs_f64() * 1000.0)?;
        writeln!(f, "Min latency:   {:.2}ms", self.min_latency.as_secs_f64() * 1000.0)?;
        write!(f, "Max latency:   {:.2}ms", self.max_latency.as_secs_f64() * 1000.0)
    }
}

pub struct Stats {
    frame_times: Vec<Duration>,
    start: Instant,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            frame_times: Vec::new(),
            start: Instant::now(),
        }
    }

    pub fn record(&mut self, elapsed: Duration) {
        self.frame_times.push(elapsed);
    }

    pub fn last_latency_ms(&self) -> f64 {
        self.frame_times
            .last()
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }

    pub fn finalize(self) -> PipelineStats {
        let total_frames = self.frame_times.len() as u32;
        let total_time = self.start.elapsed();

        if total_frames == 0 {
            return PipelineStats {
                total_frames: 0,
                total_time,
                avg_fps: 0.0,
                avg_latency: Duration::ZERO,
                min_latency: Duration::ZERO,
                max_latency: Duration::ZERO,
            };
        }

        let sum: Duration = self.frame_times.iter().sum();
        let avg_latency = sum / total_frames;
        let min_latency = *self.frame_times.iter().min().unwrap();
        let max_latency = *self.frame_times.iter().max().unwrap();
        let avg_fps = total_frames as f64 / total_time.as_secs_f64();

        PipelineStats {
            total_frames,
            total_time,
            avg_fps,
            avg_latency,
            min_latency,
            max_latency,
        }
    }
}
