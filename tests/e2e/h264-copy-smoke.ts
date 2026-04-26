import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  assertCleanMediaLogs,
  assertRtmpStreams,
  type ProbeStream,
} from "./media-assertions";

const RTMP_URL = process.env.RTMP_URL ?? "rtmp://localhost:1935/live/h264-copy-smoke";
const DURATION_MS = Number(process.env.STREAM_DURATION_MS ?? 25_000);
const PROBE_AT_MS = Number(process.env.PROBE_AT_MS ?? 8_000);
const PROBE_TIMEOUT_MS = Number(process.env.PROBE_TIMEOUT_MS ?? 20_000);

function run(cmd: string, args: string[], opts: { cwd?: string } = {}) {
  const result = spawnSync(cmd, args, { encoding: "utf-8", ...opts });
  if (result.status !== 0) {
    throw new Error(`${cmd} failed\nstdout=${result.stdout}\nstderr=${result.stderr}`);
  }
  return result;
}

function createH264Fixture(dir: string): string {
  const path = join(dir, "input.h264");
  run("ffmpeg", [
    "-y",
    "-hide_banner",
    "-loglevel",
    "error",
    "-f",
    "lavfi",
    "-i",
    "testsrc2=size=1280x720:rate=30",
    "-t",
    "8",
    "-c:v",
    "libx264",
    "-preset",
    "ultrafast",
    "-tune",
    "zerolatency",
    "-g",
    "30",
    "-pix_fmt",
    "yuv420p",
    "-bsf:v",
    "h264_mp4toannexb",
    "-f",
    "h264",
    path,
  ]);
  return path;
}

function probeRtmp(url: string): ProbeStream[] {
  const result = spawnSync("ffprobe", [
    "-v",
    "error",
    "-rw_timeout",
    "10000000",
    "-print_format",
    "json",
    "-show_streams",
    url,
  ], { encoding: "utf-8", timeout: 15_000 });
  if (result.status !== 0) {
    throw new Error(`ffprobe failed: ${result.stderr}`);
  }
  return (JSON.parse(result.stdout) as { streams?: ProbeStream[] }).streams ?? [];
}

async function probeRtmpEventually(url: string): Promise<ProbeStream[]> {
  const deadline = Date.now() + PROBE_TIMEOUT_MS;
  let lastError: unknown = null;
  while (Date.now() < deadline) {
    try {
      return probeRtmp(url);
    } catch (err) {
      lastError = err;
      await new Promise((resolve) => setTimeout(resolve, 1000));
    }
  }
  throw lastError instanceof Error ? lastError : new Error(String(lastError));
}

async function main() {
  const dir = mkdtempSync(join(tmpdir(), "brivva-h264-copy-"));
  let ffmpeg: ReturnType<typeof spawn> | null = null;
  try {
    const input = createH264Fixture(dir);
    if (!existsSync(input) || readFileSync(input).length === 0) {
      throw new Error("empty h264 fixture");
    }

    ffmpeg = spawn("ffmpeg", [
      "-hide_banner",
      "-loglevel",
      "warning",
      "-re",
      "-stream_loop",
      "-1",
      "-fflags",
      "+genpts+nobuffer",
      "-flags",
      "low_delay",
      "-thread_queue_size",
      "512",
      "-use_wallclock_as_timestamps",
      "1",
      "-r",
      "30",
      "-f",
      "h264",
      "-i",
      input,
      "-f",
      "lavfi",
      "-i",
      "anullsrc=channel_layout=mono:sample_rate=44100",
      "-c:v",
      "libx264",
      "-preset",
      "ultrafast",
      "-tune",
      "zerolatency",
      "-b:v",
      "2500k",
      "-maxrate",
      "2500k",
      "-bufsize",
      "5000k",
      "-pix_fmt",
      "yuv420p",
      "-g",
      "30",
      "-c:a",
      "aac",
      "-ac:a",
      "2",
      "-b:a",
      "128k",
      "-map",
      "0:v",
      "-map",
      "1:a",
      "-f",
      "flv",
      RTMP_URL,
    ], { stdio: ["ignore", "pipe", "pipe"] });

    let stderr = "";
    ffmpeg.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });

    await new Promise((resolve) => setTimeout(resolve, PROBE_AT_MS));
    const streams = await probeRtmpEventually(RTMP_URL);
    const video = streams.find((stream) => stream.codec_type === "video");
    const audio = streams.find((stream) => stream.codec_type === "audio");

    assertRtmpStreams(streams, "h264 copy smoke");
    assertCleanMediaLogs(stderr, "h264 copy ffmpeg");
    console.log(`[h264-rtmp] OK audio=${audio.codec_name} video=${video.codec_name}`);
  } finally {
    if (ffmpeg) {
      ffmpeg.kill("SIGTERM");
      await new Promise((resolve) => setTimeout(resolve, 500));
      if (!ffmpeg.killed) ffmpeg.kill("SIGKILL");
    }
    rmSync(dir, { recursive: true, force: true });
  }
}

main().catch((err) => {
  console.error("[h264-rtmp] ERROR", err);
  process.exit(1);
});
