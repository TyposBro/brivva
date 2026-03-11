import { Hono } from "hono";
import type { Bindings, Variables } from "../../../core/types";

const roomApp = new Hono<{ Bindings: Bindings; Variables: Variables }>();

// ── Types ────────────────────────────────────────────────────────────────────

type Lang = "en" | "ja" | "zh";

interface GuestInfo {
  ws: WebSocket;
  lang: Lang;
}

interface Room {
  id: string;
  sourceLang: string;
  hostWs: WebSocket;
  novaWs?: WebSocket;
  guests: Map<string, GuestInfo>;
  langGroups: Map<Lang, Set<string>>;
}

type NovaMessage = {
  type: string;
  channel?: { alternatives: [{ transcript: string }] };
  is_final?: boolean;
  speech_final?: boolean;
};

type M2MResult = { translated_text: string };

// ── In-memory room state ─────────────────────────────────────────────────────
// Lives in the worker isolate — fine for demo purposes

const rooms = new Map<string, Room>();

// ── Helpers ──────────────────────────────────────────────────────────────────

function genRoomId(): string {
  let id: string;
  do {
    id = Math.random().toString(36).slice(2, 8).toUpperCase();
  } while (rooms.has(id));
  return id;
}

const VOICE_MAP: Record<Lang, string> = {
  en: "af_bella",
  ja: "jf_alpha",
  zh: "zf_xiaobei",
};

function send(ws: WebSocket, data: string | Uint8Array) {
  try {
    ws.send(data);
  } catch {
    // guest may have disconnected
  }
}

function broadcastToLang(room: Room, lang: Lang, data: string | Uint8Array) {
  const ids = room.langGroups.get(lang);
  if (!ids) return;
  for (const id of ids) {
    const guest = room.guests.get(id);
    if (guest) send(guest.ws, data);
  }
}

function broadcastToAllGuests(room: Room, data: string | Uint8Array) {
  for (const [, guest] of room.guests) {
    send(guest.ws, data);
  }
}

function pushGuestCount(room: Room) {
  const counts: Record<string, number> = {};
  for (const [lang, ids] of room.langGroups) {
    counts[lang] = ids.size;
  }
  send(room.hostWs, JSON.stringify({ type: "room:guest_count", counts }));
}

function closeRoom(room: Room, reason = "room:closed") {
  broadcastToAllGuests(room, JSON.stringify({ type: reason }));
  room.novaWs?.close();
  rooms.delete(room.id);
}

// ── Fan-out: translate → TTS → broadcast ─────────────────────────────────────

async function fanOut(room: Room, transcript: string, utteranceId: number, env: Bindings) {
  const activeLangs = (["en", "ja", "zh"] as Lang[]).filter(
    (lang) => (room.langGroups.get(lang)?.size ?? 0) > 0
  );
  if (activeLangs.length === 0) return;

  // Translate all active languages in parallel
  const translations = await Promise.all(
    activeLangs.map(async (lang) => {
      const t0 = Date.now();
      try {
        const result = await (
          env.AI.run as (m: string, i: object) => Promise<M2MResult>
        )("@cf/meta/m2m100-1.2b", {
          text: transcript,
          source_lang: "ko",
          target_lang: lang,
        });
        return { lang, text: result.translated_text?.trim() ?? "", translateMs: Date.now() - t0 };
      } catch (err) {
        console.error(`[room:${room.id}] translate ${lang} error:`, err);
        return { lang, text: "", translateMs: 0 };
      }
    })
  );

  // TTS + broadcast per language in parallel
  await Promise.all(
    translations
      .filter(({ text }) => text.length > 0)
      .map(async ({ lang, text, translateMs }) => {
        broadcastToLang(
          room,
          lang,
          JSON.stringify({ type: "translation", text, utteranceId, translateMs })
        );

        const t1 = Date.now();
        let kokoroResp: Response;
        try {
          kokoroResp = await fetch(`${env.KOKORO_URL}/v1/audio/speech`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ model: "kokoro", voice: VOICE_MAP[lang], input: text }),
          });
        } catch (err) {
          console.error(`[room:${room.id}] TTS fetch ${lang} error:`, err);
          return;
        }

        if (!kokoroResp.ok || !kokoroResp.body) {
          console.error(`[room:${room.id}] Kokoro ${lang} error:`, kokoroResp.status);
          return;
        }

        broadcastToLang(room, lang, JSON.stringify({ type: "tts_start", utteranceId }));

        const reader = kokoroResp.body.getReader();
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          broadcastToLang(room, lang, value);
        }

        broadcastToLang(
          room,
          lang,
          JSON.stringify({ type: "tts_end", utteranceId, ttsMs: Date.now() - t1 })
        );
      })
  );
}

// ── Nova-3 message handler ────────────────────────────────────────────────────

async function handleNovaMessage(
  data: string | ArrayBuffer,
  room: Room,
  env: Bindings,
  state: { pending: string }
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

  if (!msg.is_final && !msg.speech_final) {
    if (!transcript) return;
    state.pending = transcript;
    broadcastToAllGuests(room, JSON.stringify({ type: "interim", transcript }));
    return;
  }

  const finalTranscript = transcript || state.pending;
  state.pending = "";
  if (!finalTranscript) return;

  const utteranceId = Date.now();
  broadcastToAllGuests(
    room,
    JSON.stringify({ type: "final", transcript: finalTranscript, utteranceId })
  );

  await fanOut(room, finalTranscript, utteranceId, env);
}

// ── Nova-3 WebSocket connection ───────────────────────────────────────────────

async function connectNova(room: Room, env: Bindings): Promise<void> {
  const url = new URL(
    `https://gateway.ai.cloudflare.com/v1/${env.CF_ACCOUNT_ID}/${env.CF_AI_GATEWAY_ID}/workers-ai`
  );
  url.searchParams.set("model", "@cf/deepgram/nova-3");
  url.searchParams.set("encoding", "linear16");
  url.searchParams.set("sample_rate", "16000");
  url.searchParams.set("channels", "1");
  url.searchParams.set("language", "ko");
  url.searchParams.set("interim_results", "true");
  url.searchParams.set("punctuate", "true");
  url.searchParams.set("smart_format", "true");
  url.searchParams.set("endpointing", "300");
  url.searchParams.set("utterance_end_ms", "1000");

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

  const novaWs = novaResponse.webSocket!;
  novaWs.accept();
  room.novaWs = novaWs;

  const novaState = { pending: "" };
  novaWs.addEventListener("message", (e) => {
    handleNovaMessage(e.data, room, env, novaState).catch(console.error);
  });
  novaWs.addEventListener("error", (e) => console.error(`[room:${room.id}] Nova error:`, e));
}

// ── WebSocket endpoint ────────────────────────────────────────────────────────

roomApp.get("/room", async (c) => {
  if (c.req.header("Upgrade") !== "websocket") {
    return c.text("WebSocket upgrade required", 426);
  }

  const { 0: client, 1: server } = new WebSocketPair();
  server.accept();

  const env = c.env;
  let role: "host" | "guest" | null = null;
  let roomId: string | null = null;
  let guestId: string | null = null;

  server.addEventListener("message", async (event) => {
    // Binary PCM from host → Nova-3
    if (event.data instanceof ArrayBuffer) {
      if (role !== "host" || !roomId) return;
      const novaWs = rooms.get(roomId)?.novaWs;
      if (novaWs?.readyState === WebSocket.OPEN) {
        novaWs.send(event.data);
      }
      return;
    }

    // JSON control messages
    let msg: { type: string; [k: string]: unknown };
    try {
      msg = JSON.parse(event.data as string);
    } catch {
      return;
    }

    // ── Host: create room ─────────────────────────────────────────────────
    if (!role && msg.type === "host:create") {
      role = "host";
      roomId = genRoomId();
      const room: Room = {
        id: roomId,
        sourceLang: (msg.sourceLang as string) ?? "ko",
        hostWs: server,
        guests: new Map(),
        langGroups: new Map([
          ["en", new Set()],
          ["ja", new Set()],
          ["zh", new Set()],
        ]),
      };
      rooms.set(roomId, room);

      try {
        await connectNova(room, env);
        send(server, JSON.stringify({ type: "room:created", roomId }));
      } catch (err) {
        send(server, JSON.stringify({ type: "error", message: String(err) }));
        rooms.delete(roomId);
        server.close();
      }
      return;
    }

    // ── Guest: join room ──────────────────────────────────────────────────
    if (!role && msg.type === "guest:join") {
      const targetRoom = rooms.get(msg.roomId as string);
      if (!targetRoom) {
        send(server, JSON.stringify({ type: "error", message: "Room not found" }));
        server.close();
        return;
      }

      const lang = msg.lang as Lang;
      if (!["en", "ja", "zh"].includes(lang)) {
        send(server, JSON.stringify({ type: "error", message: "Invalid language" }));
        server.close();
        return;
      }

      role = "guest";
      roomId = msg.roomId as string;
      guestId = crypto.randomUUID();
      targetRoom.guests.set(guestId, { ws: server, lang });
      targetRoom.langGroups.get(lang)!.add(guestId);

      send(server, JSON.stringify({ type: "room:joined", roomId, lang }));
      pushGuestCount(targetRoom);
      return;
    }

    // ── Host: end room ────────────────────────────────────────────────────
    if (role === "host" && msg.type === "host:end" && roomId) {
      const room = rooms.get(roomId);
      if (room) closeRoom(room);
      server.close();
    }
  });

  server.addEventListener("close", () => {
    if (role === "host" && roomId) {
      const room = rooms.get(roomId);
      if (room) closeRoom(room);
    } else if (role === "guest" && roomId && guestId) {
      const room = rooms.get(roomId);
      if (room) {
        const guest = room.guests.get(guestId);
        if (guest) {
          room.langGroups.get(guest.lang)?.delete(guestId);
          room.guests.delete(guestId);
          pushGuestCount(room);
        }
      }
    }
  });

  return new Response(null, { status: 101, webSocket: client } as ResponseInit);
});

export default roomApp;
