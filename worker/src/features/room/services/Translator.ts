import type { Lang, TranslationResult } from "../types";

type M2MResult = { translated_text: string };

export class Translator {
  constructor(
    private ai: Ai,
    private sourceLang: string,
    private roomId: string,
  ) {}

  async translateAll(transcript: string, langs: Lang[]): Promise<TranslationResult[]> {
    return Promise.all(langs.map((lang) => this.translate(transcript, lang)));
  }

  private async translate(transcript: string, lang: Lang): Promise<TranslationResult> {
    const t0 = Date.now();
    try {
      const result = await (
        this.ai.run as (m: string, i: object) => Promise<M2MResult>
      )("@cf/meta/m2m100-1.2b", { text: transcript, source_lang: this.sourceLang, target_lang: lang });
      return { lang, text: result.translated_text?.trim() ?? "", translateMs: Date.now() - t0 };
    } catch (err) {
      console.error(`[room:${this.roomId}] translate ${lang} error:`, err);
      return { lang, text: "", translateMs: 0 };
    }
  }
}
