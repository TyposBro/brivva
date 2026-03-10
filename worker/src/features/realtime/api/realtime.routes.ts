import { Hono } from "hono";
import type { Bindings, Variables } from "../../../core/types";

const realtimeApp = new Hono<{ Bindings: Bindings; Variables: Variables }>();

type NovaMessage = {
  type: string;
  channel?: { alternatives: [{ transcript: string }] };
  is_final?: boolean;
  speech_final?: boolean;
};

type LlmResult = { response: string };

// Each speech_final utterance gets a stable ID so out-of-order
// translations (when user speaks fast) update the right card.
async function handleNovaMessage(
  data: string | ArrayBuffer,
  clientWs: WebSocket,
  env: Bindings
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
  if (!transcript) return;

  if (!msg.speech_final) {
    // Interim: stream live words to the client immediately
    clientWs.send(JSON.stringify({ type: "interim", transcript }));
    return;
  }

  // speech_final: complete utterance — assign ID so translation can find it
  const utteranceId = Date.now();
  clientWs.send(JSON.stringify({ type: "final", transcript, utteranceId }));

  // Translate English → Spanish
  const llmResult = await (
    env.AI.run as (m: string, i: object) => Promise<LlmResult>
  )("@cf/meta/llama-3.2-1b-instruct", {
    messages: [
      {
        role: "system",
        content:
          "Translate English to Spanish. Output only the Spanish translation, nothing else.",
      },
      { role: "user", content: transcript },
    ],
    max_tokens: 512,
  });

  const translation = llmResult.response?.trim() ?? "";
  if (!translation) return;

  clientWs.send(JSON.stringify({ type: "translation", text: translation, utteranceId }));

  // TTS — stream aura-2-es audio back through the same WebSocket
  const ttsStream = await (
    env.AI.run as (m: string, i: object) => Promise<ReadableStream<Uint8Array>>
  )("@cf/deepgram/aura-2-es", { text: translation, speaker: "aquila" });

  clientWs.send(JSON.stringify({ type: "tts_start", utteranceId }));
  const reader = ttsStream.getReader();
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    clientWs.send(value);
  }
  clientWs.send(JSON.stringify({ type: "tts_end", utteranceId }));
}

realtimeApp.get("/realtime", async (c) => {
  if (c.req.header("Upgrade") !== "websocket") {
    return c.text("WebSocket upgrade required", 426);
  }

  const { 0: client, 1: server } = new WebSocketPair();
  server.accept();

  const env = c.env;

  // Connect to Nova-3 via Cloudflare AI Gateway WebSocket
  // PCM linear16 @ 16kHz — matches what the frontend sends
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

  // Nova-3 → Client
  novaWs.addEventListener("message", (event) => {
    handleNovaMessage(event.data, server, env).catch(console.error);
  });
  novaWs.addEventListener("close", () => server.close());
  novaWs.addEventListener("error", () => server.close());

  // Client → Nova-3: forward raw PCM audio
  server.addEventListener("message", (event) => {
    if (event.data instanceof ArrayBuffer && novaWs.readyState === WebSocket.OPEN) {
      novaWs.send(event.data);
    }
  });
  server.addEventListener("close", () => novaWs.close());

  return new Response(null, { status: 101, webSocket: client } as ResponseInit);
});

export default realtimeApp;
