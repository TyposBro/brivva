import { Hono, type Context } from "hono";
import { cors } from "hono/cors";

import { signJwt } from "./auth";
import * as db from "./db";
import * as el from "./elevenlabs";
import { toUserInfo, type Env } from "./types";
import * as yt from "./youtube";

const app = new Hono<{ Bindings: Env }>();

app.use(
  "*",
  cors({
    origin: (o) => o ?? "*",
    allowHeaders: ["Content-Type", "Authorization", "X-Internal-Secret"],
    allowMethods: ["GET", "POST", "PATCH", "DELETE", "OPTIONS"],
    credentials: true,
  }),
);

app.get("/", (c) => c.text("Brivva API (Workers + D1)"));
app.get("/health", (c) => c.json({ ok: true }));

// ── Internal auth for server→worker calls ─────────────────
// Fargate uses this on token-refresh writes (Phase 3 — currently unused).
function requireInternal(c: Context<{ Bindings: Env }>): Response | null {
  const got = c.req.header("X-Internal-Secret");
  if (!got || got !== c.env.INTERNAL_SECRET) {
    return new Response("unauthorized", { status: 401 });
  }
  return null;
}

// ── users ────────────────────────────────────────────────

app.get("/api/user", async (c) => {
  const userId = c.req.query("user_id");
  if (!userId) return c.json({ error: "user_id required" }, 400);
  const user = await db.getOrCreateUser(c.env.DB, userId);
  return c.json(toUserInfo(user));
});

// ── voices ───────────────────────────────────────────────

app.get("/api/voices", async (c) => {
  const userId = c.req.query("user_id");
  if (!userId) return c.json({ error: "user_id required" }, 400);
  const voices = await db.listVoices(c.env.DB, userId);
  return c.json({ voices });
});

app.post("/api/voices", async (c) => {
  const body = await c.req.json<{
    user_id?: string;
    name?: string;
    audio_base64?: string;
  }>();
  if (!body.user_id || !body.name || !body.audio_base64) {
    return c.json({ error: "user_id, name, audio_base64 required" }, 400);
  }
  await db.getOrCreateUser(c.env.DB, body.user_id); // ensure FK
  const { voice_id } = await el.cloneVoice(
    c.env.ELEVENLABS_API_KEY,
    body.name,
    body.audio_base64,
  );
  const voice = await db.createVoice(c.env.DB, body.user_id, voice_id, body.name);
  return c.json(voice);
});

app.delete("/api/voices/:id", async (c) => {
  const id = c.req.param("id");
  const v = await db.getVoice(c.env.DB, id);
  if (!v) return c.json({ error: "not found" }, 404);
  await el.deleteRemoteVoice(c.env.ELEVENLABS_API_KEY, v.elevenlabs_voice_id);
  await db.deleteVoiceRow(c.env.DB, id);
  return c.json({ status: "deleted" });
});

// ── platform credentials ─────────────────────────────────

app.get("/api/credentials", async (c) => {
  const userId = c.req.query("user_id");
  if (!userId) return c.json({ error: "user_id required" }, 400);
  const credentials = await db.listCredentials(c.env.DB, userId);
  return c.json({ credentials });
});

app.post("/api/credentials", async (c) => {
  const body = await c.req.json<{
    user_id?: string;
    platform?: string;
    rtmp_url?: string;
    stream_key?: string;
    display_name?: string;
  }>();
  if (!body.user_id || !body.platform) {
    return c.json({ error: "user_id, platform required" }, 400);
  }
  const row = await db.upsertCredential(
    c.env.DB,
    body.user_id,
    body.platform,
    body.rtmp_url ?? null,
    body.stream_key ?? null,
    body.display_name ?? null,
  );
  return c.json(row);
});

app.delete("/api/credentials", async (c) => {
  const userId = c.req.query("user_id");
  const platform = c.req.query("platform");
  if (!userId || !platform)
    return c.json({ error: "user_id + platform required" }, 400);
  await db.deleteCredentialRow(c.env.DB, userId, platform);
  return c.json({ status: "deleted" });
});

// ── sessions ─────────────────────────────────────────────

app.get("/api/sessions", async (c) => {
  const userId = c.req.query("user_id");
  if (!userId) return c.json({ error: "user_id required" }, 400);
  const sessions = await db.listSessions(c.env.DB, userId);
  return c.json({ sessions });
});

app.post("/api/sessions", async (c) => {
  const body = await c.req.json<{
    user_id?: string;
    title?: string;
    source_lang?: string;
    target_langs?: string[];
    voice_id?: string;
    platforms?: { platform: string; lang?: string; rtmp_url?: string; stream_key?: string }[];
  }>();
  if (!body.user_id || !body.title || !body.source_lang || !body.target_langs?.length) {
    return c.json({ error: "user_id, title, source_lang, target_langs required" }, 400);
  }
  await db.getOrCreateUser(c.env.DB, body.user_id);
  const session = await db.createSession(
    c.env.DB,
    body.user_id,
    body.voice_id ?? null,
    body.title,
    body.source_lang,
    JSON.stringify(body.target_langs),
  );

  // Optional: attach manual RTMP streams for non-auto platforms.
  const streams = [];
  for (const p of body.platforms ?? []) {
    if (!p.rtmp_url || !p.stream_key || !p.lang) continue;
    streams.push(
      await db.createStreamManual(
        c.env.DB,
        session.id,
        p.lang,
        p.platform,
        p.rtmp_url,
        p.stream_key,
      ),
    );
  }

  return c.json({ session, streams });
});

app.get("/api/sessions/:id", async (c) => {
  const id = c.req.param("id");
  const session = await db.getSession(c.env.DB, id);
  const streams = await db.listStreams(c.env.DB, id);
  return c.json({ session, streams });
});

app.delete("/api/sessions/:id", async (c) => {
  const id = c.req.param("id");
  await db.deleteSessionRow(c.env.DB, id);
  return c.json({ status: "deleted" });
});

app.post("/api/sessions/:id/streams", async (c) => {
  const sessionId = c.req.param("id");
  const body = await c.req.json<{
    lang?: string;
    platform?: string;
    rtmp_url?: string;
    stream_key?: string;
  }>();
  if (!body.lang || !body.platform || !body.rtmp_url || !body.stream_key) {
    return c.json({ error: "lang, platform, rtmp_url, stream_key required" }, 400);
  }
  const s = await db.createStreamManual(
    c.env.DB,
    sessionId,
    body.lang,
    body.platform,
    body.rtmp_url,
    body.stream_key,
  );
  return c.json(s);
});

app.delete("/api/sessions/:session_id/streams/:stream_id", async (c) => {
  await db.deleteStreamRow(c.env.DB, c.req.param("stream_id"));
  return c.json({ status: "deleted" });
});

// ── OAuth (YouTube) ──────────────────────────────────────

app.get("/auth/youtube", (c) => {
  const userId = c.req.query("user_id");
  if (!userId) return c.json({ error: "user_id required" }, 400);
  return c.redirect(yt.authorizeUrl(c.env, userId));
});

app.get("/auth/youtube/callback", async (c) => {
  const code = c.req.query("code");
  const state = c.req.query("state"); // user_id
  const err = c.req.query("error");
  if (err) {
    return c.redirect(`${c.env.FRONTEND_URL}/?oauth_error=${encodeURIComponent(err)}`);
  }
  if (!code || !state) return c.json({ error: "code + state required" }, 400);

  try {
    const tokens = await yt.exchangeCode(c.env, code);
    const channel = await yt.getChannelInfo(tokens.access_token);
    await db.getOrCreateUser(c.env.DB, state);
    const expiresAt = Math.floor(Date.now() / 1000) + tokens.expires_in;
    await db.updateYouTubeTokens(
      c.env.DB,
      state,
      tokens.access_token,
      tokens.refresh_token ?? "",
      expiresAt,
      channel.id,
      channel.title,
    );
    const jwt = await signJwt(c.env.JWT_SECRET, { sub: state });
    // Pass JWT via URL fragment (not readable by servers/logs).
    return c.redirect(`${c.env.FRONTEND_URL}/?user_id=${encodeURIComponent(state)}#token=${jwt}`);
  } catch (e) {
    return c.json({ error: String(e) }, 500);
  }
});

// Short-lived JWT issuance for already-authenticated users (called by FE on
// page load if it has a user_id but no fresh JWT).
app.post("/auth/token", async (c) => {
  const body = await c.req.json<{ user_id?: string }>();
  if (!body.user_id) return c.json({ error: "user_id required" }, 400);
  const user = await db.getOrCreateUser(c.env.DB, body.user_id);
  const jwt = await signJwt(c.env.JWT_SECRET, { sub: user.id });
  return c.json({ token: jwt });
});

// ── Internal (Fargate → Worker) ──────────────────────────
// Used by Fargate's ffmpeg/streaming layer to fetch a user's voice clone id
// or their RTMP creds without opening D1 directly. Secured with a shared secret.

app.get("/internal/users/:id", async (c) => {
  const err = requireInternal(c);
  if (err) return err;
  const u = await db.getOrCreateUser(c.env.DB, c.req.param("id"));
  return c.json(u);
});

app.get("/internal/voices/:id", async (c) => {
  const err = requireInternal(c);
  if (err) return err;
  const v = await db.getVoice(c.env.DB, c.req.param("id"));
  return c.json(v);
});

// Session + streams bundle for Fargate to bootstrap a room. Voice is joined
// in-line so Fargate doesn't need a second call.
app.get("/internal/sessions/:id", async (c) => {
  const err = requireInternal(c);
  if (err) return err;
  const id = c.req.param("id");
  const session = await db.getSession(c.env.DB, id);
  if (!session) return c.json({ error: "not found" }, 404);
  const streams = await db.listStreams(c.env.DB, id);
  const voice = session.voice_id ? await db.getVoice(c.env.DB, session.voice_id) : null;
  return c.json({ session, streams, voice });
});

// Status update (Fargate → Workers when room goes live / ends).
app.patch("/internal/sessions/:id", async (c) => {
  const err = requireInternal(c);
  if (err) return err;
  const id = c.req.param("id");
  const body = await c.req.json<{ status?: string; room_id?: string | null }>();
  if (!body.status) return c.json({ error: "status required" }, 400);
  await db.updateSessionStatus(c.env.DB, id, body.status, body.room_id ?? null);
  return c.json({ status: "ok" });
});

export default app;
