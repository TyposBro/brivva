import { Hono } from "hono";
import { TranslationService } from "../core/translation.service";
import type { Bindings, Variables } from "../../../core/types";

const translationApp = new Hono<{ Bindings: Bindings; Variables: Variables }>();

translationApp.post("/translate", async (c) => {
  const formData = await c.req.formData();

  const audio = formData.get("audio") as File | null;
  if (!audio) {
    return c.json({ message: "audio field is required" }, 400);
  }

  const sourceLang = (formData.get("sourceLang") as string | null) ?? "ko";
  const targetLang = (formData.get("targetLang") as string | null) ?? "en";

  const audioBuffer = await audio.arrayBuffer();
  const service = new TranslationService(c.env);
  const result = await service.process(audioBuffer, sourceLang, targetLang);

  return c.json(result);
});

export default translationApp;
