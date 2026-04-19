// Drizzle-backed D1 helpers. One function per verb that the FE + Fargate
// internal clients already call. Handlers accept a raw `D1Database` so
// the call sites stay a one-liner (`db.listVoices(c.env.DB, userId)`); the
// tiny `drizzle(...)` wrap is cheap — it's just a typed proxy around the
// same prepared-statement API underneath.

import { and, desc, eq, sql } from "drizzle-orm";
import { drizzle } from "drizzle-orm/d1";

import * as schema from "../../core/schema";
import type {
  PlatformCredential,
  Session,
  SessionMetrics,
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

export async function createVoice(db: D1Database, args: CreateVoice): Promise<Voice> {
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
  await wrap(db).delete(schema.voices).where(eq(schema.voices.id, voiceId)).run();
}

// ── sessions ──────────────────────────────────────────────

export type CreateSession = {
  userId: string;
  voiceId: string | null;
  title: string;
  sourceLang: string;
  targetLangs: string;
};

export async function createSession(db: D1Database, args: CreateSession): Promise<Session> {
  const row: Session = {
    id: uuid(),
    user_id: args.userId,
    voice_id: args.voiceId,
    title: args.title,
    source_lang: args.sourceLang,
    target_langs: args.targetLangs,
    status: "setup",
    live_session_id: null,
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

export async function deleteSessionRow(
  db: D1Database,
  id: string,
): Promise<void> {
  const d = wrap(db);
  await d.delete(schema.session_metrics).where(eq(schema.session_metrics.session_id, id)).run();
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
  rtmpUrl: string;
  streamKey: string;
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
    status: "ready",
    delay_ms: args.delayMs,
    host_gain: args.hostGain,
    created_at: now(),
  };
  await wrap(db).insert(schema.streams).values(row).run();
  return row;
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

  const nextSource =
    args.sourceSeconds ?? prior?.source_seconds ?? 0;

  const priorOutputs: Record<string, number> = prior
    ? (JSON.parse(prior.output_seconds_json) as Record<string, number>)
    : {};
  const mergedOutputs = { ...priorOutputs, ...(args.outputSecondsByLang ?? {}) };

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
