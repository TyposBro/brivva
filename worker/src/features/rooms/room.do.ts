import type { Bindings } from "../../core/types";

type Lang = "en" | "ja" | "zh";
type M2MResult = { translated_text: string };
type NovaMessage = {
  type: string;
  channel?: { alternatives: [{ transcript: string }] };
  is_final?: boolean;
  speech_final?: boolean;
};

const LANGS: Lang[] = ["en", "ja", "zh"];

const VOICE_MAP: Record<Lang, string> = {
  en: "af_bella",
  ja: "jf_alpha",
  zh: "zf_xiaobei",
};

// Shared Nova-3 streaming params (identical for both Deepgram direct and CF Gateway)
function setNovaParams(url: URL, language: string) {
  url.searchParams.set("encoding", "linear16");
  url.searchParams.set("sample_rate", "16000");
  url.searchParams.set("channels", "1");
  url.searchParams.set("language", language);
  url.searchParams.set("interim_results", "true");
  url.searchParams.set("punctuate", "true");
  url.searchParams.set("smart_format", "true");
  url.searchParams.set("endpointing", "300");
  url.searchParams.set("utterance_end_ms", "1000");
}

export class RoomDO {
  private roomId = "";
  private sourceLang = "en";
  private hostWs: WebSocket | null = null;
  private novaWs: WebSocket | null = null;
  private novaState = { pending: "" };
  private guests = new Map<string, { ws: WebSocket; lang: Lang }>();
  private langGroups = new Map<Lang, Set<string>>(LANGS.map((l) => [l, new Set()]));

  constructor(
    _state: DurableObjectState,
    private env: Bindings
  ) {}

  // ── Helpers ────────────────────────────────────────────────────────────────

  private send(ws: WebSocket, data: string | Uint8Array) {
    try { ws.send(data); } catch {}
  }

  private broadcastToLang(lang: Lang, data: string | Uint8Array) {
    for (const id of this.langGroups.get(lang) ?? []) {
      const guest = this.guests.get(id);
      if (guest) this.send(guest.ws, data);
    }
  }

  private broadcastToAllGuests(data: string | Uint8Array) {
    for (const [, guest] of this.guests) this.send(guest.ws, data);
  }

  private pushGuestCount() {
    if (!this.hostWs) return;
    const counts: Record<string, number> = {};
    for (const [lang, ids] of this.langGroups) counts[lang] = ids.size;
    this.send(this.hostWs, JSON.stringify({ type: "room:guest_count", counts }));
  }

  private closeRoom() {
    this.broadcastToAllGuests(JSON.stringify({ type: "room:closed" }));
    this.novaWs?.close();
    this.hostWs = null;
  }

  // ── Nova-3 ─────────────────────────────────────────────────────────────────

  private async connectNova(): Promise<void> {
    // Direct Deepgram API supports all languages including Korean.
    // CF AI Gateway Nova-3 binding only supports language=en.
    const useDirect = !!this.env.DEEPGRAM_API_KEY;

    let wsUrl: string;
    let authHeader: Record<string, string>;

    if (useDirect) {
      const url = new URL("https://api.deepgram.com/v1/listen");
      url.searchParams.set("model", "nova-3");
      setNovaParams(url, this.sourceLang);
      wsUrl = url.toString();
      authHeader = { Authorization: `Token ${this.env.DEEPGRAM_API_KEY}` };
    } else {
      // CF AI Gateway fallback — English only
      const url = new URL(
        `https://gateway.ai.cloudflare.com/v1/${this.env.CF_ACCOUNT_ID}/${this.env.CF_AI_GATEWAY_ID}/workers-ai`
      );
      url.searchParams.set("model", "@cf/deepgram/nova-3");
      setNovaParams(url, "en");
      wsUrl = url.toString();
      authHeader = { "cf-aig-authorization": `Bearer ${this.env.CF_API_TOKEN}` };
    }

    const resp = await fetch(wsUrl, {
      headers: { Upgrade: "websocket", ...authHeader },
    });

    if (resp.status !== 101) {
      const body = await resp.text().catch(() => "");
      throw new Error(`Nova-3 WS ${resp.status}: ${body}`);
    }

    const novaWs = resp.webSocket!;
    novaWs.accept();
    this.novaWs = novaWs;

    novaWs.addEventListener("message", (e) => {
      this.handleNovaMessage(e.data).catch(console.error);
    });
    novaWs.addEventListener("error", (e) => console.error(`[room:${this.roomId}] Nova error:`, e));
  }

  private async handleNovaMessage(data: string | ArrayBuffer) {
    if (typeof data !== "string") return;
    let msg: NovaMessage;
    try { msg = JSON.parse(data) as NovaMessage; } catch { return; }
    if (msg.type !== "Results") return;

    const transcript = msg.channel?.alternatives?.[0]?.transcript?.trim() ?? "";

    if (!msg.is_final && !msg.speech_final) {
      if (!transcript) return;
      this.novaState.pending = transcript;
      const interimMsg = JSON.stringify({ type: "interim", transcript });
      if (this.hostWs) this.send(this.hostWs, interimMsg);
      this.broadcastToAllGuests(interimMsg);
      return;
    }

    const finalTranscript = transcript || this.novaState.pending;
    this.novaState.pending = "";
    if (!finalTranscript) return;

    const utteranceId = Date.now();
    const finalMsg = JSON.stringify({ type: "final", transcript: finalTranscript, utteranceId });
    if (this.hostWs) this.send(this.hostWs, finalMsg);
    this.broadcastToAllGuests(finalMsg);
    await this.fanOut(finalTranscript, utteranceId);
  }

  // ── Fan-out ────────────────────────────────────────────────────────────────

  private async fanOut(transcript: string, utteranceId: number) {
    const activeLangs = LANGS.filter((lang) => (this.langGroups.get(lang)?.size ?? 0) > 0);
    if (!activeLangs.length) return;

    const translations = await Promise.all(
      activeLangs.map(async (lang) => {
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
      })
    );

    await Promise.all(
      translations
        .filter(({ text }) => text.length > 0)
        .map(async ({ lang, text, translateMs }) => {
          const translationMsg = JSON.stringify({ type: "translation", text, utteranceId, translateMs });
          this.broadcastToLang(lang, translationMsg);
          // Send translation timing to host for benchmark dashboard
          if (this.hostWs) this.send(this.hostWs, JSON.stringify({ type: "translation", utteranceId, translateMs }));

          const t1 = Date.now();
          let resp: Response;
          try {
            resp = await fetch(`${this.env.KOKORO_URL}/v1/audio/speech`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ model: "kokoro", voice: VOICE_MAP[lang], input: text }),
            });
          } catch (err) {
            console.error(`[room:${this.roomId}] TTS ${lang} error:`, err);
            return;
          }

          if (!resp.ok || !resp.body) return;

          this.broadcastToLang(lang, JSON.stringify({ type: "tts_start", utteranceId }));
          const reader = resp.body.getReader();
          while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            this.broadcastToLang(lang, value);
          }
          const ttsMs = Date.now() - t1;
          const ttsEndMsg = JSON.stringify({ type: "tts_end", utteranceId, ttsMs });
          this.broadcastToLang(lang, ttsEndMsg);
          // Send TTS timing to host for benchmark dashboard
          if (this.hostWs) this.send(this.hostWs, ttsEndMsg);
        })
    );
  }

  // ── fetch (WebSocket entrypoint) ───────────────────────────────────────────

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response("WebSocket upgrade required", { status: 426 });
    }

    const url = new URL(request.url);
    const role = url.searchParams.get("role");
    const roomId = url.searchParams.get("roomId") ?? "";
    const lang = url.searchParams.get("lang") as Lang | null;

    const { 0: client, 1: server } = new WebSocketPair();
    server.accept();

    if (role === "host") {
      if (this.hostWs) {
        server.send(JSON.stringify({ type: "error", message: "Room already has a host" }));
        server.close();
        return new Response(null, { status: 101, webSocket: client } as ResponseInit);
      }

      this.hostWs = server;
      this.roomId = roomId;
      this.sourceLang = url.searchParams.get("sourceLang") ?? "en";

      try {
        await this.connectNova();
        server.send(JSON.stringify({ type: "room:created", roomId }));
      } catch (err) {
        this.hostWs = null; // don't leave DO in broken state with hostWs set but no Nova
        server.send(JSON.stringify({ type: "error", message: String(err) }));
        server.close();
        return new Response(null, { status: 101, webSocket: client } as ResponseInit);
      }

      server.addEventListener("message", (event) => {
        if (event.data instanceof ArrayBuffer) {
          if (this.novaWs?.readyState === WebSocket.OPEN) this.novaWs.send(event.data);
        } else if (typeof event.data === "string") {
          try {
            const msg = JSON.parse(event.data as string);
            if (msg.type === "host:end") this.closeRoom();
          } catch {}
        }
      });

      server.addEventListener("close", () => this.closeRoom());

    } else if (role === "guest") {
      if (!this.hostWs) {
        server.send(JSON.stringify({ type: "error", message: "Room not found" }));
        server.close();
        return new Response(null, { status: 101, webSocket: client } as ResponseInit);
      }

      if (!lang || !LANGS.includes(lang)) {
        server.send(JSON.stringify({ type: "error", message: "Invalid language" }));
        server.close();
        return new Response(null, { status: 101, webSocket: client } as ResponseInit);
      }

      const guestId = crypto.randomUUID();
      this.guests.set(guestId, { ws: server, lang });
      this.langGroups.get(lang)?.add(guestId);

      server.send(JSON.stringify({ type: "room:joined", roomId, lang }));
      this.pushGuestCount();

      server.addEventListener("close", () => {
        const guest = this.guests.get(guestId);
        if (guest) {
          this.langGroups.get(guest.lang)?.delete(guestId);
          this.guests.delete(guestId);
          this.pushGuestCount();
        }
      });

    } else {
      server.close();
    }

    return new Response(null, { status: 101, webSocket: client } as ResponseInit);
  }
}
