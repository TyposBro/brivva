// Mock backend for Playwright E2E.
// Serves HTTP (auth, sessions) + WS at /api/session.
// Tests drive backend events via POST /test/emit.
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
  if (url.pathname === "/test/state" && req.method === "GET") {
    return json(res, 200, {
      hasSocket: !!activeSocket && activeSocket.readyState === 1,
      params: activeMeta ? Object.fromEntries(activeMeta.params) : null,
    });
  }
  if (url.pathname === "/test/reset" && req.method === "POST") {
    activeSocket?.close();
    activeSocket = null;
    activeMeta = null;
    authFailNext = false;
    wsRejectNext = false;
    voiceCloneFailNext = false;
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
  if (url.pathname === "/api/user" && req.method === "GET") {
    return json(res, 200, {
      id: url.searchParams.get("user_id") ?? "u1",
      youtube_connected: false,
      youtube_channel_name: null,
      youtube_channel_id: null,
      created_at: Date.now(),
    });
  }
  if (url.pathname.match(/^\/api\/sessions\/[^/]+\/voice$/) && req.method === "POST") {
    if (voiceCloneFailNext) {
      voiceCloneFailNext = false;
      return json(res, 500, { error: "clone failed" });
    }
    return json(res, 200, {
      voice: { id: "v1", user_id: "u1", elevenlabs_voice_id: "el1", name: "e2e", created_at: Date.now() },
    });
  }
  if (url.pathname.startsWith("/api/sessions/") && req.method === "GET") {
    const id = url.pathname.split("/")[3];
    return json(res, 200, {
      session: {
        id, user_id: "u1", voice_id: null,
        title: "E2E Test Session",
        source_lang: "en",
        target_langs: "ja,zh",
        status: "ready",
        room_id: null,
        created_at: Date.now(),
      },
      streams: [
        { id: "st1", lang: "ja", platform: "custom", rtmp_url: "rtmp://mock/ja/", stream_key: "k", status: "ready" },
        { id: "st2", lang: "zh", platform: "custom", rtmp_url: "rtmp://mock/zh/", stream_key: "k", status: "ready" },
      ],
    });
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
