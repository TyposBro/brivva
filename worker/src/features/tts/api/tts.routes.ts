import { Hono } from "hono";
import type { Bindings, Variables } from "../../../core/types";

// Cloudflare Workers AI Deepgram Aura-2 models by language
// Languages without a model fall back to English
const TTS_MODEL: Record<string, { model: string; speaker: string }> = {
  en: { model: "@cf/deepgram/aura-2-en", speaker: "luna" },
  es: { model: "@cf/deepgram/aura-2-es", speaker: "aquila" },
  fr: { model: "@cf/deepgram/aura-2-fr", speaker: "agathe" },
  de: { model: "@cf/deepgram/aura-2-de", speaker: "elara" },
  it: { model: "@cf/deepgram/aura-2-it", speaker: "melia" },
  ja: { model: "@cf/deepgram/aura-2-ja", speaker: "uzume" },
  nl: { model: "@cf/deepgram/aura-2-nl", speaker: "beatrix" },
};

const FALLBACK = TTS_MODEL["en"];

const ttsApp = new Hono<{ Bindings: Bindings; Variables: Variables }>();

ttsApp.post("/tts", async (c) => {
  const { text, lang } = await c.req.json<{ text: string; lang?: string }>();

  if (!text?.trim()) {
    return c.json({ message: "text is required" }, 400);
  }

  const { model, speaker } = TTS_MODEL[lang ?? ""] ?? FALLBACK;

  // AI.run is typed per-model — cast for dynamic model selection
  const stream = await (c.env.AI.run as (m: string, i: object) => Promise<ReadableStream>)(
    model,
    { text, speaker }
  );

  return new Response(stream, {
    headers: { "Content-Type": "audio/mpeg" },
  });
});

export default ttsApp;
