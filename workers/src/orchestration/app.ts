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
  InternalSessionMetricsUpdateSchema,
  InternalSessionStatusUpdateSchema,
  PlatformQuerySchema,
  SaveCredentialRequestSchema,
  SessionIdParamsSchema,
  SessionStreamParamsSchema,
  TikTokAuthRequestSchema,
  UpdateSessionVoicePresetSchema,
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
import {
  createYouTubeBroadcast,
  YouTubeBroadcastError,
  type YouTubeBroadcastResult,
} from "../features/youtube/broadcast-api";
import {
  provisionBroadcast as provisionGripBroadcast,
  GripSellerApiError,
  type GripBroadcastResult,
} from "../features/grip/seller-api";
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
  const user = await db.getOrCreateUser(c.env.DB, body.user_id);
  // Fall back to the user's active voice clone when the caller doesn't pin one
  // explicitly — avoids ghost "Voice Setup" screens on /session/:id/setup when
  // onboarding already enrolled a clone.
  const voiceId = body.voice_id ?? user.active_voice_id ?? null;

  // Strict enrollment-vs-session source_lang guard. The cloned voice can only
  // be cross-lingually steered by ElevenLabs' `language_code` on the TARGET
  // side — the enrollment language must still match what the host actually
  // speaks, otherwise the clone synthesizes with the wrong accent (Apr 2026
  // "Indian accent" regression). We enforce mismatch BEFORE any platform
  // provisioning or D1 writes so no side effects occur on reject.
  //
  // Edge cases:
  //   voice == null                   → allow (host skipped the clone; default
  //                                     voice has no enrollment lang)
  //   voice.source_lang == null       → reject (legacy pre-migration 0006 row,
  //                                     force re-record with a known lang)
  if (voiceId) {
    const voice = await db.getVoice(c.env.DB, voiceId);
    if (voice && voice.source_lang !== body.source_lang) {
      return c.json(
        {
          error: "voice_language_mismatch",
          voice_source_lang: voice.source_lang,
          session_source_lang: body.source_lang,
        },
        400,
      );
    }
  }

  const session = await db.createSession(c.env.DB, {
    userId: body.user_id,
    voiceId,
    title: body.title,
    sourceLang: body.source_lang,
    targetLangs: JSON.stringify(body.target_langs),
  });

  const privacyStatus = body.privacy_status ?? "unlisted";

  // Build an in-memory list first so we can roll back cleanly if any YouTube
  // broadcast create fails mid-way. A session with some streams inserted and
  // others failed would confuse the FE ("waiting for utterances forever" on a
  // stream that has a NULL rtmp_url). All-or-nothing is simpler.
  type Prepared =
    | {
        kind: "manual";
        lang: string;
        platform: string;
        rtmpUrl: string;
        streamKey: string;
        delayMs: number;
        hostGain: number;
      }
    | {
        kind: "youtube";
        lang: string;
        platform: string;
        delayMs: number;
        hostGain: number;
      }
    | {
        kind: "grip";
        lang: string;
        platform: string;
        productId: string;
        delayMs: number;
        hostGain: number;
      };
  const prepared: Prepared[] = [];
  for (const p of body.platforms ?? []) {
    if (!p.lang) continue;
    const delayMs = p.delay_ms ?? DEFAULT_DELAY_MS;
    const hostGain = resolveHostGain(p, body.source_lang);
    // YouTube destinations are auto-fillable even when the FE didn't populate
    // rtmp_url/stream_key — Workers will resolve them via YouTube API below.
    if (p.platform === "youtube" && user.youtube_access_token) {
      prepared.push({
        kind: "youtube",
        lang: p.lang,
        platform: p.platform,
        delayMs,
        hostGain,
      });
      continue;
    }
    // Grip destinations: when Seller API keys are present AND the caller
    // supplied a product_id, auto-provision fresh RTMP creds. Otherwise
    // fall through to the manual paste-creds path — the FE ships the pasted
    // rtmp_url + stream_key inline with the session. We no longer persist
    // Grip creds (see POST /auth/grip → 410): keys are one-shot per
    // broadcast and reusing a saved key silently breaks the next session.
    if (
      p.platform === "grip" &&
      p.product_id &&
      c.env.GRIP_ACCESS_KEY &&
      c.env.GRIP_SECRET_KEY
    ) {
      prepared.push({
        kind: "grip",
        lang: p.lang,
        platform: p.platform,
        productId: p.product_id,
        delayMs,
        hostGain,
      });
      continue;
    }
    if (!p.rtmp_url || !p.stream_key) continue;
    prepared.push({
      kind: "manual",
      lang: p.lang,
      platform: p.platform,
      rtmpUrl: p.rtmp_url,
      streamKey: p.stream_key,
      delayMs,
      hostGain,
    });
  }

  // Insert pending/manual rows first so we always have a stream id to
  // associate with the YouTube broadcast. Track insertion order so we can
  // cleanup on failure without re-querying.
  const insertedIds: string[] = [];
  const streams = [];
  try {
    for (const p of prepared) {
      if (p.kind === "manual") {
        const row = await db.createStreamManual(c.env.DB, {
          sessionId: session.id,
          lang: p.lang,
          platform: p.platform,
          rtmpUrl: p.rtmpUrl,
          streamKey: p.streamKey,
          delayMs: p.delayMs,
          hostGain: p.hostGain,
        });
        insertedIds.push(row.id);
        streams.push(row);
        continue;
      }

      if (p.kind === "grip") {
        // Grip Seller-API auto-provision path. Mirrors the YouTube branch —
        // insert a pending row first so we have a stream id, then call Grip,
        // then patch the row with the fresh RTMP url + stream key.
        const pending = await db.createStreamManual(c.env.DB, {
          sessionId: session.id,
          lang: p.lang,
          platform: p.platform,
          rtmpUrl: null,
          streamKey: null,
          delayMs: p.delayMs,
          hostGain: p.hostGain,
        });
        insertedIds.push(pending.id);

        let gripResult: GripBroadcastResult;
        try {
          gripResult = await provisionGripBroadcast(c.env, {
            accessKey: c.env.GRIP_ACCESS_KEY!,
            secretKey: c.env.GRIP_SECRET_KEY!,
            productId: p.productId,
            title: body.title,
          });
        } catch (e) {
          // Roll back like the YouTube failure branch. Surface the Grip API
          // status back to the caller so the FE can either retry or prompt
          // the user for paste-creds (Task B fallback). §0.5.4: log the
          // underlying error BEFORE rollback so a post-incident grep for
          // "grip seller" or the caller's session_id surfaces the real
          // cause (the return-to-caller JSON is lossy).
          console.warn(
            `[sessions] grip seller api provision failed — rolling back session ${session.id}`,
            { error: String(e), productId: p.productId },
          );
          for (const id of insertedIds) {
            await db.deleteStreamRow(c.env.DB, id);
          }
          await db.deleteSessionRow(c.env.DB, session.id);
          const status = e instanceof GripSellerApiError ? e.status : 500;
          const message = e instanceof Error ? e.message : String(e);
          return c.json(
            { error: `Grip Seller API provision failed: ${message}` },
            status === 401 || status === 403 ? status : 502,
          );
        }

        await db.updateStreamRtmp(c.env.DB, {
          streamId: pending.id,
          rtmpUrl: gripResult.rtmpUrl,
          streamKey: gripResult.streamKey,
          platformBroadcastId: gripResult.broadcastId,
          platformStreamId: null,
        });

        streams.push({
          ...pending,
          rtmp_url: gripResult.rtmpUrl,
          stream_key: gripResult.streamKey,
          platform_broadcast_id: gripResult.broadcastId,
          platform_stream_id: null,
          status: "ready",
        });
        continue;
      }

      // YouTube auto-fill path.
      const pending = await db.createStreamManual(c.env.DB, {
        sessionId: session.id,
        lang: p.lang,
        platform: p.platform,
        rtmpUrl: null,
        streamKey: null,
        delayMs: p.delayMs,
        hostGain: p.hostGain,
      });
      insertedIds.push(pending.id);

      let result: YouTubeBroadcastResult;
      try {
        result = await createYouTubeBroadcast(
          c.env,
          {
            accessToken: user.youtube_access_token!,
            title: body.title,
            scheduledStartTime: new Date().toISOString(),
            privacyStatus,
          },
          {
            refreshToken: user.youtube_refresh_token,
            onTokenRefresh: async (newToken, expiresAt) => {
              await db.updateAccessToken(c.env.DB, {
                userId: user.id,
                accessToken: newToken,
                expiresAt,
              });
            },
          },
        );
      } catch (e) {
        // Roll back the session + all previously inserted stream rows so the
        // FE never sees a half-baked session with orphan streams. Surface the
        // error to the caller — the dashboard renders the message inline.
        // §0.5.4: log the underlying error so a post-incident grep for
        // "youtube broadcast" or the session_id surfaces the real cause.
        console.warn(
          `[sessions] youtube broadcast create failed — rolling back session ${session.id}`,
          { error: String(e) },
        );
        for (const id of insertedIds) {
          await db.deleteStreamRow(c.env.DB, id);
        }
        await db.deleteSessionRow(c.env.DB, session.id);
        const status = e instanceof YouTubeBroadcastError ? e.status : 500;
        const message = e instanceof Error ? e.message : String(e);
        return c.json(
          { error: `YouTube broadcast create failed: ${message}` },
          status === 401 || status === 403 ? status : 502,
        );
      }

      await db.updateStreamRtmp(c.env.DB, {
        streamId: pending.id,
        rtmpUrl: result.rtmpUrl,
        streamKey: result.streamKey,
        platformBroadcastId: result.broadcastId,
        platformStreamId: result.streamId,
      });

      streams.push({
        ...pending,
        rtmp_url: result.rtmpUrl,
        stream_key: result.streamKey,
        platform_broadcast_id: result.broadcastId,
        platform_stream_id: result.streamId,
        status: "ready",
        watch_url: result.watchUrl,
      });
    }
  } catch (e) {
    // Defensive — we already return early on the YouTube failure branch, so
    // this catches e.g. D1 transient errors during manual inserts.
    // §0.5.4: log so a post-incident grep for "session create rollback"
    // surfaces the underlying cause; re-throwing alone loses context.
    console.warn(
      `[sessions] defensive rollback after session-create exception — session ${session.id}`,
      { error: String(e) },
    );
    for (const id of insertedIds) {
      await db.deleteStreamRow(c.env.DB, id);
    }
    await db.deleteSessionRow(c.env.DB, session.id);
    throw e;
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
  await db.updateSessionVoicePreset(c.env.DB, params.id, "cloned");
  return c.json({ voice });
});

app.patch("/api/sessions/:id/voice-preset", async (c) => {
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;
  const body = parseWithSchema(c, UpdateSessionVoicePresetSchema, await c.req.json());
  if (body instanceof Response) return body;

  const session = await db.getSession(c.env.DB, params.id);
  if (!session) return c.json({ error: "not found" }, 404);

  // 'cloned' requires the session already point at a voice row. Reject
  // otherwise — we don't want to silently pick a default when the host
  // thought they were using their clone.
  if (body.voice_preset === "cloned" && !session.voice_id) {
    return c.json({ error: "no cloned voice attached to session" }, 400);
  }

  await db.updateSessionVoicePreset(c.env.DB, params.id, body.voice_preset);
  const updated = await db.getSession(c.env.DB, params.id);
  return c.json({ session: updated });
});

app.delete("/api/sessions/:id", async (c) => {
  const params = parseWithSchema(c, SessionIdParamsSchema, {
    id: c.req.param("id"),
  });
  if (params instanceof Response) return params;
  // Soft-end: do NOT drop the row. The summary modal + post-stream view both
  // need the session and its metrics to render; hard-deleting them produced
  // the "Session not found" screen on End-Session and left server-rs
  // PATCH-spamming /metrics on a dead row (D1 SQLITE_BUSY storms followed).
  // Workers is also the single source of truth for "session is over" — once
  // status='ended' lands, server-rs's metrics reporter self-cancels on the
  // next 404 (it can race with this handler so we still allow that path).
  const existing = await db.getSession(c.env.DB, params.id);
  if (!existing) return c.json({ status: "ended" });
  await db.updateSessionStatus(c.env.DB, {
    id: params.id,
    status: "ended",
    liveSessionId: null,
  });
  return c.json({ status: "ended" });
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
    if (!Array.isArray(parsed)) {
      console.warn(
        "parseTargetLangs: stored target_langs is not a JSON array — treating as empty",
        { rawLen: raw.length, kind: typeof parsed },
      );
      return [];
    }
    return parsed.filter((v) => typeof v === "string");
  } catch (error) {
    console.warn(
      "parseTargetLangs: JSON.parse failed on stored target_langs — treating as empty; quote + summary will report zero langs",
      { rawLen: raw.length, error: (error as Error).message },
    );
    return [];
  }
}

// Pre-stream cost projection. `output_minutes = expected_minutes × target_lang_count`
// because we produce one translated stream per target language. Used by the
// FE "you'll spend ~$X" banner on session start.
//
// Rate decision: flat PER_OUTPUT_MINUTE_USD for all users — pricing is
// voice-agnostic today, so a missing active_voice_id does NOT affect the
// quote. `estimated_cost_usd` is therefore always a finite, 2-decimal number
// (never null/undefined) even for freshly-signed-up users with no clone yet.
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

  const targetLangs = parseTargetLangs(session.target_langs);
  // Passthrough (wire code "pass" or target==source) produces no TTS minutes
  // and therefore no billable output — keep it out of the breakdown so the
  // FE line-items match the aggregate.
  const billableLangs = targetLangs.filter(
    (lang) => lang !== "pass" && lang !== session.source_lang,
  );
  const perMinuteCost = Math.round(expectedMinutes * PER_OUTPUT_MINUTE_USD * 100) / 100;
  const breakdown = billableLangs.map((lang) => ({
    lang,
    minutes: expectedMinutes,
    cost_usd: Number.isFinite(perMinuteCost) ? perMinuteCost : 0,
  }));
  const outputMinutes = Math.round(expectedMinutes * billableLangs.length * 100) / 100;
  const rawCost = outputMinutes * PER_OUTPUT_MINUTE_USD;
  // Defensive: if anything upstream returns NaN/Infinity we still respond
  // with a number — the FE renderer must never see null for this field.
  const estimatedCostUsd = Number.isFinite(rawCost)
    ? Math.round(rawCost * 100) / 100
    : 0;

  return c.json({
    session_id: params.id,
    expected_minutes: expectedMinutes,
    output_minutes: outputMinutes,
    per_output_minute_usd: PER_OUTPUT_MINUTE_USD,
    estimated_cost_usd: estimatedCostUsd,
    breakdown,
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

  const owner = await db.getUserById(c.env.DB, session.user_id);

  const metrics = await db.getSessionMetrics(c.env.DB, params.id);
  const outputByLangSeconds: Record<string, number> = metrics
    ? (JSON.parse(metrics.output_seconds_json) as Record<string, number>)
    : {};
  const outputByLang: Record<string, number> = {};
  for (const [k, v] of Object.entries(outputByLangSeconds)) {
    outputByLang[k] = minutesFromSeconds(v);
  }
  const totalMinutes =
    Math.round(
      Object.values(outputByLang).reduce((a, b) => a + b, 0) * 100,
    ) / 100;

  const liveStatuses = new Set(["setup", "live"]);
  // Tier resolves from the session owner. Pre-migration rows default to
  // 'self_serve' via schema; an owner row may be missing in tests that seed
  // only sessions, so fall through safely.
  const billingTier: "self_serve" | "b2b" =
    owner?.billing_tier === "b2b" ? "b2b" : "self_serve";
  const isB2B = billingTier === "b2b";

  return c.json({
    session_id: params.id,
    status: session.status,
    is_final: !liveStatuses.has(session.status),
    billing_tier: billingTier,
    source_minutes: minutesFromSeconds(metrics?.source_seconds ?? 0),
    total_minutes: totalMinutes,
    output_by_lang: outputByLang,
    // B2B invoices are reconciled off-platform — omit the self-serve price
    // so the FE can't accidentally render a dollar figure to an invoiced
    // customer.
    total_cost_usd: isB2B ? null : estimateCost(outputByLang),
    rate_usd: isB2B ? null : PER_OUTPUT_MINUTE_USD,
    billed_to: isB2B ? owner?.bills_to ?? null : null,
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
    // §0.5.4: OAuth callback errors otherwise only surface as a 500 JSON
    // body to the FE; log so a post-incident grep catches token-exchange
    // failures, channel-info 403s, etc.
    console.warn("[auth] youtube callback exception", { error: String(e) });
    return c.json({ error: String(e) }, 500);
  }
});

// ── OAuth (Google sign-in) ───────────────────────────────
// Separate Google Client redirect entry. Uses openid+email+profile scope,
// identifies users by the Google `sub` (never user-supplied), and mints
// the same JWT shape /auth/token returns so the FE path after sign-in is
// unchanged.

app.get("/auth/google", async (c) => {
  // Dev-only bypass: skip the real Google round-trip, mint a JWT for a
  // fixed local user, and redirect back to the FE as if OAuth completed.
  // Guarded by a strict "true" string check so only `.dev.vars` (never
  // wrangler.toml [vars]) can trip it. Prod Workers never set this var,
  // so this branch is a compile-time no-op there.
  if (c.env.DEV_AUTH_BYPASS === "true") {
    const DEV_USER_ID = "dev-user";
    const user = await db.getOrCreateUser(c.env.DB, DEV_USER_ID);
    await db.updateUserProfile(c.env.DB, {
      userId: user.id,
      email: "dev@brivva.local",
      name: "Dev User",
      picture: null,
    });
    const jwt = await signJwt(c.env.JWT_SECRET, { sub: user.id });
    console.warn("[auth] DEV_AUTH_BYPASS active — skipping Google OAuth", {
      user_id: user.id,
    });
    return c.redirect(
      `${c.env.FRONTEND_URL}/?user_id=${encodeURIComponent(user.id)}#token=${jwt}`,
    );
  }
  // State is an opaque CSRF token; for sign-in we don't have a user_id yet.
  const state = crypto.randomUUID();
  return c.redirect(gsignin.authorizeUrl(c.env, state));
});

// Test-only endpoint that hard-resets the dev-user D1 state so the
// Playwright e2e at `frontend/e2e/full-stack-live.e2e.ts` can run
// idempotently. Gated by DEV_AUTH_BYPASS — prod Workers have the var
// unset, so the route 404s.
app.post("/test/reset-dev-user", async (c) => {
  if (c.env.DEV_AUTH_BYPASS !== "true") {
    return c.json({ error: "not found" }, 404);
  }
  const DEV_USER_ID = "dev-user";
  // Delete children before parents to satisfy FK references. session_metrics
  // rows are keyed by session_id, so they go with sessions.
  await c.env.DB.prepare(
    "DELETE FROM session_metrics WHERE session_id IN (SELECT id FROM sessions WHERE user_id = ?)",
  )
    .bind(DEV_USER_ID)
    .run();
  await c.env.DB.prepare(
    "DELETE FROM streams WHERE session_id IN (SELECT id FROM sessions WHERE user_id = ?)",
  )
    .bind(DEV_USER_ID)
    .run();
  await c.env.DB.prepare("DELETE FROM sessions WHERE user_id = ?")
    .bind(DEV_USER_ID)
    .run();
  await c.env.DB.prepare("DELETE FROM voices WHERE user_id = ?")
    .bind(DEV_USER_ID)
    .run();
  await c.env.DB.prepare("DELETE FROM platform_credentials WHERE user_id = ?")
    .bind(DEV_USER_ID)
    .run();
  await c.env.DB.prepare("DELETE FROM users WHERE id = ?")
    .bind(DEV_USER_ID)
    .run();
  console.warn("[test] DEV_AUTH_BYPASS reset: wiped dev-user D1 state");
  return c.json({ ok: true });
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
    // §0.5.4: same rationale as /auth/youtube/callback — surface the
    // underlying cause so sign-in failures are grep-auditable.
    console.warn("[auth] google sign-in callback exception", { error: String(e) });
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

// ── Platform credential paste flows (TikTok) ────────────
// TikTok doesn't expose real OAuth for live streaming — the creator pastes
// a session token + stream key from their host dashboard. The session
// token is stored as display_name metadata; the stream key + RTMP URL are
// what Fargate actually pushes to. TikTok stream keys are reusable until
// the host regenerates them, so persisting them is safe. Grip also paste-
// creds, but its keys are one-shot so /auth/grip returns 410 — see below.

// Grip stream keys are one-shot per broadcast (AWS IVS under the hood — a
// fresh key issues ~1 hour before each scheduled show, and IVS rejects a
// second publisher with the same key). Saving + pre-filling guaranteed the
// failure mode where the second session silently died after ~25s. The FE
// now pastes fresh each session; the server refuses to persist creds so a
// legacy client can't reintroduce the bug.
app.post("/auth/grip", (c) => {
  return c.json(
    {
      error: "grip_creds_not_savable",
      message:
        "Grip stream keys are one-shot per broadcast. Paste them fresh on each session.",
    },
    410,
  );
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
  // Voice join moved from session.voice_id → users.active_voice_id. Rationale:
  // the one-voice-per-user invariant means re-record must refresh dispatch
  // mid-session without creating a new session row, and the session row's
  // voice_id can lag the actual active voice after an upsert. Reading the
  // owner's active_voice_id keeps Fargate dispatch in sync with POST
  // /api/voices upserts on every bundle fetch.
  const owner = await db.getUserById(c.env.DB, session.user_id);
  const activeVoiceId = owner?.active_voice_id ?? null;
  const voice = activeVoiceId ? await db.getVoice(c.env.DB, activeVoiceId) : null;
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
