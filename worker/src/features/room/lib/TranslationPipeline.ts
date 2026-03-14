import type { Bindings } from "../../../core/types";
import { type Lang, type Broadcaster } from "./Broadcaster";

type M2MResult = { translated_text: string };

const VOICE_MAP: Record<Lang, string> = {
  en: "af_bella",
  ja: "jf_alpha",
  zh: "zf_xiaobei",
};

export class TranslationPipeline {
  constructor(
    private env: Bindings,
    private roomId: string,
    private broadcaster: Broadcaster,
    private sourceLang: string,
  ) {}

  async process(transcript: string, utteranceId: number) {
    const langs = this.broadcaster.activeLangs();
    if (!langs.length) return;

    const translations = await this.translateAll(transcript, langs);

    await Promise.all(
      translations
        .filter(({ text }) => text.length > 0)
        .map((t) => this.synthesizeAndBroadcast(t.lang, t.text, utteranceId, t.translateMs)),
    );
  }

  // --- translate ---

  private translateAll(transcript: string, langs: Lang[]) {
    return Promise.all(langs.map((lang) => this.translate(transcript, lang)));
  }

  private async translate(transcript: string, lang: Lang) {
    const t0 = Date.now();
    try {
      const result = await (
        this.env.AI.run as (m: string, i: object) => Promise<M2MResult>
      )("@cf/meta/m2m100-1.2b", { text: transcript, source_lang: this.sourceLang, target_lang: lang });
      return { lang, text: result.translated_text?.trim() ?? "", translateMs: Date.now() - t0 };
    } catch (err) {
      console.error(`[room:${this.roomId}] translate ${lang} error:`, err);
      return { lang, text: "", translateMs: 0 };
    }
  }

  // --- TTS ---

  private async synthesizeAndBroadcast(lang: Lang, text: string, utteranceId: number, translateMs: number) {
    this.broadcastTranslation(lang, text, utteranceId, translateMs);

    const t1 = Date.now();
    const audio = await this.callTts(lang, text);
    if (!audio) return;

    this.broadcaster.sendToLang(lang, JSON.stringify({ type: "tts_start", utteranceId }));
    await this.streamAudio(lang, audio);

    const ttsMs = Date.now() - t1;
    const ttsEndMsg = JSON.stringify({ type: "tts_end", utteranceId, ttsMs });
    this.broadcaster.sendToLang(lang, ttsEndMsg);
    this.broadcaster.sendToHost(ttsEndMsg);
  }

  private broadcastTranslation(lang: Lang, text: string, utteranceId: number, translateMs: number) {
    this.broadcaster.sendToLang(lang, JSON.stringify({ type: "translation", text, utteranceId, translateMs }));
    this.broadcaster.sendToHost(JSON.stringify({ type: "translation", utteranceId, translateMs }));
  }

  private async callTts(lang: Lang, text: string): Promise<ReadableStream | null> {
    try {
      const resp = await fetch(`${this.env.KOKORO_URL}/v1/audio/speech`, {
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

  private async streamAudio(lang: Lang, stream: ReadableStream) {
    const reader = stream.getReader();
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      this.broadcaster.sendToLang(lang, value);
    }
  }
}
