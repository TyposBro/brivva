// Stub for ElevenLabs TTS. server-rs POSTs to:
//   /v1/text-to-speech/:voice_id/stream?output_format=mp3_44100_128
// with JSON { text, model_id }. We return a fixed-duration silent MP3
// generated at startup via ffmpeg. server-rs decodes via its own
// decode_mp3_to_pcm → pushes into RTMP mixer.

import { spawnSync } from "node:child_process";
import { readFileSync, existsSync, writeFileSync } from "node:fs";

const PORT = Number(process.env.PORT ?? 9002);
const MP3_PATH = "/tmp/smoke_tts.mp3";
const DURATION_S = Number(process.env.TTS_DURATION_S ?? 2);

function ensureMp3(): Buffer {
  if (existsSync(MP3_PATH)) return readFileSync(MP3_PATH);
  const result = spawnSync(
    "ffmpeg",
    [
      "-y",
      "-f", "lavfi",
      "-i", `sine=frequency=440:duration=${DURATION_S}`,
      "-c:a", "libmp3lame",
      "-b:a", "128k",
      "-ar", "44100",
      MP3_PATH,
    ],
    { stdio: "inherit" },
  );
  if (result.status !== 0) {
    // Fallback: write empty buffer so we at least return something.
    writeFileSync(MP3_PATH, Buffer.alloc(0));
  }
  return readFileSync(MP3_PATH);
}

const mp3 = ensureMp3();
console.log(`[elevenlabs-stub] prepared ${mp3.length} bytes of MP3`);

Bun.serve({
  port: PORT,
  async fetch(req) {
    const url = new URL(req.url);
    if (url.pathname === "/health") return new Response("ok");
    if (
      req.method === "POST" &&
      url.pathname.startsWith("/v1/text-to-speech/") &&
      url.pathname.endsWith("/stream")
    ) {
      return new Response(mp3, {
        headers: { "Content-Type": "audio/mpeg" },
      });
    }
    return new Response("not found", { status: 404 });
  },
});

console.log(`[elevenlabs-stub] listening on :${PORT}`);
