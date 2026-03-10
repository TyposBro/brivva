import { Hono } from "hono";
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

type LlmResult = { response: string };

async function processUtterance(env: Bindings, ws: WebSocket, audio: Uint8Array) {
  try {
    const novaStream = new Blob([audio], { type: "audio/webm;codecs=opus" }).stream();

    // 1. Nova-3: transcribe English speech → English text
    const novaResult = await (
      env.AI.run as (m: string, i: object) => Promise<Nova3Result>
    )("@cf/deepgram/nova-3", {
      audio: { body: novaStream, contentType: "audio/webm;codecs=opus" },
      language: "en",
      punctuate: true,
      smart_format: true,
    });

    const transcription =
      novaResult.results?.channels?.[0]?.alternatives?.[0]?.transcript?.trim() ?? "";

    if (!transcription) return;

    // 2. LLM: translate English text → Spanish
    const llmResult = await (
      env.AI.run as (m: string, i: object) => Promise<LlmResult>
    )("@cf/meta/llama-3.1-8b-instruct", {
      messages: [
        {
          role: "system",
          content:
            "You are a translator. Translate the English text to Spanish. Output only the Spanish translation, nothing else.",
        },
        { role: "user", content: transcription },
      ],
      max_tokens: 512,
    });

    const translation = llmResult.response?.trim() ?? "";

    ws.send(JSON.stringify({ type: "result", transcription, translation }));
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
  let initChunk: Uint8Array | null = null;
  let mediaChunks: Uint8Array[] = [];

  server.addEventListener("message", (event) => {
    const { data } = event;

    if (data instanceof ArrayBuffer) {
      const chunk = new Uint8Array(data);
      if (!initChunk) {
        initChunk = chunk;
      } else {
        mediaChunks.push(chunk);
      }
      return;
    }

    if (typeof data !== "string") return;

    let msg: { type: string };
    try {
      msg = JSON.parse(data) as { type: string };
    } catch {
      return;
    }

    if (msg.type === "process" && initChunk && mediaChunks.length > 0) {
      const utterance = combineChunks([initChunk, ...mediaChunks]);
      mediaChunks = [];
      processUtterance(env, server, utterance).catch(console.error);
    }
  });

  return new Response(null, { status: 101, webSocket: client } as ResponseInit);
});

export default realtimeApp;
