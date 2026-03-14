import type { Bindings } from "../../../core/types";
import type { SttCallbacks } from "../types";

type NovaMessage = {
  type: string;
  channel?: { alternatives: [{ transcript: string }] };
  is_final?: boolean;
  speech_final?: boolean;
};

const NOVA_PARAMS: Record<string, string> = {
  encoding: "linear16",
  sample_rate: "16000",
  channels: "1",
  interim_results: "true",
  punctuate: "true",
  smart_format: "true",
  endpointing: "300",
  utterance_end_ms: "1000",
};

export class NovaStt {
  private ws: WebSocket | null = null;
  private pending = "";

  constructor(
    private env: Bindings,
    private roomId: string,
    private callbacks: SttCallbacks,
  ) {}

  async connect(sourceLang: string): Promise<void> {
    const { url, headers } = this.buildConnection(sourceLang);

    const resp = await fetch(url, { headers: { Upgrade: "websocket", ...headers } });
    if (resp.status !== 101) {
      const body = await resp.text().catch(() => "");
      throw new Error(`Nova-3 WS ${resp.status}: ${body}`);
    }

    this.ws = resp.webSocket!;
    this.ws.accept();
    this.ws.addEventListener("message", (e) => this.handleMessage(e.data));
    this.ws.addEventListener("error", (e) => console.error(`[room:${this.roomId}] Nova error:`, e));
  }

  sendAudio(data: ArrayBuffer) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(data);
  }

  close() {
    this.ws?.close();
    this.ws = null;
  }

  // --- internal ---

  private buildConnection(sourceLang: string): { url: string; headers: Record<string, string> } {
    if (this.env.DEEPGRAM_API_KEY) return this.buildDirectDeepgram(sourceLang);
    return this.buildCfGateway();
  }

  private buildDirectDeepgram(sourceLang: string): { url: string; headers: Record<string, string> } {
    const url = new URL("https://api.deepgram.com/v1/listen");
    url.searchParams.set("model", "nova-3");
    url.searchParams.set("language", sourceLang);
    this.applyParams(url);
    return { url: url.toString(), headers: { Authorization: `Token ${this.env.DEEPGRAM_API_KEY}` } };
  }

  private buildCfGateway(): { url: string; headers: Record<string, string> } {
    const url = new URL(
      `https://gateway.ai.cloudflare.com/v1/${this.env.CF_ACCOUNT_ID}/${this.env.CF_AI_GATEWAY_ID}/workers-ai`,
    );
    url.searchParams.set("model", "@cf/deepgram/nova-3");
    url.searchParams.set("language", "en");
    this.applyParams(url);
    return { url: url.toString(), headers: { "cf-aig-authorization": `Bearer ${this.env.CF_API_TOKEN}` } };
  }

  private applyParams(url: URL) {
    for (const [key, value] of Object.entries(NOVA_PARAMS)) url.searchParams.set(key, value);
  }

  private async handleMessage(data: string | ArrayBuffer) {
    if (typeof data !== "string") return;

    const msg = this.parse(data);
    if (!msg) return;

    const transcript = msg.channel?.alternatives?.[0]?.transcript?.trim() ?? "";

    if (!msg.is_final && !msg.speech_final) {
      if (!transcript) return;
      this.pending = transcript;
      this.callbacks.onInterim(transcript);
    } else {
      const finalTranscript = transcript || this.pending;
      this.pending = "";
      if (finalTranscript) this.callbacks.onFinal(finalTranscript, Date.now());
    }
  }

  private parse(data: string): NovaMessage | null {
    try {
      const msg = JSON.parse(data) as NovaMessage;
      return msg.type === "Results" ? msg : null;
    } catch {
      return null;
    }
  }
}
