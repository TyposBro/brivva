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

export async function updateYouTubeTokens(
  db: D1Database,
  userId: string,
  accessToken: string,
  refreshToken: string,
  expiresAt: number,
  channelId: string,
  channelName: string,
): Promise<void> {
  await wrap(db)
    .update(schema.users)
    .set({
      youtube_access_token: accessToken,
      youtube_refresh_token: refreshToken,
      youtube_token_expires_at: expiresAt,
      youtube_channel_id: channelId,
      youtube_channel_name: channelName,
    })
    .where(eq(schema.users.id, userId))
    .run();
}

export async function updateAccessToken(
  db: D1Database,
  userId: string,
  accessToken: string,
  expiresAt: number,
): Promise<void> {
  await wrap(db)
    .update(schema.users)
    .set({
      youtube_access_token: accessToken,
      youtube_token_expires_at: expiresAt,
    })
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

export async function createVoice(
  db: D1Database,
  userId: string,
  elevenlabsVoiceId: string,
  name: string,
): Promise<Voice> {
  const row: Voice = {
    id: uuid(),
    user_id: userId,
    elevenlabs_voice_id: elevenlabsVoiceId,
    name,
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

export async function createSession(
  db: D1Database,
  userId: string,
  voiceId: string | null,
  title: string,
  sourceLang: string,
  targetLangs: string,
): Promise<Session> {
  const row: Session = {
    id: uuid(),
    user_id: userId,
    voice_id: voiceId,
    title,
    source_lang: sourceLang,
    target_langs: targetLangs,
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

export async function updateSessionStatus(
  db: D1Database,
  id: string,
  status: string,
  liveSessionId: string | null,
): Promise<void> {
  await wrap(db)
    .update(schema.sessions)
    .set({ status, live_session_id: liveSessionId })
    .where(eq(schema.sessions.id, id))
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

export async function createStreamManual(
  db: D1Database,
  sessionId: string,
  lang: string,
  platform: string,
  rtmpUrl: string,
  streamKey: string,
  delayMs: number,
  hostGain: number,
): Promise<StreamRecord> {
  const row: StreamRecord = {
    id: uuid(),
    session_id: sessionId,
    lang,
    platform,
    platform_broadcast_id: null,
    platform_stream_id: null,
    stream_key: streamKey,
    rtmp_url: rtmpUrl,
    status: "ready",
    delay_ms: delayMs,
    host_gain: hostGain,
    created_at: now(),
  };
  await wrap(db).insert(schema.streams).values(row).run();
  return row;
}

export async function updateStreamPlatform(
  db: D1Database,
  streamId: string,
  broadcastId: string,
  platformStreamId: string,
  streamKey: string,
  rtmpUrl: string,
): Promise<void> {
  await wrap(db)
    .update(schema.streams)
    .set({
      platform_broadcast_id: broadcastId,
      platform_stream_id: platformStreamId,
      stream_key: streamKey,
      rtmp_url: rtmpUrl,
      status: "ready",
    })
    .where(eq(schema.streams.id, streamId))
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

export async function upsertCredential(
  db: D1Database,
  userId: string,
  platform: string,
  rtmpUrl: string | null,
  streamKey: string | null,
  displayName: string | null,
): Promise<PlatformCredential> {
  const d = wrap(db);
  const id = uuid();
  const ts = now();
  await d
    .insert(schema.platform_credentials)
    .values({
      id,
      user_id: userId,
      platform,
      rtmp_url: rtmpUrl,
      stream_key: streamKey,
      display_name: displayName,
      created_at: ts,
      updated_at: ts,
    })
    .onConflictDoUpdate({
      target: [
        schema.platform_credentials.user_id,
        schema.platform_credentials.platform,
      ],
      set: {
        rtmp_url: rtmpUrl,
        stream_key: streamKey,
        // Preserve prior display_name when the new one is null (matches the
        // COALESCE behavior the hand-written SQL migration shipped with).
        display_name: sql`COALESCE(${displayName}, ${schema.platform_credentials.display_name})`,
        updated_at: ts,
      },
    })
    .run();

  const row = await getCredential(db, userId, platform);
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
