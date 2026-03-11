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
};

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

  // ── Main pipeline: M2M100 translation ────────────────────────────────────
  const t0 = Date.now();
  const m2mResult = await (
    env.AI.run as (m: string, i: object) => Promise<M2MResult>
  )("@cf/meta/m2m100-1.2b", {
    text: finalTranscript,
    source_lang: "en",
    target_lang: "ja",
  });
  const translateMs = Date.now() - t0;

  const translation = m2mResult.translated_text?.trim() ?? "";
  if (!translation) return;

  clientWs.send(JSON.stringify({ type: "translation", text: translation, utteranceId, translateMs }));

  // ── Main pipeline: Kokoro TTS (self-hosted via Kokoro-FastAPI) ───────────
  const t1 = Date.now();

  const kokoroResp = await fetch(`${env.KOKORO_URL}/v1/audio/speech`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ model: "kokoro", voice: "jf_alpha", input: translation }),
  });

  if (!kokoroResp.ok || !kokoroResp.body) {
    const detail = await kokoroResp.text().catch(() => "");
    console.error("Kokoro TTS error:", kokoroResp.status, detail);
    clientWs.send(JSON.stringify({ type: "error", message: `Kokoro error ${kokoroResp.status}: ${detail}` }));
    return;
  }

  const audioReader = kokoroResp.body.getReader();

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

  const novaState: State = { pending: "" };

  // Nova-3 → Client
  novaWs.addEventListener("message", (event) => {
    handleNovaMessage(event.data, server, env, novaState).catch(console.error);
  });
  novaWs.addEventListener("close", () => server.close());
  novaWs.addEventListener("error", () => server.close());

  // Client → Nova-3: forward PCM
  server.addEventListener("message", (event) => {
    if (event.data instanceof ArrayBuffer && novaWs.readyState === WebSocket.OPEN) {
      novaWs.send(event.data);
    }
  });
  server.addEventListener("close", () => novaWs.close());

  return new Response(null, { status: 101, webSocket: client } as ResponseInit);
});

export default realtimeApp;
