import { Hono, type Context } from "hono";
import { cors } from "hono/cors";
import { swaggerUI } from "@hono/swagger-ui";
import {
  AddStreamRequestSchema,
  AuthTokenRequestSchema,
  BillingSummaryQuerySchema,
  CloneSessionVoiceRequestSchema,
  CompleteOnboardingRequestSchema,
  CreateSessionRequestSchema,
  CreateVoiceRequestSchema,
  GripAuthRequestSchema,
  InternalSessionMetricsUpdateSchema,
  InternalSessionStatusUpdateSchema,
  PlatformQuerySchema,
  SaveCredentialRequestSchema,
  SessionIdParamsSchema,
  SessionStreamParamsSchema,
  TikTokAuthRequestSchema,
  UserQuerySchema,
} from "@brivva/contracts/http";
import {
  GoogleSigninCallbackQuerySchema,
  YoutubeCallbackQuerySchema,
} from "@brivva/contracts/oauth";

import { signJwt } from "../shared/auth/jwt";
import * as db from "../shared/db/db";
import { validateVoiceSample } from "../shared/audio/wav-duration";
import * as el from "../features/voices/elevenlabs-client";
import { buildOpenApiDocument } from "./openapi";
import { toUserInfo, type Env } from "../core/types";
import * as yt from "../features/youtube/google-oauth-client";
import * as gsignin from "../features/auth/google-signin-client";
import { verifyStripeSignature } from "../features/billing/stripe-webhook";

const app = new Hono<{ Bindings: Env }>();

function invalid(c: Context<{ Bindings: Env }>, error: unknown) {
  if (error && typeof error === "object" && "issues" in error && Array.isArray(error.issues)) {
    return c.json(
      {
        error: "invalid request",
        issues: error.issues.map((issue) => ({
          path: issue.path.join("."),
          message: issue.message,
        })),
      },
      400,
    );
  }
  return c.json({ error: "invalid request" }, 400);
}

function parseWithSchema<T>(
  c: Context<{ Bindings: Env }>,
  schema: { safeParse(input: unknown): { success: true; data: T } | { success: false; error: unknown } },
  input: unknown,
): T | Response {
  const parsed = schema.safeParse(input);
  return parsed.success ? parsed.data : invalid(c, parsed.error);
}

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
app.get("/openapi.json", (c) => c.json(buildOpenApiDocument()));
app.get("/docs", swaggerUI({ url: "/openapi.json" }));

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
  const query = parseWithSchema(c, UserQuerySchema, {
    user_id: c.req.query("user_id"),
  });
  if (query instanceof Response) return query;
  const user = await db.getOrCreateUser(c.env.DB, query.user_id);
  return c.json(toUserInfo(user));
});

// Marks first-run onboarding as finished so the FE can stop gating UI on it.
// Idempotent — re-posting is a no-op after the first call.
app.post("/api/user/complete-onboarding", async (c) => {
  const body = parseWithSchema(c, CompleteOnboardingRequestSchema, await c.req.json());
  if (body instanceof Response) return body;
  await db.getOrCreateUser(c.env.DB, body.user_id);
  const user = await db.markOnboardingCompleted(c.env.DB, body.user_id);
  return c.json(toUserInfo(user));
});

// ── voices ───────────────────────────────────────────────

app.get("/api/voices", async (c) => {
  const query = parseWithSchema(c, UserQuerySchema, {
    user_id: c.req.query("user_id"),
  });
  if (query instanceof Response) return query;
  const voices = await db.listVoices(c.env.DB, query.user_id);
  return c.json({ voices });
});

// Upsert semantics: a user has at most one active voice clone. Creating a
// new one deletes the previous (both locally and in ElevenLabs) before the
// new row lands, and flips `users.active_voice_id` to the new id. This keeps
// the ElevenLabs voice library from accumulating stale clones and matches
// the FE "Re-record" flow, which conceptually replaces rather than adds.
app.post("/api/voices", async (c) => {
  const body = parseWithSchema(c, CreateVoiceRequestSchema, await c.req.json());
  if (body instanceof Response) return body;

  const validation = validateVoiceSample(body.audio_base64);
  if (!validation.ok) {
    return c.json({ error: validation.error }, 400);
  }

  const user = await db.getOrCreateUser(c.env.DB, body.user_id); // ensure FK

  // Delete prior clone first — failure here shouldn't block a new clone
  // (stale row is tolerable), but we try.
  if (user.active_voice_id) {
    const prior = await db.getVoice(c.env.DB, user.active_voice_id);
    if (prior) {
      try {
        await el.deleteRemoteVoice(c.env.ELEVENLABS_API_KEY, prior.elevenlabs_voice_id);
      } catch (e) {
        console.warn(`[voices] failed to delete prior ElevenLabs voice: ${String(e)}`);
      }
      await db.deleteVoiceRow(c.env.DB, prior.id);
    }
  }

  const { voice_id } = await el.cloneVoice(c.env.ELEVENLABS_API_KEY, {
    name: body.name,
    audioBase64: body.audio_base64,
    sourceLang: body.source_lang ?? null,
  });
  const voice = await db.createVoice(c.env.DB, {
    userId: body.user_id,
    elevenlabsVoiceId: voice_id,
    name: body.name,
    sourceLang: body.source_lang ?? null,
  });
  await db.setActiveVoice(c.env.DB, body.user_id, voice.id);
  return c.json(voice);
});

app.delete("/api/voices/:id", async (c) => {
  const id = c.req.param("id");
  const v = await db.getVoice(c.env.DB, id);
  if (!v) return c.json({ error: "not found" }, 404);
  await el.deleteRemoteVoice(c.env.ELEVENLABS_API_KEY, v.elevenlabs_voice_id);
  await db.deleteVoiceRow(c.env.DB, id);
  // Clear active pointer if the deleted voice was the active one.
  const user = await db.getOrCreateUser(c.env.DB, v.user_id);
  if (user.active_voice_id === id) {
    await db.setActiveVoice(c.env.DB, v.user_id, null);
  }
  return c.json({ status: "deleted" });
});

// ── platform credentials ─────────────────────────────────

app.get("/api/credentials", async (c) => {
  const query = parseWithSchema(c, UserQuerySchema, {
    user_id: c.req.query("user_id"),
  });
  if (query instanceof Response) return query;
  const credentials = await db.listCredentials(c.env.DB, query.user_id);
  return c.json({ credentials });
});

app.post("/api/credentials", async (c) => {
  const body = parseWithSchema(c, SaveCredentialRequestSchema, await c.req.json());
  if (body instanceof Response) return body;
  const row = await db.upsertCredential(c.env.DB, {
    userId: body.user_id,
    platform: body.platform,
    rtmpUrl: body.rtmp_url ?? null,
    streamKey: body.stream_key ?? null,
    displayName: body.display_name ?? null,
  });
  return c.json(row);
});

app.delete("/api/credentials", async (c) => {
  const query = parseWithSchema(c, PlatformQuerySchema, {
    user_id: c.req.query("user_id"),
    platform: c.req.query("platform"),
  });
  if (query instanceof Response) return query;
  await db.deleteCredentialRow(c.env.DB, query.user_id, query.platform);
  return c.json({ status: "deleted" });
});

// ── sessions ─────────────────────────────────────────────

app.get("/api/sessions", async (c) => {
  const query = parseWithSchema(c, UserQuerySchema, {
    user_id: c.req.query("user_id"),
  });
  if (query instanceof Response) return query;
  const sessions = await db.listSessions(c.env.DB, query.user_id);
  return c.json({ sessions });
});

// Default RTMP delay (ms) if the frontend omits it. Tuned to cover the
// typical STT (~500ms) + translate + TTS (~500ms) round-trip.
const DEFAULT_DELAY_MS = 2000;
// Default ducking gain for the delayed host audio under translated TTS.
// 1.0 = full volume (right for a source-language stream), 0.2 = quiet
// underlay (right for target-language streams with TTS on top).
const DEFAULT_HOST_GAIN_TARGET = 0.2;
const DEFAULT_HOST_GAIN_SOURCE = 1.0;

function resolveHostGain(
  p: { host_gain?: number; lang?: string },
  sourceLang: string,
): number {
  if (typeof p.host_gain === "number") {
    return Math.max(0, Math.min(1, p.host_gain));
  }
  return p.lang === sourceLang ? DEFAULT_HOST_GAIN_SOURCE : DEFAULT_HOST_GAIN_TARGET;
}

app.post("/api/sessions", async (c) => {
  const body = parseWithSchema(c, CreateSessionRequestSchema, await c.req.json());
  if (body instanceof Response) return body;
  await db.getOrCreateUser(c.env.DB, body.user_id);
  const session = await db.createSession(c.env.DB, {
    userId: body.user_id,
    voiceId: body.voice_id ?? null,
    title: body.title,
    sourceLang: body.source_lang,
    targetLangs: JSON.stringify(body.target_langs),
  });

  const streams = [];
  for (const p of body.platforms ?? []) {
    if (!p.rtmp_url || !p.stream_key || !p.lang) continue;
    streams.push(
      await db.createStreamManual(c.env.DB, {
        sessionId: session.id,
        lang: p.lang,
        platform: p.platform,
        rtmpUrl: p.rtmp_url,
        streamKey: p.stream_key,
        delayMs: p.delay_ms ?? DEFAULT_DELAY_MS,
        hostGain: resolveHostGain(p, body.source_lang),
      }),
    );
  }

  return c.json({ session, streams });
});

app.get("/api/sessions/:id", async (c) => {
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;
  const session = await db.getSession(c.env.DB, params.id);
  const streams = await db.listStreams(c.env.DB, params.id);
  return c.json({ session, streams });
});

app.post("/api/sessions/:id/voice", async (c) => {
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;
  const body = parseWithSchema(c, CloneSessionVoiceRequestSchema, await c.req.json());
  if (body instanceof Response) return body;

  const session = await db.getSession(c.env.DB, params.id);
  if (!session) return c.json({ error: "not found" }, 404);
  if (session.user_id !== body.user_id) {
    return c.json({ error: "forbidden" }, 403);
  }

  const validation = validateVoiceSample(body.audio_base64);
  if (!validation.ok) {
    return c.json({ error: validation.error }, 400);
  }

  const sourceLang = body.source_lang ?? session.source_lang;
  const voiceName = body.name?.trim() || `${session.title} Host Voice`;
  const { voice_id } = await el.cloneVoice(c.env.ELEVENLABS_API_KEY, {
    name: voiceName,
    audioBase64: body.audio_base64,
    sourceLang,
  });
  const voice = await db.createVoice(c.env.DB, {
    userId: body.user_id,
    elevenlabsVoiceId: voice_id,
    name: voiceName,
    sourceLang,
  });
  await db.updateSessionVoiceId(c.env.DB, params.id, voice.id);
  return c.json({ voice });
});

app.delete("/api/sessions/:id", async (c) => {
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;
  await db.deleteSessionRow(c.env.DB, params.id);
  return c.json({ status: "deleted" });
});

app.post("/api/sessions/:id/streams", async (c) => {
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;
  const body = parseWithSchema(c, AddStreamRequestSchema, await c.req.json());
  if (body instanceof Response) return body;
  // Stand-alone add-stream: we don't know session.source_lang here without a
  // DB read. FE must send host_gain explicitly when it differs from the
  // target-stream default.
  const hostGain =
    typeof body.host_gain === "number"
      ? Math.max(0, Math.min(1, body.host_gain))
      : DEFAULT_HOST_GAIN_TARGET;
  const s = await db.createStreamManual(c.env.DB, {
    sessionId: params.id,
    lang: body.lang,
    platform: body.platform,
    rtmpUrl: body.rtmp_url,
    streamKey: body.stream_key,
    delayMs: body.delay_ms ?? DEFAULT_DELAY_MS,
    hostGain,
  });
  return c.json(s);
});

app.delete("/api/sessions/:session_id/streams/:stream_id", async (c) => {
  const params = parseWithSchema(c, SessionStreamParamsSchema, {
    session_id: c.req.param("session_id"),
    stream_id: c.req.param("stream_id"),
  });
  if (params instanceof Response) return params;
  await db.deleteStreamRow(c.env.DB, params.stream_id);
  return c.json({ status: "deleted" });
});

// ── session usage (billing feeder) ───────────────────────
// Populated from /internal/sessions/:id/metrics PATCHes that Fargate writes
// as STT/TTS minutes accumulate. Returns zeros for sessions that haven't
// produced metrics yet (status=setup, or no PATCHes happened during a short
// session).

function minutesFromSeconds(s: number): number {
  return Math.round((s / 60) * 100) / 100;
}

// Self-serve rate. B2B clients may land at a custom rate via a contract, but
// that's handled outside the API today (see docs/b2b-onboarding-notes.md).
const PER_OUTPUT_MINUTE_USD = 1.5;

function estimateCost(outputByLang: Record<string, number>): number {
  let total = 0;
  for (const v of Object.values(outputByLang)) total += v;
  return Math.round(total * PER_OUTPUT_MINUTE_USD * 100) / 100;
}

function parseTargetLangs(raw: string): string[] {
  try {
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((v) => typeof v === "string") : [];
  } catch {
    return [];
  }
}

// Pre-stream cost projection. `output_minutes = expected_minutes × target_lang_count`
// because we produce one translated stream per target language. Used by the
// FE "you'll spend ~$X" banner on session start.
app.get("/api/sessions/:id/quote", async (c) => {
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;
  const rawMinutes = c.req.query("expected_minutes");
  const expectedMinutes = Number(rawMinutes);
  if (!Number.isFinite(expectedMinutes) || expectedMinutes <= 0) {
    return c.json({ error: "expected_minutes must be a positive number" }, 400);
  }

  const session = await db.getSession(c.env.DB, params.id);
  if (!session) return c.json({ error: "not found" }, 404);

  const langCount = parseTargetLangs(session.target_langs).length;
  const outputMinutes = Math.round(expectedMinutes * langCount * 100) / 100;
  const costUsd = Math.round(outputMinutes * PER_OUTPUT_MINUTE_USD * 100) / 100;

  return c.json({
    session_id: params.id,
    expected_minutes: expectedMinutes,
    output_minutes: outputMinutes,
    per_output_minute_usd: PER_OUTPUT_MINUTE_USD,
    cost_usd: costUsd,
  });
});

// Unified live-or-final rollup. `is_final` is true once status is no longer
// `live`/`setup`, but the shape of the payload doesn't change — FE renders
// the same panel either way.
app.get("/api/sessions/:id/summary", async (c) => {
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;

  const session = await db.getSession(c.env.DB, params.id);
  if (!session) return c.json({ error: "not found" }, 404);

  const metrics = await db.getSessionMetrics(c.env.DB, params.id);
  const outputByLangSeconds: Record<string, number> = metrics
    ? (JSON.parse(metrics.output_seconds_json) as Record<string, number>)
    : {};
  const outputByLangMinutes: Record<string, number> = {};
  for (const [k, v] of Object.entries(outputByLangSeconds)) {
    outputByLangMinutes[k] = minutesFromSeconds(v);
  }

  const liveStatuses = new Set(["setup", "live"]);
  return c.json({
    session_id: params.id,
    status: session.status,
    is_final: !liveStatuses.has(session.status),
    source_minutes: minutesFromSeconds(metrics?.source_seconds ?? 0),
    output_minutes_by_lang: outputByLangMinutes,
    estimated_cost_usd: estimateCost(outputByLangMinutes),
    updated_at: metrics?.updated_at ?? null,
  });
});

app.get("/api/sessions/:id/usage", async (c) => {
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;

  const session = await db.getSession(c.env.DB, params.id);
  if (!session) return c.json({ error: "not found" }, 404);

  const metrics = await db.getSessionMetrics(c.env.DB, params.id);
  const outputByLangSeconds: Record<string, number> = metrics
    ? (JSON.parse(metrics.output_seconds_json) as Record<string, number>)
    : {};
  const outputByLangMinutes: Record<string, number> = {};
  for (const [k, v] of Object.entries(outputByLangSeconds)) {
    outputByLangMinutes[k] = minutesFromSeconds(v);
  }

  return c.json({
    session_id: params.id,
    source_minutes: minutesFromSeconds(metrics?.source_seconds ?? 0),
    output_minutes_by_lang: outputByLangMinutes,
    estimated_cost_usd: estimateCost(outputByLangMinutes),
  });
});

// ── OAuth (YouTube add-channel) ──────────────────────────
// /auth/youtube is the ADD-CHANNEL flow — grants YouTube scopes to an
// already-authenticated user so we can create broadcasts on their behalf.
// It is NOT the sign-in flow: /auth/google below owns sign-in.

app.get("/auth/youtube", (c) => {
  const query = parseWithSchema(c, UserQuerySchema, {
    user_id: c.req.query("user_id"),
  });
  if (query instanceof Response) return query;
  return c.redirect(yt.authorizeUrl(c.env, query.user_id));
});

app.get("/auth/youtube/callback", async (c) => {
  const query = parseWithSchema(c, YoutubeCallbackQuerySchema, {
    code: c.req.query("code"),
    state: c.req.query("state"),
    error: c.req.query("error"),
  });
  if (query instanceof Response) return query;
  const code = query.code;
  const state = query.state; // user_id
  const err = query.error;
  if (err) {
    return c.redirect(`${c.env.FRONTEND_URL}/?oauth_error=${encodeURIComponent(err)}`);
  }
  if (!code || !state) return c.json({ error: "code + state required" }, 400);

  try {
    const tokens = await yt.exchangeCode(c.env, code);
    const channel = await yt.getChannelInfo(tokens.access_token);
    await db.getOrCreateUser(c.env.DB, state);
    const expiresAt = Math.floor(Date.now() / 1000) + tokens.expires_in;
    await db.updateYouTubeTokens(c.env.DB, {
      userId: state,
      accessToken: tokens.access_token,
      refreshToken: tokens.refresh_token ?? "",
      expiresAt,
      channelId: channel.id,
      channelName: channel.title,
    });
    const jwt = await signJwt(c.env.JWT_SECRET, { sub: state });
    // Pass JWT via URL fragment (not readable by servers/logs).
    return c.redirect(`${c.env.FRONTEND_URL}/?user_id=${encodeURIComponent(state)}#token=${jwt}`);
  } catch (e) {
    return c.json({ error: String(e) }, 500);
  }
});

// ── OAuth (Google sign-in) ───────────────────────────────
// Separate Google Client redirect entry. Uses openid+email+profile scope,
// identifies users by the Google `sub` (never user-supplied), and mints
// the same JWT shape /auth/token returns so the FE path after sign-in is
// unchanged.

app.get("/auth/google", (c) => {
  // State is an opaque CSRF token; for sign-in we don't have a user_id yet.
  const state = crypto.randomUUID();
  return c.redirect(gsignin.authorizeUrl(c.env, state));
});

app.get("/auth/google/callback", async (c) => {
  const query = parseWithSchema(c, GoogleSigninCallbackQuerySchema, {
    code: c.req.query("code"),
    state: c.req.query("state"),
    error: c.req.query("error"),
  });
  if (query instanceof Response) return query;
  if (query.error) {
    return c.redirect(`${c.env.FRONTEND_URL}/?oauth_error=${encodeURIComponent(query.error)}`);
  }
  if (!query.code) return c.json({ error: "code required" }, 400);

  try {
    const tokens = await gsignin.exchangeCode(c.env, query.code);
    const info = await gsignin.fetchUserInfo(tokens.access_token);
    const user = await db.getOrCreateUser(c.env.DB, info.sub);
    await db.updateUserProfile(c.env.DB, {
      userId: user.id,
      email: info.email,
      name: info.name,
      picture: info.picture,
    });
    const jwt = await signJwt(c.env.JWT_SECRET, { sub: user.id });
    return c.redirect(
      `${c.env.FRONTEND_URL}/?user_id=${encodeURIComponent(user.id)}#token=${jwt}`,
    );
  } catch (e) {
    return c.json({ error: String(e) }, 500);
  }
});

// Short-lived JWT issuance for already-authenticated users (called by FE on
// page load if it has a user_id but no fresh JWT).
app.post("/auth/token", async (c) => {
  const body = parseWithSchema(c, AuthTokenRequestSchema, await c.req.json());
  if (body instanceof Response) return body;
  const user = await db.getOrCreateUser(c.env.DB, body.user_id);
  const jwt = await signJwt(c.env.JWT_SECRET, { sub: user.id });
  return c.json({ token: jwt });
});

// ── Platform credential paste flows (Grip, TikTok) ───────
// These platforms don't expose real OAuth for live streaming — the creator
// pastes a session token + stream key from their host dashboard. The
// session token is stored as display_name metadata; the stream key + RTMP
// URL are what Fargate actually pushes to.

app.post("/auth/grip", async (c) => {
  const body = parseWithSchema(c, GripAuthRequestSchema, await c.req.json());
  if (body instanceof Response) return body;
  await db.getOrCreateUser(c.env.DB, body.user_id);
  const row = await db.upsertCredential(c.env.DB, {
    userId: body.user_id,
    platform: "grip",
    rtmpUrl: body.rtmp_url ?? null,
    streamKey: body.stream_key,
    displayName: body.display_name ?? `grip:${body.session_token.slice(0, 6)}…`,
  });
  return c.json(row);
});

app.post("/auth/tiktok", async (c) => {
  const body = parseWithSchema(c, TikTokAuthRequestSchema, await c.req.json());
  if (body instanceof Response) return body;
  await db.getOrCreateUser(c.env.DB, body.user_id);
  const row = await db.upsertCredential(c.env.DB, {
    userId: body.user_id,
    platform: "tiktok",
    rtmpUrl: body.rtmp_url ?? null,
    streamKey: body.stream_key,
    displayName: body.display_name ?? `tiktok:${body.session_token.slice(0, 6)}…`,
  });
  return c.json(row);
});

// ── Billing (Stripe scaffolding) ─────────────────────────

function monthWindow(nowSeconds: number): { start: number; end: number } {
  const d = new Date(nowSeconds * 1000);
  const start = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), 1) / 1000;
  const end = Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 1) / 1000;
  return { start, end };
}

// Flat published rate. B2B contract rates are negotiated offline; the FE
// displays this for self-serve users and for quoting.
app.get("/api/billing/rate", (c) => {
  return c.json({ per_output_minute_usd: PER_OUTPUT_MINUTE_USD });
});

app.get("/api/billing/summary", async (c) => {
  const query = parseWithSchema(c, BillingSummaryQuerySchema, {
    user_id: c.req.query("user_id"),
  });
  if (query instanceof Response) return query;

  const metrics = await db.listUserSessionMetrics(c.env.DB, query.user_id);
  let sourceSeconds = 0;
  const outputByLangSeconds: Record<string, number> = {};
  for (const m of metrics) {
    sourceSeconds += m.source_seconds;
    const perSession = JSON.parse(m.output_seconds_json) as Record<string, number>;
    for (const [lang, secs] of Object.entries(perSession)) {
      outputByLangSeconds[lang] = (outputByLangSeconds[lang] ?? 0) + secs;
    }
  }

  const outputByLangMinutes: Record<string, number> = {};
  for (const [k, v] of Object.entries(outputByLangSeconds)) {
    outputByLangMinutes[k] = minutesFromSeconds(v);
  }
  const window = monthWindow(Math.floor(Date.now() / 1000));

  return c.json({
    user_id: query.user_id,
    period_start: window.start,
    period_end: window.end,
    source_minutes: minutesFromSeconds(sourceSeconds),
    output_minutes_by_lang: outputByLangMinutes,
    estimated_cost_usd: estimateCost(outputByLangMinutes),
  });
});

app.post("/stripe/webhook", async (c) => {
  const payload = await c.req.text();
  const sigHeader = c.req.header("Stripe-Signature") ?? null;
  const secret = c.env.STRIPE_WEBHOOK_SECRET;

  if (!secret) {
    console.warn("[stripe] webhook received without STRIPE_WEBHOOK_SECRET set");
    return c.json({ received: true, verified: false }, 200);
  }

  const result = await verifyStripeSignature(
    payload,
    sigHeader,
    secret,
    Math.floor(Date.now() / 1000),
  );
  if (!result.ok) {
    console.warn(`[stripe] signature rejected: ${result.reason}`);
    return c.json({ error: result.reason }, 400);
  }
  console.log(`[stripe] webhook verified: ${result.eventType ?? "unknown"}`);
  return c.json({ received: true, verified: true }, 200);
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

// Session + streams bundle for Fargate to bootstrap a live session. Voice is joined
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

// Status update (Fargate → Workers when a live session starts / ends).
app.patch("/internal/sessions/:id", async (c) => {
  const err = requireInternal(c);
  if (err) return err;
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;
  const body = parseWithSchema(c, InternalSessionStatusUpdateSchema, await c.req.json());
  if (body instanceof Response) return body;
  await db.updateSessionStatus(c.env.DB, {
    id: params.id,
    status: body.status,
    liveSessionId: body.live_session_id ?? null,
  });
  return c.json({ status: "ok" });
});

// Metrics update (Fargate → Workers as STT/TTS minutes accumulate).
// Merge-semantics: omitted fields leave prior values alone; per-lang output
// seconds are shallow-merged so a single target-language delta doesn't clobber
// the others.
app.patch("/internal/sessions/:id/metrics", async (c) => {
  const err = requireInternal(c);
  if (err) return err;
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;
  const body = parseWithSchema(
    c,
    InternalSessionMetricsUpdateSchema,
    await c.req.json(),
  );
  if (body instanceof Response) return body;

  const session = await db.getSession(c.env.DB, params.id);
  if (!session) return c.json({ error: "not found" }, 404);

  await db.upsertSessionMetrics(c.env.DB, {
    sessionId: params.id,
    sourceSeconds: body.source_seconds,
    outputSecondsByLang: body.output_seconds_by_lang,
  });
  return c.json({ status: "ok" });
});

export default app;
