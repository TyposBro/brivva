// Stub for brivva-api Workers backend. server-rs calls:
//   GET  /internal/sessions/:id  → returns session bundle
//   PATCH /internal/sessions/:id → status update, return 200
//
// Fixture: one session "SMOKE001" for user "smoke-user", one English
// host + one Japanese translated RTMP stream pointing at the local mediamtx.
// Matches JWT `sub` in driver and exercises TTS instead of source passthrough.

const PORT = Number(process.env.PORT ?? 8787);
const MEDIAMTX_RTMP = process.env.MEDIAMTX_RTMP ?? "rtmp://rtmp:1935/live";

const bundle = {
  session: {
    id: "SMOKE001",
    user_id: "smoke-user",
    voice_id: "voice-row-1",
    title: "smoke",
    source_lang: "en",
    target_langs: "[\"ja\"]",
    status: "created",
    live_session_id: null,
    voice_preset: "cloned",
    created_at: Math.floor(Date.now() / 1000),
  },
  streams: [
    {
      id: "smoke-stream-ja",
      session_id: "SMOKE001",
      lang: "ja",
      platform: "local-test",
      platform_broadcast_id: null,
      platform_stream_id: null,
      rtmp_url: MEDIAMTX_RTMP,
      stream_key: "smoke-ja",
      status: "pending",
      delay_ms: 2000,
      host_gain: 0.2,
      created_at: Math.floor(Date.now() / 1000),
    },
  ],
  voice: {
    id: "voice-row-1",
    user_id: "smoke-user",
    elevenlabs_voice_id: "smoke-eleven-voice",
    name: "Smoke Voice",
    source_lang: "en",
    created_at: Math.floor(Date.now() / 1000),
  },
};

Bun.serve({
  port: PORT,
  async fetch(req) {
    const url = new URL(req.url);
    if (url.pathname === "/health") return new Response("ok");
    if (url.pathname.startsWith("/internal/sessions/")) {
      if (req.method === "GET") {
        return Response.json(bundle);
      }
      if (req.method === "PATCH") {
        const body = await req.text();
        console.log("[workers-stub] status PATCH:", body);
        return Response.json({ status: "ok" });
      }
    }
    return new Response("not found", { status: 404 });
  },
});

console.log(`[workers-stub] listening on :${PORT}`);
