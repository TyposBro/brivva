import type { Lang, HostBroadcaster, Translator, Tts } from "../types";

export class TranslationPipeline {
  constructor(
    private broadcaster: HostBroadcaster,
    private translator: Translator,
    private tts: Tts,
  ) {}

  async process(transcript: string, utteranceId: number) {
    const langs = this.broadcaster.activeLangs();
    if (!langs.length) return;

    const translations = await this.translator.translateAll(transcript, langs);

    await Promise.all(
      translations
        .filter(({ text }) => text.length > 0)
        .map((t) => this.deliverToLang(t.lang, t.text, utteranceId, t.translateMs)),
    );
  }

  // --- internals ---

  private async deliverToLang(lang: Lang, text: string, utteranceId: number, translateMs: number) {
    this.broadcastTranslation(lang, text, utteranceId, translateMs);
    await this.synthesizeAndStream(lang, text, utteranceId);
  }

  private broadcastTranslation(lang: Lang, text: string, utteranceId: number, translateMs: number) {
    this.broadcaster.sendToLang(lang, JSON.stringify({ type: "translation", text, utteranceId, translateMs }));
    this.broadcaster.sendToHost(JSON.stringify({ type: "translation", utteranceId, translateMs }));
  }

  private async synthesizeAndStream(lang: Lang, text: string, utteranceId: number) {
    const t0 = Date.now();
    const audio = await this.tts.synthesize(lang, text);
    if (!audio) return;

    this.broadcaster.sendToLang(lang, JSON.stringify({ type: "tts_start", utteranceId }));
    await this.streamAudio(lang, audio);

    const ttsMs = Date.now() - t0;
    const ttsEndMsg = JSON.stringify({ type: "tts_end", utteranceId, ttsMs });
    this.broadcaster.sendToLang(lang, ttsEndMsg);
    this.broadcaster.sendToHost(ttsEndMsg);
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
