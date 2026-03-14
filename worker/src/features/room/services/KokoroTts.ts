import type { Lang } from "../types";

const VOICE_MAP: Record<Lang, string> = {
  en: "af_bella",
  ja: "jf_alpha",
  zh: "zf_xiaobei",
};

export class KokoroTts {
  constructor(private baseUrl: string, private roomId: string) {}

  async synthesize(lang: Lang, text: string): Promise<ReadableStream | null> {
    try {
      const resp = await fetch(`${this.baseUrl}/v1/audio/speech`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ model: "kokoro", voice: VOICE_MAP[lang], input: text }),
      });
      return resp.ok ? resp.body : null;
    } catch (err) {
      console.error(`[room:${this.roomId}] TTS ${lang} error:`, err);
      return null;
    }
  }
}
