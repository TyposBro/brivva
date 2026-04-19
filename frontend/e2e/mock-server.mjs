// Mock backend for Playwright E2E.
// Serves HTTP (auth, sessions, onboarding, quote, summary) + WS at /api/session.
// Tests drive backend events via POST /test/* endpoints.
import { createServer } from "node:http";
import { WebSocketServer } from "ws";
import { URL } from "node:url";

const PORT = Number(process.env.MOCK_PORT ?? 8787);

/** @type {import("ws").WebSocket | null} */
let activeSocket = null;
/** @type {{ params: URLSearchParams } | null} */
let activeMeta = null;
let authFailNext = false;
let wsRejectNext = false;
let voiceCloneFailNext = false;
let summaryFailNext = false;
let killSwitchActive = false;

// Per-user mock state for onboarding-aware flows.
const users = new Map();

function defaultUser(id) {
  return {
    id,
    youtube_connected: false,
    youtube_channel_name: null,
    youtube_channel_id: null,
    email: `${id}@example.com`,
    name: id,
    picture: null,
    onboarding_completed_at: null,
    active_voice_id: null,
    billing_tier: "self_serve",
    bills_to: null,
    created_at: Math.floor(Date.now() / 1000),
  };
}

function getUser(id) {
  if (!users.has(id)) users.set(id, defaultUser(id));
  return users.get(id);
}

function patchUser(id, patch) {
  const u = getUser(id);
  Object.assign(u, patch);
  return u;
}

const voices = new Map();

function json(res, status, body) {
  res.writeHead(status, {
    "Content-Type": "application/json",
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Methods": "GET,POST,DELETE,OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
  });
  res.end(JSON.stringify(body));
}

function readBody(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8");
      try { resolve(raw ? JSON.parse(raw) : {}); } catch { resolve({}); }
    });
  });
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", `http://localhost:${PORT}`);
  if (req.method === "OPTIONS") return json(res, 200, {});

  // --- Test control endpoints ---
  if (url.pathname === "/test/emit" && req.method === "POST") {
    const body = await readBody(req);
    if (activeSocket && activeSocket.readyState === 1) {
      activeSocket.send(JSON.stringify(body));
      return json(res, 200, { ok: true });
    }
    return json(res, 409, { ok: false, reason: "no active socket" });
  }
  if (url.pathname === "/test/close" && req.method === "POST") {
    activeSocket?.close();
    return json(res, 200, { ok: true });
  }
  if (url.pathname === "/test/set-auth-fail" && req.method === "POST") {
    authFailNext = true;
    return json(res, 200, { ok: true });
  }
  if (url.pathname === "/test/set-ws-reject" && req.method === "POST") {
    wsRejectNext = true;
    return json(res, 200, { ok: true });
  }
  if (url.pathname === "/test/set-voice-fail" && req.method === "POST") {
    voiceCloneFailNext = true;
    return json(res, 200, { ok: true });
  }
  if (url.pathname === "/test/set-summary-fail" && req.method === "POST") {
    summaryFailNext = true;
    return json(res, 200, { ok: true });
  }
  if (url.pathname === "/test/fire-kill-switch" && req.method === "POST") {
    killSwitchActive = true;
    if (activeSocket?.readyState === 1) {
      activeSocket.send(JSON.stringify({
        type: "error",
        message: "Voice clone broken — falling back to default voice",
      }));
    }
    return json(res, 200, { ok: true });
  }
  if (url.pathname === "/test/state" && req.method === "GET") {
    return json(res, 200, {
      hasSocket: !!activeSocket && activeSocket.readyState === 1,
      params: activeMeta ? Object.fromEntries(activeMeta.params) : null,
      killSwitchActive,
    });
  }
  if (url.pathname === "/test/seed-user" && req.method === "POST") {
    const body = await readBody(req);
    const u = patchUser(body.user_id ?? "e2e-user", body);
    return json(res, 200, u);
  }
  if (url.pathname === "/test/reset" && req.method === "POST") {
    activeSocket?.close();
    activeSocket = null;
    activeMeta = null;
    authFailNext = false;
    wsRejectNext = false;
    voiceCloneFailNext = false;
    summaryFailNext = false;
    killSwitchActive = false;
    users.clear();
    voices.clear();
    return json(res, 200, { ok: true });
  }

  // --- Real API shims ---
  if (url.pathname === "/auth/token" && req.method === "POST") {
    if (authFailNext) {
      authFailNext = false;
      return json(res, 401, { error: "invalid user" });
    }
    return json(res, 200, { token: "test-token" });
  }
  if (url.pathname === "/auth/google" && req.method === "GET") {
    // Skip Google round-trip — bounce straight back to the SPA with a fake
    // user_id + JWT in the OAuth-style fragment.
    const cb = `http://localhost:5174/?user_id=e2e-user#token=eyJhbGciOiJIUzI1NiJ9.${Buffer.from(JSON.stringify({ exp: Math.floor(Date.now() / 1000) + 900 })).toString("base64").replace(/=+$/, "").replace(/\+/g, "-").replace(/\//g, "_")}.sig`;
    res.writeHead(302, { Location: cb });
    res.end();
    return;
  }
  if (url.pathname === "/auth/youtube" && req.method === "GET") {
    const userId = url.searchParams.get("user_id") ?? "e2e-user";
    patchUser(userId, { youtube_connected: true, youtube_channel_name: "E2E Channel" });
    res.writeHead(302, { Location: "http://localhost:5174/dashboard?youtube=connected" });
    res.end();
    return;
  }
  if (url.pathname === "/api/user" && req.method === "GET") {
    const userId = url.searchParams.get("user_id") ?? "e2e-user";
    return json(res, 200, getUser(userId));
  }
  if (url.pathname === "/api/user/complete-onboarding" && req.method === "POST") {
    const body = await readBody(req);
    const u = patchUser(body.user_id ?? "e2e-user", {
      onboarding_completed_at: Math.floor(Date.now() / 1000),
    });
    return json(res, 200, u);
  }
  if (url.pathname === "/api/voices" && req.method === "POST") {
    if (voiceCloneFailNext) {
      voiceCloneFailNext = false;
      return json(res, 500, { error: "clone failed" });
    }
    const body = await readBody(req);
    const voice = {
      id: "v-" + Math.random().toString(36).slice(2, 8),
      user_id: body.user_id,
      elevenlabs_voice_id: "el1",
      name: body.name,
      source_lang: body.source_lang ?? null,
      created_at: Math.floor(Date.now() / 1000),
    };
    voices.set(body.user_id, voice);
    patchUser(body.user_id, { active_voice_id: voice.id });
    return json(res, 200, voice);
  }
  if (url.pathname === "/api/voices" && req.method === "GET") {
    const userId = url.searchParams.get("user_id") ?? "e2e-user";
    const v = voices.get(userId);
    return json(res, 200, { voices: v ? [v] : [] });
  }
  if (url.pathname === "/api/credentials" && req.method === "GET") {
    return json(res, 200, { credentials: [] });
  }
  if (url.pathname === "/api/sessions" && req.method === "GET") {
    return json(res, 200, { sessions: [] });
  }
  if (url.pathname === "/api/sessions" && req.method === "POST") {
    const body = await readBody(req);
    const session = {
      id: "s-" + Math.random().toString(36).slice(2, 8),
      user_id: body.user_id,
      voice_id: body.voice_id ?? "v-default",
      title: body.title,
      source_lang: body.source_lang,
      target_langs: JSON.stringify(body.target_langs),
      status: "ready",
      live_session_id: null,
      created_at: Math.floor(Date.now() / 1000),
    };
    const streams = (body.platforms ?? []).map((p, i) => ({
      id: "st" + i,
      session_id: session.id,
      lang: p.lang ?? body.target_langs[0] ?? "en",
      platform: p.platform,
      platform_broadcast_id: null,
      platform_stream_id: null,
      stream_key: p.stream_key ?? "k",
      rtmp_url: p.rtmp_url ?? "rtmp://mock/",
      status: "ready",
      delay_ms: p.delay_ms ?? 1500,
      host_gain: p.host_gain ?? 0.2,
      created_at: Math.floor(Date.now() / 1000),
    }));
    return json(res, 200, { session, streams, errors: [] });
  }
  if (url.pathname.match(/^\/api\/sessions\/[^/]+\/quote/) && req.method === "GET") {
    const minutes = Number(url.searchParams.get("expected_minutes") ?? 30);
    return json(res, 200, {
      estimated_cost_usd: minutes * 1.5,
      expected_minutes: minutes,
      breakdown: [{ lang: "ja", minutes, cost_usd: minutes * 1.5 }],
    });
  }
  if (url.pathname.match(/^\/api\/sessions\/[^/]+\/summary/) && req.method === "GET") {
    if (summaryFailNext) {
      // Keep failing until /test/reset — React StrictMode double-mounts the
      // SummaryModal effect, so a one-shot flag would let the second pass
      // succeed and overwrite the error state.
      return json(res, 500, { error: "summary unavailable" });
    }
    return json(res, 200, {
      total_minutes: 12,
      total_cost_usd: 18,
      breakdown: [{ lang: "ja", minutes: 12, cost_usd: 18 }],
    });
  }
  if (url.pathname.match(/^\/api\/sessions\/[^/]+\/voice$/) && req.method === "POST") {
    if (voiceCloneFailNext) {
      voiceCloneFailNext = false;
      return json(res, 500, { error: "clone failed" });
    }
    const body = await readBody(req);
    const voice = {
      id: "v-" + Math.random().toString(36).slice(2, 8),
      user_id: body.user_id,
      elevenlabs_voice_id: "el1",
      name: "session-voice",
      source_lang: body.source_lang ?? null,
      created_at: Math.floor(Date.now() / 1000),
    };
    voices.set(body.user_id, voice);
    patchUser(body.user_id, { active_voice_id: voice.id });
    return json(res, 200, { voice });
  }
  if (url.pathname.startsWith("/api/sessions/") && req.method === "GET") {
    const id = url.pathname.split("/")[3];
    return json(res, 200, {
      session: {
        id,
        user_id: "e2e-user",
        voice_id: "v-default",
        title: "E2E Test Session",
        source_lang: "en",
        target_langs: '["ja","zh"]',
        status: "live",
        live_session_id: "live-1",
        created_at: Math.floor(Date.now() / 1000) - 60,
      },
      streams: [
        { id: "st1", session_id: id, lang: "ja", platform: "custom", platform_broadcast_id: null, platform_stream_id: null, rtmp_url: "rtmp://mock/ja/", stream_key: "k", status: "ready", delay_ms: 1500, host_gain: 0.2, created_at: Math.floor(Date.now() / 1000) },
        { id: "st2", session_id: id, lang: "zh", platform: "custom", platform_broadcast_id: null, platform_stream_id: null, rtmp_url: "rtmp://mock/zh/", stream_key: "k", status: "ready", delay_ms: 3000, host_gain: 0.2, created_at: Math.floor(Date.now() / 1000) },
      ],
    });
  }
  if (url.pathname.startsWith("/api/sessions/") && req.method === "DELETE") {
    return json(res, 200, { status: "ended" });
  }

  return json(res, 404, { error: "not found", path: url.pathname });
});

const wss = new WebSocketServer({ noServer: true });
server.on("upgrade", (req, socket, head) => {
  if (wsRejectNext) {
    wsRejectNext = false;
    socket.destroy();
    return;
  }
  const url = new URL(req.url ?? "/", `http://localhost:${PORT}`);
  if (url.pathname !== "/api/session") {
    socket.destroy();
    return;
  }
  wss.handleUpgrade(req, socket, head, (ws) => {
    activeSocket = ws;
    activeMeta = { params: url.searchParams };
    ws.on("close", () => {
      if (activeSocket === ws) { activeSocket = null; activeMeta = null; }
    });
    // Drop binary audio silently; tests don't care about payload.
    ws.on("message", () => {});
  });
});

server.listen(PORT, () => {
  console.log(`mock-server listening on :${PORT}`);
});
