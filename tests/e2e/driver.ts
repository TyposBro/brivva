// E2E smoke driver. Walks the full media pipeline against a
// docker-compose stack (server-rs + mediamtx + workers/soniox/elevenlabs stubs).
//
// Flow:
//   1. wait for server-rs /health
//   2. mint HS256 JWT for smoke-user
//   3. open WS to /api/session?session_id=SMOKE001
//   4. for ~40s: push 20ms PCM sine frames + a JPEG every 2s
//   5. close WS
//   6. assert WS emitted translation + tts_end at least once
//   7. ffprobe translated RTMP output → assert audio+video tracks
//
// Fails hard on any missing track. Intended for nightly CI + post-deploy.

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";

const SERVER_URL = process.env.SERVER_URL ?? "http://localhost:3000";
const WS_URL = process.env.WS_URL ?? "ws://localhost:3000/api/session";
const RTMP_URL = process.env.RTMP_URL ?? "rtmp://localhost:1935/live/smoke-ja";
const JWT_SECRET = process.env.JWT_SECRET ?? "smoke-jwt-secret";
const SESSION_ID = process.env.SESSION_ID ?? "SMOKE001";
const USER_ID = process.env.USER_ID ?? "smoke-user";
const STREAM_DURATION_MS = Number(process.env.STREAM_DURATION_MS ?? 40_000);
const SETTLE_MS = Number(process.env.SETTLE_MS ?? 5_000);

const SAMPLE_RATE = 44_100;
const FRAME_MS = 20;
const SAMPLES_PER_FRAME = (SAMPLE_RATE * FRAME_MS) / 1000;
const JPEG_EVERY_MS = 2_000;
const JPEG_PATH = "/tmp/smoke_frame.jpg";

// ── JWT (HS256) ──────────────────────────────────────────
function b64url(input: string | Uint8Array): string {
  const bytes = typeof input === "string" ? new TextEncoder().encode(input) : input;
  return Buffer.from(bytes)
    .toString("base64")
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

async function mintJwt(sub: string, secret: string): Promise<string> {
  const header = { alg: "HS256", typ: "JWT" };
  const payload = {
    sub,
    iss: "brivva-api",
    aud: "brivva-fargate",
    exp: Math.floor(Date.now() / 1000) + 900,
  };
  const signingInput = `${b64url(JSON.stringify(header))}.${b64url(JSON.stringify(payload))}`;
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sig = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(signingInput));
  return `${signingInput}.${b64url(new Uint8Array(sig))}`;
}

// ── Audio/video fixtures ────────────────────────────────
function generateSineFrame(freqHz: number, phase: number): { pcm: Buffer; nextPhase: number } {
  const buf = Buffer.alloc(SAMPLES_PER_FRAME * 2);
  const step = (2 * Math.PI * freqHz) / SAMPLE_RATE;
  let p = phase;
  for (let i = 0; i < SAMPLES_PER_FRAME; i++) {
    const sample = Math.round(Math.sin(p) * 12_000);
    buf.writeInt16LE(sample, i * 2);
    p += step;
  }
  return { pcm: buf, nextPhase: p % (2 * Math.PI) };
}

function ensureJpeg(): Buffer {
  if (existsSync(JPEG_PATH)) return readFileSync(JPEG_PATH);
  const result = spawnSync(
    "ffmpeg",
    [
      "-y",
      "-f", "lavfi",
      "-i", "color=c=blue:s=320x240:d=1",
      "-frames:v", "1",
      JPEG_PATH,
    ],
    { stdio: "inherit" },
  );
  if (result.status !== 0) {
    writeFileSync(JPEG_PATH, Buffer.alloc(0));
  }
  return readFileSync(JPEG_PATH);
}

// ── Health wait ──────────────────────────────────────────
async function waitForHealth(url: string, timeoutMs = 60_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const resp = await fetch(`${url}/health`);
      if (resp.ok) return;
    } catch {
      // ignore until deadline
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`server /health not ready after ${timeoutMs}ms`);
}

// ── WS stream ────────────────────────────────────────────
async function streamAudioVideo(wsUrl: string): Promise<{ sawTranslation: boolean; sawTtsEnd: boolean }> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl);
    ws.binaryType = "arraybuffer";

    let phase = 0;
    let sawTranslation = false;
    let sawTtsEnd = false;
    let audioTimer: ReturnType<typeof setInterval> | null = null;
    let jpegTimer: ReturnType<typeof setInterval> | null = null;
    let stopTimer: ReturnType<typeof setTimeout> | null = null;

    const cleanup = () => {
      if (audioTimer) clearInterval(audioTimer);
      if (jpegTimer) clearInterval(jpegTimer);
      if (stopTimer) clearTimeout(stopTimer);
    };

    ws.addEventListener("open", () => {
      console.log("[driver] WS open, streaming for", STREAM_DURATION_MS, "ms");
      const jpeg = ensureJpeg();

      audioTimer = setInterval(() => {
        if (ws.readyState !== WebSocket.OPEN) return;
        const { pcm, nextPhase } = generateSineFrame(440, phase);
        phase = nextPhase;
        ws.send(pcm);
      }, FRAME_MS);

      jpegTimer = setInterval(() => {
        if (ws.readyState !== WebSocket.OPEN) return;
        ws.send(JSON.stringify({
          type: "face:frame",
          data: jpeg.toString("base64"),
        }));
      }, JPEG_EVERY_MS);

      stopTimer = setTimeout(() => {
        console.log("[driver] stream window elapsed, sending host:end");
        try {
          ws.send(JSON.stringify({ type: "host:end" }));
        } catch { /* ignore */ }
        setTimeout(() => ws.close(), 500);
      }, STREAM_DURATION_MS);
    });

    ws.addEventListener("message", (event) => {
      if (typeof event.data !== "string") return;
      try {
        const msg = JSON.parse(event.data) as { type?: string; targetLang?: string };
        if (msg.type === "translation" && msg.targetLang === "ja") sawTranslation = true;
        if (msg.type === "tts_end" && msg.targetLang === "ja") sawTtsEnd = true;
      } catch {
        // ignore non-json frames
      }
    });

    ws.addEventListener("close", () => {
      cleanup();
      resolve({ sawTranslation, sawTtsEnd });
    });

    ws.addEventListener("error", (err) => {
      cleanup();
      reject(err);
    });
  });
}

// ── ffprobe assertion ────────────────────────────────────
type ProbeStream = { codec_type: string; codec_name?: string };

function probeRtmp(url: string): ProbeStream[] {
  const result = spawnSync(
    "ffprobe",
    [
      "-v", "error",
      "-rw_timeout", "10000000",
      "-print_format", "json",
      "-show_streams",
      url,
    ],
    { encoding: "utf-8" },
  );
  if (result.status !== 0) {
    throw new Error(`ffprobe failed: ${result.stderr}`);
  }
  const parsed = JSON.parse(result.stdout) as { streams?: ProbeStream[] };
  return parsed.streams ?? [];
}

// ── Entry ────────────────────────────────────────────────
async function main() {
  console.log("[driver] wait for server /health …");
  await waitForHealth(SERVER_URL);

  const token = await mintJwt(USER_ID, JWT_SECRET);
  const wsUrl = `${WS_URL}?token=${encodeURIComponent(token)}&source_lang=en&session_id=${SESSION_ID}`;

  console.log("[driver] connecting", wsUrl.slice(0, wsUrl.indexOf("?") + 1) + "…");
  const wsSignals = await streamAudioVideo(wsUrl);
  if (!wsSignals.sawTranslation) {
    throw new Error("smoke FAIL — no translation event observed for ja");
  }
  if (!wsSignals.sawTtsEnd) {
    throw new Error("smoke FAIL — no tts_end event observed for ja");
  }

  console.log(`[driver] WS closed. Waiting ${SETTLE_MS}ms for RTMP to settle …`);
  await new Promise((r) => setTimeout(r, SETTLE_MS));

  console.log("[driver] probing RTMP:", RTMP_URL);
  const streams = probeRtmp(RTMP_URL);
  const audio = streams.find((s) => s.codec_type === "audio");
  const video = streams.find((s) => s.codec_type === "video");

  console.log("[driver] streams:", streams);

  const missing: string[] = [];
  if (!audio) missing.push("audio");
  if (!video) missing.push("video");
  if (missing.length) {
    throw new Error(`smoke FAIL — missing tracks: ${missing.join(", ")}`);
  }

  console.log(`[driver] smoke OK — audio=${audio?.codec_name} video=${video?.codec_name}`);
}

main().catch((err) => {
  console.error("[driver] ERROR", err);
  process.exit(1);
});
