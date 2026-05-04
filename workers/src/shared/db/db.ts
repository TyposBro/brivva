// Drizzle-backed D1 helpers. One function per verb that the FE + Fargate
// internal clients already call. Handlers accept a raw `D1Database` so
// the call sites stay a one-liner (`db.listVoices(c.env.DB, userId)`); the
// tiny `drizzle(...)` wrap is cheap — it's just a typed proxy around the
// same prepared-statement API underneath.

import { and, desc, eq, inArray, sql } from "drizzle-orm";
import { drizzle } from "drizzle-orm/d1";

import * as schema from "../../core/schema";
export type { SessionProviderFailure } from "../../core/schema";
import type {
  PlatformCredential,
  Session,
  SessionLogEvent,
  SessionMetrics,
  SessionProviderFailure,
  StreamRecord,
  User,
  Voice,
} from "../../core/schema";

const now = () => Math.floor(Date.now() / 1000);
const uuid = () => crypto.randomUUID();

function wrap(db: D1Database) {
  return drizzle(db, { schema });
}

// ── users ─────────────────────────────────────────────────

export async function getOrCreateUser(
  db: D1Database,
  id: string,
): Promise<User> {
  const d = wrap(db);
  await d
    .insert(schema.users)
    .values({ id, created_at: now() })
    .onConflictDoNothing()
    .run();
  const row = await d.query.users.findFirst({
    where: eq(schema.users.id, id),
  });
  if (!row) throw new Error("user upsert failed");
  return row;
}

export async function getUserById(
  db: D1Database,
  id: string,
): Promise<User | null> {
  const row = await wrap(db).query.users.findFirst({
    where: eq(schema.users.id, id),
  });
  return row ?? null;
}

export type UpdateYouTubeTokens = {
  userId: string;
  accessToken: string;
  refreshToken: string;
  expiresAt: number;
  channelId: string;
  channelName: string;
};

export async function updateYouTubeTokens(
  db: D1Database,
  args: UpdateYouTubeTokens,
): Promise<void> {
  await wrap(db)
    .update(schema.users)
    .set({
      youtube_access_token: args.accessToken,
      youtube_refresh_token: args.refreshToken,
      youtube_token_expires_at: args.expiresAt,
      youtube_channel_id: args.channelId,
      youtube_channel_name: args.channelName,
    })
    .where(eq(schema.users.id, args.userId))
    .run();
}

export type UpdateAccessToken = {
  userId: string;
  accessToken: string;
  expiresAt: number;
};

export async function updateAccessToken(
  db: D1Database,
  args: UpdateAccessToken,
): Promise<void> {
  await wrap(db)
    .update(schema.users)
    .set({
      youtube_access_token: args.accessToken,
      youtube_token_expires_at: args.expiresAt,
    })
    .where(eq(schema.users.id, args.userId))
    .run();
}

export type UpdateUserProfile = {
  userId: string;
  email: string | null;
  name: string | null;
  picture: string | null;
};

export async function updateUserProfile(
  db: D1Database,
  args: UpdateUserProfile,
): Promise<void> {
  await wrap(db)
    .update(schema.users)
    .set({ email: args.email, name: args.name, picture: args.picture })
    .where(eq(schema.users.id, args.userId))
    .run();
}

export async function markOnboardingCompleted(
  db: D1Database,
  userId: string,
): Promise<User> {
  await wrap(db)
    .update(schema.users)
    .set({ onboarding_completed_at: now() })
    .where(eq(schema.users.id, userId))
    .run();
  return await getOrCreateUser(db, userId);
}

export async function setActiveVoice(
  db: D1Database,
  userId: string,
  voiceId: string | null,
): Promise<void> {
  await wrap(db)
    .update(schema.users)
    .set({ active_voice_id: voiceId })
    .where(eq(schema.users.id, userId))
    .run();
}

// ── voices ────────────────────────────────────────────────

export async function listVoices(
  db: D1Database,
  userId: string,
): Promise<Voice[]> {
  return await wrap(db).query.voices.findMany({
    where: eq(schema.voices.user_id, userId),
    orderBy: desc(schema.voices.created_at),
  });
}

export async function getVoice(
  db: D1Database,
  voiceId: string,
): Promise<Voice | null> {
  const row = await wrap(db).query.voices.findFirst({
    where: eq(schema.voices.id, voiceId),
  });
  return row ?? null;
}

export type CreateVoice = {
  userId: string;
  elevenlabsVoiceId: string;
  name: string;
  sourceLang: string | null;
};

export async function createVoice(
  db: D1Database,
  args: CreateVoice,
): Promise<Voice> {
  const row: Voice = {
    id: uuid(),
    user_id: args.userId,
    elevenlabs_voice_id: args.elevenlabsVoiceId,
    name: args.name,
    source_lang: args.sourceLang,
    created_at: now(),
  };
  await wrap(db).insert(schema.voices).values(row).run();
  return row;
}

export async function deleteVoiceRow(
  db: D1Database,
  voiceId: string,
): Promise<void> {
  const d = wrap(db);
  // Detach any historical (ended) sessions before dropping the voice row.
  // FK enforcement in D1 would otherwise reject the DELETE. Setup/live
  // sessions are guarded at the handler, so anything surviving here is
  // archival and safe to null-detach.
  await d
    .update(schema.sessions)
    .set({ voice_id: null })
    .where(eq(schema.sessions.voice_id, voiceId))
    .run();
  await d.delete(schema.voices).where(eq(schema.voices.id, voiceId)).run();
}

// Active == setup-or-live: a session in setup is mid-preflight and
// about to ffmpeg-spawn, so yanking its voice still breaks the pipeline.
const ACTIVE_SESSION_STATUSES = ["setup", "live"] as const;

export async function countActiveSessionsForVoice(
  db: D1Database,
  voiceId: string,
): Promise<number> {
  const row = await wrap(db)
    .select({ n: sql<number>`count(*)` })
    .from(schema.sessions)
    .where(
      and(
        eq(schema.sessions.voice_id, voiceId),
        inArray(schema.sessions.status, [...ACTIVE_SESSION_STATUSES]),
      ),
    )
    .get();
  return Number(row?.n ?? 0);
}

export async function countActiveSessionsForUser(
  db: D1Database,
  userId: string,
): Promise<number> {
  const row = await wrap(db)
    .select({ n: sql<number>`count(*)` })
    .from(schema.sessions)
    .where(
      and(
        eq(schema.sessions.user_id, userId),
        inArray(schema.sessions.status, [...ACTIVE_SESSION_STATUSES]),
      ),
    )
    .get();
  return Number(row?.n ?? 0);
}

// Hard-delete everything we own for a user. Children-first cascade
// mirrors /test/reset-dev-user. Callers MUST have already guarded
// against active sessions and fanned out ElevenLabs voice deletion —
// this function only touches D1.
export async function hardDeleteUserCascade(
  db: D1Database,
  userId: string,
): Promise<void> {
  const d = wrap(db);
  await d
    .delete(schema.session_log_events)
    .where(
      inArray(
        schema.session_log_events.session_id,
        d
          .select({ id: schema.sessions.id })
          .from(schema.sessions)
          .where(eq(schema.sessions.user_id, userId)),
      ),
    )
    .run();
  await d
    .delete(schema.session_provider_failures)
    .where(
      inArray(
        schema.session_provider_failures.session_id,
        d
          .select({ id: schema.sessions.id })
          .from(schema.sessions)
          .where(eq(schema.sessions.user_id, userId)),
      ),
    )
    .run();
  await d
    .delete(schema.session_metrics)
    .where(
      inArray(
        schema.session_metrics.session_id,
        d
          .select({ id: schema.sessions.id })
          .from(schema.sessions)
          .where(eq(schema.sessions.user_id, userId)),
      ),
    )
    .run();
  await d
    .delete(schema.streams)
    .where(
      inArray(
        schema.streams.session_id,
        d
          .select({ id: schema.sessions.id })
          .from(schema.sessions)
          .where(eq(schema.sessions.user_id, userId)),
      ),
    )
    .run();
  await d
    .delete(schema.sessions)
    .where(eq(schema.sessions.user_id, userId))
    .run();
  await d.delete(schema.voices).where(eq(schema.voices.user_id, userId)).run();
  await d
    .delete(schema.platform_credentials)
    .where(eq(schema.platform_credentials.user_id, userId))
    .run();
  await d.delete(schema.users).where(eq(schema.users.id, userId)).run();
}

// ── sessions ──────────────────────────────────────────────

export type CreateSession = {
  userId: string;
  voiceId: string | null;
  title: string;
  sourceLang: string;
  targetLangs: string;
  translationTerms?: string | null;
};

export async function createSession(
  db: D1Database,
  args: CreateSession,
): Promise<Session> {
  // Default new sessions with an attached clone to voice_preset='cloned' so
  // returning users don't have to re-pick each time. No clone → 'female'
  // (matches the historical Lang::voice_id() default the schema migration
  // pinned).
  const row: Session = {
    id: uuid(),
    user_id: args.userId,
    voice_id: args.voiceId,
    title: args.title,
    source_lang: args.sourceLang,
    target_langs: args.targetLangs,
    status: "setup",
    live_session_id: null,
    voice_preset: args.voiceId ? "cloned" : "female",
    translation_terms: args.translationTerms ?? null,
    created_at: now(),
  };
  await wrap(db).insert(schema.sessions).values(row).run();
  return row;
}

export async function listSessions(
  db: D1Database,
  userId: string,
): Promise<Session[]> {
  return await wrap(db).query.sessions.findMany({
    where: eq(schema.sessions.user_id, userId),
    orderBy: desc(schema.sessions.created_at),
  });
}

export async function getSession(
  db: D1Database,
  id: string,
): Promise<Session | null> {
  const row = await wrap(db).query.sessions.findFirst({
    where: eq(schema.sessions.id, id),
  });
  return row ?? null;
}

export type UpdateSessionStatus = {
  id: string;
  status: string;
  liveSessionId: string | null;
};

export async function updateSessionStatus(
  db: D1Database,
  args: UpdateSessionStatus,
): Promise<void> {
  await wrap(db)
    .update(schema.sessions)
    .set({ status: args.status, live_session_id: args.liveSessionId })
    .where(eq(schema.sessions.id, args.id))
    .run();
}

export async function updateSessionVoiceId(
  db: D1Database,
  id: string,
  voiceId: string | null,
): Promise<void> {
  await wrap(db)
    .update(schema.sessions)
    .set({ voice_id: voiceId })
    .where(eq(schema.sessions.id, id))
    .run();
}

export async function updateSessionVoicePreset(
  db: D1Database,
  id: string,
  preset: "cloned" | "female" | "male",
): Promise<void> {
  await wrap(db)
    .update(schema.sessions)
    .set({ voice_preset: preset })
    .where(eq(schema.sessions.id, id))
    .run();
}

export async function deleteSessionRow(
  db: D1Database,
  id: string,
): Promise<void> {
  const d = wrap(db);
  await d
    .delete(schema.session_log_events)
    .where(eq(schema.session_log_events.session_id, id))
    .run();
  await d
    .delete(schema.session_provider_failures)
    .where(eq(schema.session_provider_failures.session_id, id))
    .run();
  await d
    .delete(schema.session_metrics)
    .where(eq(schema.session_metrics.session_id, id))
    .run();
  await d.delete(schema.streams).where(eq(schema.streams.session_id, id)).run();
  await d.delete(schema.sessions).where(eq(schema.sessions.id, id)).run();
}

// ── streams ───────────────────────────────────────────────

export async function listStreams(
  db: D1Database,
  sessionId: string,
): Promise<StreamRecord[]> {
  return await wrap(db).query.streams.findMany({
    where: eq(schema.streams.session_id, sessionId),
    orderBy: [schema.streams.platform, schema.streams.lang],
  });
}

export type CreateStreamManual = {
  sessionId: string;
  lang: string;
  platform: string;
  /** Nullable for auto-fill platforms (e.g. YouTube) where Workers populates
   *  RTMP ingest after the row exists via `updateStreamRtmp`. */
  rtmpUrl: string | null;
  streamKey: string | null;
  delayMs: number;
  hostGain: number;
};

export async function createStreamManual(
  db: D1Database,
  args: CreateStreamManual,
): Promise<StreamRecord> {
  const row: StreamRecord = {
    id: uuid(),
    session_id: args.sessionId,
    lang: args.lang,
    platform: args.platform,
    platform_broadcast_id: null,
    platform_stream_id: null,
    stream_key: args.streamKey,
    rtmp_url: args.rtmpUrl,
    // "pending" = auto-fill platforms whose RTMP hasn't been populated yet.
    // Fargate ignores these rows in session_ws's start_rtmp_streams because
    // rtmp_url is None; the auto-fill path flips this to "ready" after it
    // calls the platform API.
    status: args.rtmpUrl && args.streamKey ? "ready" : "pending",
    delay_ms: args.delayMs,
    host_gain: args.hostGain,
    created_at: now(),
  };
  const d = wrap(db);
  // Concurrent POST /api/sessions/:id/streams with the same
  // (session_id, lang, platform) must NOT double-insert. Migration 0010
  // adds the unique index that enforces it; the handler uses ON CONFLICT
  // DO NOTHING and re-selects so both callers see the canonical row. The
  // "winner" is whichever insert hit D1 first.
  await d.insert(schema.streams).values(row).onConflictDoNothing().run();
  const existing = await d.query.streams.findFirst({
    where: and(
      eq(schema.streams.session_id, args.sessionId),
      eq(schema.streams.lang, args.lang),
      eq(schema.streams.platform, args.platform),
    ),
  });
  if (!existing)
    throw new Error("createStreamManual: row missing after insert");
  return existing;
}

export type UpdateStreamPlatform = {
  streamId: string;
  broadcastId: string;
  platformStreamId: string;
  streamKey: string;
  rtmpUrl: string;
};

export async function updateStreamPlatform(
  db: D1Database,
  args: UpdateStreamPlatform,
): Promise<void> {
  await wrap(db)
    .update(schema.streams)
    .set({
      platform_broadcast_id: args.broadcastId,
      platform_stream_id: args.platformStreamId,
      stream_key: args.streamKey,
      rtmp_url: args.rtmpUrl,
      status: "ready",
    })
    .where(eq(schema.streams.id, args.streamId))
    .run();
}

/**
 * Populates the RTMP ingest + platform IDs on a stream row that was inserted
 * without them (the auto-fill path for platforms that expose a real API —
 * YouTube today, possibly others later). Mirrors `updateStreamPlatform`
 * semantically — kept as a separate export so the call site in
 * `POST /api/sessions` reads top-to-bottom as "create row, fill RTMP".
 */
export type UpdateStreamRtmp = {
  streamId: string;
  rtmpUrl: string;
  streamKey: string;
  platformBroadcastId: string;
  // Nullable to accommodate platforms (e.g. Grip) that don't expose a
  // separate "stream id" concept — YouTube issues distinct broadcast and
  // stream ids; Grip bundles both into one broadcast id.
  platformStreamId: string | null;
};

export async function updateStreamRtmp(
  db: D1Database,
  args: UpdateStreamRtmp,
): Promise<void> {
  await wrap(db)
    .update(schema.streams)
    .set({
      rtmp_url: args.rtmpUrl,
      stream_key: args.streamKey,
      platform_broadcast_id: args.platformBroadcastId,
      platform_stream_id: args.platformStreamId,
      status: "ready",
    })
    .where(eq(schema.streams.id, args.streamId))
    .run();
}

export async function deleteStreamRow(
  db: D1Database,
  streamId: string,
): Promise<void> {
  await wrap(db)
    .delete(schema.streams)
    .where(eq(schema.streams.id, streamId))
    .run();
}

// ── platform credentials ──────────────────────────────────

export async function listCredentials(
  db: D1Database,
  userId: string,
): Promise<PlatformCredential[]> {
  return await wrap(db).query.platform_credentials.findMany({
    where: eq(schema.platform_credentials.user_id, userId),
    orderBy: schema.platform_credentials.platform,
  });
}

export async function getCredential(
  db: D1Database,
  userId: string,
  platform: string,
): Promise<PlatformCredential | null> {
  const row = await wrap(db).query.platform_credentials.findFirst({
    where: and(
      eq(schema.platform_credentials.user_id, userId),
      eq(schema.platform_credentials.platform, platform),
    ),
  });
  return row ?? null;
}

export type UpsertCredential = {
  userId: string;
  platform: string;
  rtmpUrl: string | null;
  streamKey: string | null;
  displayName: string | null;
};

export async function upsertCredential(
  db: D1Database,
  args: UpsertCredential,
): Promise<PlatformCredential> {
  const d = wrap(db);
  const id = uuid();
  const ts = now();
  await d
    .insert(schema.platform_credentials)
    .values({
      id,
      user_id: args.userId,
      platform: args.platform,
      rtmp_url: args.rtmpUrl,
      stream_key: args.streamKey,
      display_name: args.displayName,
      created_at: ts,
      updated_at: ts,
    })
    .onConflictDoUpdate({
      target: [
        schema.platform_credentials.user_id,
        schema.platform_credentials.platform,
      ],
      set: {
        rtmp_url: args.rtmpUrl,
        stream_key: args.streamKey,
        // Preserve prior display_name when the new one is null (matches the
        // COALESCE behavior the hand-written SQL migration shipped with).
        display_name: sql`COALESCE(${args.displayName}, ${schema.platform_credentials.display_name})`,
        updated_at: ts,
      },
    })
    .run();

  const row = await getCredential(db, args.userId, args.platform);
  if (!row) throw new Error("credential upsert failed");
  return row;
}

export async function deleteCredentialRow(
  db: D1Database,
  userId: string,
  platform: string,
): Promise<void> {
  await wrap(db)
    .delete(schema.platform_credentials)
    .where(
      and(
        eq(schema.platform_credentials.user_id, userId),
        eq(schema.platform_credentials.platform, platform),
      ),
    )
    .run();
}

// ── session metrics ───────────────────────────────────────

export async function getSessionMetrics(
  db: D1Database,
  sessionId: string,
): Promise<SessionMetrics | null> {
  const row = await wrap(db).query.session_metrics.findFirst({
    where: eq(schema.session_metrics.session_id, sessionId),
  });
  return row ?? null;
}

export type UpsertSessionMetrics = {
  sessionId: string;
  sourceSeconds: number | undefined;
  outputSecondsByLang: Record<string, number> | undefined;
};

// Merge-semantic upsert: if the caller omits a field, prior values are kept.
// output_seconds_json is shallow-merged by lang so Fargate can PATCH a
// single target-language delta without clobbering the others.
export async function upsertSessionMetrics(
  db: D1Database,
  args: UpsertSessionMetrics,
): Promise<SessionMetrics> {
  const d = wrap(db);
  const prior = await getSessionMetrics(db, args.sessionId);
  const ts = now();

  const nextSource = args.sourceSeconds ?? prior?.source_seconds ?? 0;

  const priorOutputs: Record<string, number> = prior
    ? (JSON.parse(prior.output_seconds_json) as Record<string, number>)
    : {};
  const mergedOutputs = {
    ...priorOutputs,
    ...(args.outputSecondsByLang ?? {}),
  };

  const row: SessionMetrics = {
    session_id: args.sessionId,
    source_seconds: nextSource,
    output_seconds_json: JSON.stringify(mergedOutputs),
    updated_at: ts,
  };

  await d
    .insert(schema.session_metrics)
    .values(row)
    .onConflictDoUpdate({
      target: schema.session_metrics.session_id,
      set: {
        source_seconds: row.source_seconds,
        output_seconds_json: row.output_seconds_json,
        updated_at: row.updated_at,
      },
    })
    .run();

  return row;
}

export async function listUserSessionMetrics(
  db: D1Database,
  userId: string,
): Promise<SessionMetrics[]> {
  // Join-free: fetch session ids for the user, then metrics for those ids.
  const d = wrap(db);
  const userSessions = await d.query.sessions.findMany({
    where: eq(schema.sessions.user_id, userId),
    columns: { id: true },
  });
  if (userSessions.length === 0) return [];
  const ids = userSessions.map((s) => s.id);
  return await d.query.session_metrics.findMany({
    where: (m, { inArray }) => inArray(m.session_id, ids),
  });
}

// ── session logs ──────────────────────────────────────────

export type AppendSessionLogEvent = {
  sessionId: string;
  liveSessionId: string | null;
  source: string;
  level: string;
  event: string;
  message: string | null;
  fields: Record<string, unknown>;
  tsMs: number;
};

export async function appendSessionLogEvents(
  db: D1Database,
  events: AppendSessionLogEvent[],
): Promise<number> {
  if (events.length === 0) return 0;
  const createdAt = now();
  const rows: SessionLogEvent[] = events.map((event) => ({
    id: uuid(),
    session_id: event.sessionId,
    live_session_id: event.liveSessionId,
    source: event.source,
    level: event.level,
    event: event.event,
    message: event.message,
    fields_json: JSON.stringify(event.fields),
    ts_ms: Math.trunc(event.tsMs),
    created_at: createdAt,
  }));
  await wrap(db).insert(schema.session_log_events).values(rows).run();
  return rows.length;
}

export async function listSessionLogEvents(
  db: D1Database,
  sessionId: string,
  limit = 5000,
): Promise<SessionLogEvent[]> {
  return await wrap(db).query.session_log_events.findMany({
    where: eq(schema.session_log_events.session_id, sessionId),
    orderBy: schema.session_log_events.ts_ms,
    limit,
  });
}

// ── provider failures ─────────────────────────────────────

export type AppendProviderFailure = {
  sessionId: string;
  liveSessionId: string | null;
  outputId: string | null;
  provider: string;
  scope: string;
  state: string;
  reason: string;
  recoverable: boolean;
  billable: boolean;
  lang: string | null;
  platform: string | null;
  statusCode: number | null;
  errorCode: string | null;
  message: string | null;
  startedAtMs: number;
  recoveredAtMs: number | null;
};

export async function appendProviderFailure(
  db: D1Database,
  failure: AppendProviderFailure,
): Promise<SessionProviderFailure> {
  const row: SessionProviderFailure = {
    id: uuid(),
    session_id: failure.sessionId,
    live_session_id: failure.liveSessionId,
    output_id: failure.outputId,
    provider: failure.provider,
    scope: failure.scope,
    state: failure.state,
    reason: failure.reason,
    recoverable: failure.recoverable,
    billable: failure.billable,
    lang: failure.lang,
    platform: failure.platform,
    status_code: failure.statusCode,
    error_code: failure.errorCode,
    message: failure.message,
    started_at_ms: Math.trunc(failure.startedAtMs),
    recovered_at_ms:
      failure.recoveredAtMs == null ? null : Math.trunc(failure.recoveredAtMs),
    created_at: now(),
  };
  await wrap(db).insert(schema.session_provider_failures).values(row).run();
  return row;
}

export async function listProviderFailures(
  db: D1Database,
  sessionId: string,
): Promise<SessionProviderFailure[]> {
  return await wrap(db).query.session_provider_failures.findMany({
    where: eq(schema.session_provider_failures.session_id, sessionId),
    orderBy: schema.session_provider_failures.started_at_ms,
  });
}
