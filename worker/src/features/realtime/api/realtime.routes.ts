import { Hono } from "hono";
import { Buffer } from "node:buffer";
import type { Bindings, Variables } from "../../../core/types";

const realtimeApp = new Hono<{ Bindings: Bindings; Variables: Variables }>();

function combineChunks(chunks: Uint8Array[]): Uint8Array {
  const total = chunks.reduce((sum, c) => sum + c.byteLength, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const c of chunks) {
    out.set(c, offset);
    offset += c.byteLength;
  }
  return out;
}

type Nova3Result = {
  results: { channels: [{ alternatives: [{ transcript: string }] }] };
};

async function processUtterance(
  env: Bindings,
  ws: WebSocket,
  audio: Uint8Array,
  sourceLang: string
) {
  try {
    const base64 = Buffer.from(audio).toString("base64");
    // Nova-3 needs a ReadableStream; create one per call (streams are single-use)
    const novaStream = new Blob([audio], { type: "audio/webm;codecs=opus" }).stream();

    const [novaResult, whisperResult] = await Promise.all([
      // Nova-3: accurate transcription in source language
      (env.AI.run as (m: string, i: object) => Promise<Nova3Result>)(
        "@cf/deepgram/nova-3",
        {
          audio: { body: novaStream, contentType: "audio/webm;codecs=opus" },
          language: sourceLang,
          punctuate: true,
          smart_format: true,
        }
      ),
      // Whisper: translate audio → English
      env.AI.run("@cf/openai/whisper-large-v3-turbo", {
        audio: base64,
        task: "translate",
        language: sourceLang,
        vad_filter: true,
      }),
    ]);

    const transcription =
      novaResult.results?.channels?.[0]?.alternatives?.[0]?.transcript?.trim() ?? "";
    const translation = whisperResult.text?.trim() ?? "";

    if (transcription || translation) {
      ws.send(JSON.stringify({ type: "result", transcription, translation }));
    }
  } catch (err) {
    console.error("processUtterance error:", err);
    ws.send(JSON.stringify({ type: "error", message: "Processing failed" }));
  }
}

realtimeApp.get("/realtime", (c) => {
  if (c.req.header("Upgrade") !== "websocket") {
    return c.text("WebSocket upgrade required", 426);
  }

  const { 0: client, 1: server } = new WebSocketPair();
  server.accept();

  const env = c.env;
  let sourceLang = "ko";
  // First chunk from MediaRecorder is the WebM init segment (codec info).
  // It must prefix every utterance for valid audio decoding.
  let initChunk: Uint8Array | null = null;
  let mediaChunks: Uint8Array[] = [];

  server.addEventListener("message", (event) => {
    const { data } = event;

    if (data instanceof ArrayBuffer) {
      const chunk = new Uint8Array(data);
      if (!initChunk) {
        initChunk = chunk; // Save init segment; reuse for every utterance
      } else {
        mediaChunks.push(chunk);
      }
      return;
    }

    if (typeof data !== "string") return;

    let msg: { type: string; sourceLang?: string };
    try {
      msg = JSON.parse(data) as { type: string; sourceLang?: string };
    } catch {
      return;
    }

    if (msg.type === "config" && msg.sourceLang) {
      sourceLang = msg.sourceLang;
    }

    if (msg.type === "process" && initChunk && mediaChunks.length > 0) {
      const utterance = combineChunks([initChunk, ...mediaChunks]);
      mediaChunks = []; // Reset media chunks; keep initChunk for next utterance
      processUtterance(env, server, utterance, sourceLang).catch(console.error);
    }
  });

  return new Response(null, { status: 101, webSocket: client } as ResponseInit);
});

export default realtimeApp;
