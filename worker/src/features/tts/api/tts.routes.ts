import { Hono } from "hono";
import type { Bindings, Variables } from "../../../core/types";

const ttsApp = new Hono<{ Bindings: Bindings; Variables: Variables }>();

ttsApp.post("/tts", async (c) => {
  const { text } = await c.req.json<{ text: string }>();

  if (!text?.trim()) {
    return c.json({ message: "text is required" }, 400);
  }

  const stream = await c.env.AI.run("@cf/deepgram/aura-2-es", {
    text,
    speaker: "aquila",
  });

  return new Response(stream as unknown as ReadableStream, {
    headers: { "Content-Type": "audio/mpeg" },
  });
});

export default ttsApp;
