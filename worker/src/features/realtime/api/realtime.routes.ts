import { Hono } from "hono";
import type { Bindings, Variables } from "../../../core/types";

const realtimeApp = new Hono<{ Bindings: Bindings; Variables: Variables }>();

type NovaMessage = {
  type: string;
  channel?: { alternatives: [{ transcript: string }] };
  is_final?: boolean;
  speech_final?: boolean;
};

type M2MResult = { translated_text: string };

type State = {
  pending: string;
  audioChunks: ArrayBuffer[]; // PCM buffer for Whisper comparison
};

// Build a minimal WAV from raw Int16 PCM chunks (mono 16kHz)
function pcmChunksToWavArray(chunks: ArrayBuffer[]): number[] {
  if (chunks.length === 0) return [];
  const totalPcm = chunks.reduce((n, c) => n + c.byteLength, 0);
  const pcm = new Uint8Array(totalPcm);
  let off = 0;
  for (const c of chunks) { pcm.set(new Uint8Array(c), off); off += c.byteLength; }

  const wav = new Uint8Array(44 + totalPcm);
  const v = new DataView(wav.buffer);
  // RIFF header
  wav.set([82,73,70,70], 0); v.setUint32(4, 36 + totalPcm, true);
  wav.set([87,65,86,69], 8); wav.set([102,109,116,32], 12);
  v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 1, true);
  v.setUint32(24, 16000, true); v.setUint32(28, 32000, true);
  v.setUint16(32, 2, true); v.setUint16(34, 16, true);
  wav.set([100,97,116,97], 36); v.setUint32(40, totalPcm, true);
  wav.set(pcm, 44);
  return [...wav];
}

async function handleNovaMessage(
  data: string | ArrayBuffer,
  clientWs: WebSocket,
  env: Bindings,
  state: State
) {
  if (typeof data !== "string") return;

  let msg: NovaMessage;
  try {
    msg = JSON.parse(data) as NovaMessage;
  } catch {
    return;
  }

  if (msg.type !== "Results") return;

  const transcript = msg.channel?.alternatives?.[0]?.transcript?.trim() ?? "";

  if (!msg.is_final && !msg.speech_final) {
    if (!transcript) return;
    state.pending = transcript;
    clientWs.send(JSON.stringify({ type: "interim", transcript }));
    return;
  }

  const finalTranscript = transcript || state.pending;
  state.pending = "";
  if (!finalTranscript) return;

  const utteranceId = Date.now();
  clientWs.send(JSON.stringify({ type: "final", transcript: finalTranscript, utteranceId }));

  // Snapshot + reset audio buffer
  const capturedChunks = state.audioChunks;
  state.audioChunks = [];

  // ── Background: Whisper STT comparison ────────────────────────────────────
  // Runs concurrently — does NOT block the main translate+TTS pipeline.
  // Sends { type: "stt_compare" } when done.
  (async () => {
    const wavArr = pcmChunksToWavArray(capturedChunks);
    if (wavArr.length === 0) return;
    try {
      const t = Date.now();
      const result = await (
        env.AI.run as (m: string, i: object) => Promise<{ text: string }>
      )("@cf/openai/whisper-large-v3-turbo", { audio: wavArr });
      clientWs.send(JSON.stringify({
        type: "stt_compare",
        utteranceId,
        whisperMs: Date.now() - t,
        whisperTranscript: result.text?.trim() ?? "",
      }));
    } catch (err) {
      console.error("Whisper compare failed:", err);
    }
  })();

  // ── Main pipeline: M2M100 translation ────────────────────────────────────
  const t0 = Date.now();
  const m2mResult = await (
    env.AI.run as (m: string, i: object) => Promise<M2MResult>
  )("@cf/meta/m2m100-1.2b", {
    text: finalTranscript,
    source_lang: "en",
    target_lang: "fr",
  });
  const translateMs = Date.now() - t0;

  const translation = m2mResult.translated_text?.trim() ?? "";
  if (!translation) return;

  clientWs.send(JSON.stringify({ type: "translation", text: translation, utteranceId, translateMs }));

  // ── Background: CF TTS comparison (aura-2-fr / agathe) ───────────────────
  // Runs concurrently with Kokoro — does NOT block audio delivery.
  // Sends { type: "tts_compare" } when done.
  (async () => {
    try {
      const t = Date.now();
      const stream = await (
        env.AI.run as (m: string, i: object) => Promise<ReadableStream>
      )("@cf/deepgram/aura-2-fr", { text: translation, speaker: "agathe" });
      const reader = stream.getReader();
      while (true) {
        const { done } = await reader.read();
        if (done) break;
      }
      clientWs.send(JSON.stringify({
        type: "tts_compare",
        utteranceId,
        cfTtsMs: Date.now() - t,
      }));
    } catch (err) {
      console.error("CF TTS compare failed:", err);
    }
  })();

  // ── Main pipeline: Kokoro TTS via Replicate ───────────────────────────────
  const t1 = Date.now();

  type KokoroResult = { id?: string; status?: string; output?: string; error?: string };

  let kokoroResult: KokoroResult | null = null;
  for (let attempt = 0; attempt < 5; attempt++) {
    const resp = await fetch("https://api.replicate.com/v1/predictions", {
      method: "POST",
      headers: {
        "Authorization": `Bearer ${env.REPLICATE_API_TOKEN}`,
        "Content-Type": "application/json",
        "Prefer": "wait",
      },
      body: JSON.stringify({
        version: "f559560eb822dc509045f3921a1921234918b91739db4bf3daab2169b71c7a13",
        input: { text: translation, voice: "ff_siwis" },
      }),
    });
    if (resp.status === 429) {
      const retryData = await resp.json() as { retry_after?: number };
      const waitMs = ((retryData.retry_after ?? 10) + 1) * 1000;
      await new Promise((r) => setTimeout(r, waitMs));
      continue;
    }
    kokoroResult = await resp.json() as KokoroResult;
    break;
  }

  if (!kokoroResult) {
    clientWs.send(JSON.stringify({ type: "error", message: "Kokoro rate limited after retries" }));
    return;
  }

  let audioUrl = kokoroResult.output;

  if (!audioUrl && kokoroResult.id && kokoroResult.status !== "failed") {
    for (let i = 0; i < 60; i++) {
      await new Promise((r) => setTimeout(r, 1000));
      const poll = await fetch(`https://api.replicate.com/v1/predictions/${kokoroResult.id}`, {
        headers: { "Authorization": `Bearer ${env.REPLICATE_API_TOKEN}` },
      });
      const pollResult = await poll.json() as KokoroResult;
      if (pollResult.status === "succeeded" && pollResult.output) {
        audioUrl = pollResult.output;
        break;
      }
      if (pollResult.status === "failed") {
        console.error("Kokoro prediction failed:", pollResult.error);
        clientWs.send(JSON.stringify({ type: "error", message: `Kokoro failed: ${pollResult.error}` }));
        return;
      }
    }
  }

  if (!audioUrl) {
    const detail = JSON.stringify(kokoroResult);
    console.error("Kokoro TTS no output:", detail);
    clientWs.send(JSON.stringify({ type: "error", message: `Kokoro no output: ${detail}` }));
    return;
  }

  const audioResp = await fetch(audioUrl);
  const audioReader = audioResp.body!.getReader();

  clientWs.send(JSON.stringify({ type: "tts_start", utteranceId }));
  while (true) {
    const { done, value } = await audioReader.read();
    if (done) break;
    clientWs.send(value);
  }
  const ttsMs = Date.now() - t1;
  clientWs.send(JSON.stringify({ type: "tts_end", utteranceId, ttsMs }));
}

realtimeApp.get("/realtime", async (c) => {
  if (c.req.header("Upgrade") !== "websocket") {
    return c.text("WebSocket upgrade required", 426);
  }

  const { 0: client, 1: server } = new WebSocketPair();
  server.accept();

  const env = c.env;

  const url = new URL(
    `https://gateway.ai.cloudflare.com/v1/${env.CF_ACCOUNT_ID}/${env.CF_AI_GATEWAY_ID}/workers-ai`
  );
  url.searchParams.set("model", "@cf/deepgram/nova-3");
  url.searchParams.set("encoding", "linear16");
  url.searchParams.set("sample_rate", "16000");
  url.searchParams.set("channels", "1");
  url.searchParams.set("language", "en");
  url.searchParams.set("interim_results", "true");
  url.searchParams.set("punctuate", "true");
  url.searchParams.set("smart_format", "true");
  url.searchParams.set("endpointing", "300");
  url.searchParams.set("utterance_end_ms", "1000");

  let novaWs: WebSocket;
  try {
    const novaResponse = await fetch(url.toString(), {
      headers: {
        Upgrade: "websocket",
        "cf-aig-authorization": `Bearer ${env.CF_API_TOKEN}`,
      },
    });

    if (novaResponse.status !== 101) {
      const body = await novaResponse.text().catch(() => "");
      throw new Error(`Nova-3 WS ${novaResponse.status}: ${body}`);
    }

    novaWs = novaResponse.webSocket!;
    novaWs.accept();
  } catch (err) {
    console.error("Nova-3 WS connection failed:", err);
    server.send(JSON.stringify({ type: "error", message: String(err) }));
    server.close();
    return new Response(null, { status: 101, webSocket: client } as ResponseInit);
  }

  const novaState: State = { pending: "", audioChunks: [] };

  // Nova-3 → Client
  novaWs.addEventListener("message", (event) => {
    handleNovaMessage(event.data, server, env, novaState).catch(console.error);
  });
  novaWs.addEventListener("close", () => server.close());
  novaWs.addEventListener("error", () => server.close());

  // Client → Nova-3: forward PCM + buffer for Whisper comparison
  server.addEventListener("message", (event) => {
    if (event.data instanceof ArrayBuffer && novaWs.readyState === WebSocket.OPEN) {
      novaWs.send(event.data);
      if (novaState.audioChunks.length < 50) { // cap ~10s
        novaState.audioChunks.push(event.data);
      }
    }
  });
  server.addEventListener("close", () => novaWs.close());

  return new Response(null, { status: 101, webSocket: client } as ResponseInit);
});

export default realtimeApp;
