// Thin D1 helpers. One query per function, matching the verbs the FE + Fargate
// already call on the Axum server. Keeps JSON shapes identical to server-rs/db.rs.

import type {
  PlatformCredential,
  Session,
  StreamRecord,
  User,
  Voice,
} from "./types";

const now = () => Math.floor(Date.now() / 1000);
const uuid = () => crypto.randomUUID();

// ── users ─────────────────────────────────────────────────

export async function getOrCreateUser(db: D1Database, id: string): Promise<User> {
  await db
    .prepare("INSERT OR IGNORE INTO users (id, created_at) VALUES (?, ?)")
    .bind(id, now())
    .run();
  const row = await db
    .prepare("SELECT * FROM users WHERE id = ?")
    .bind(id)
    .first<User>();
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
  await db
    .prepare(
      `UPDATE users SET youtube_access_token=?, youtube_refresh_token=?,
         youtube_token_expires_at=?, youtube_channel_id=?, youtube_channel_name=?
       WHERE id=?`,
    )
    .bind(accessToken, refreshToken, expiresAt, channelId, channelName, userId)
    .run();
}

export async function updateAccessToken(
  db: D1Database,
  userId: string,
  accessToken: string,
  expiresAt: number,
): Promise<void> {
  await db
    .prepare(
      "UPDATE users SET youtube_access_token=?, youtube_token_expires_at=? WHERE id=?",
    )
    .bind(accessToken, expiresAt, userId)
    .run();
}

// ── voices ────────────────────────────────────────────────

export async function listVoices(db: D1Database, userId: string): Promise<Voice[]> {
  const { results } = await db
    .prepare("SELECT * FROM voices WHERE user_id=? ORDER BY created_at DESC")
    .bind(userId)
    .all<Voice>();
  return results;
}

export async function getVoice(db: D1Database, voiceId: string): Promise<Voice | null> {
  const row = await db
    .prepare("SELECT * FROM voices WHERE id=?")
    .bind(voiceId)
    .first<Voice>();
  return row ?? null;
}

export async function createVoice(
  db: D1Database,
  userId: string,
  elevenlabsVoiceId: string,
  name: string,
): Promise<Voice> {
  const id = uuid();
  const createdAt = now();
  await db
    .prepare(
      "INSERT INTO voices (id, user_id, elevenlabs_voice_id, name, created_at) VALUES (?,?,?,?,?)",
    )
    .bind(id, userId, elevenlabsVoiceId, name, createdAt)
    .run();
  return {
    id,
    user_id: userId,
    elevenlabs_voice_id: elevenlabsVoiceId,
    name,
    created_at: createdAt,
  };
}

export async function deleteVoiceRow(db: D1Database, voiceId: string): Promise<void> {
  await db.prepare("DELETE FROM voices WHERE id=?").bind(voiceId).run();
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
  const id = uuid();
  const createdAt = now();
  await db
    .prepare(
      `INSERT INTO sessions (id, user_id, voice_id, title, source_lang, target_langs, status, created_at)
       VALUES (?,?,?,?,?,?,?,?)`,
    )
    .bind(id, userId, voiceId, title, sourceLang, targetLangs, "setup", createdAt)
    .run();
  return {
    id,
    user_id: userId,
    voice_id: voiceId,
    title,
    source_lang: sourceLang,
    target_langs: targetLangs,
    status: "setup",
    room_id: null,
    created_at: createdAt,
  };
}

export async function listSessions(db: D1Database, userId: string): Promise<Session[]> {
  const { results } = await db
    .prepare("SELECT * FROM sessions WHERE user_id=? ORDER BY created_at DESC")
    .bind(userId)
    .all<Session>();
  return results;
}

export async function getSession(db: D1Database, id: string): Promise<Session | null> {
  const row = await db
    .prepare("SELECT * FROM sessions WHERE id=?")
    .bind(id)
    .first<Session>();
  return row ?? null;
}

export async function updateSessionStatus(
  db: D1Database,
  id: string,
  status: string,
  roomId: string | null,
): Promise<void> {
  await db
    .prepare("UPDATE sessions SET status=?, room_id=? WHERE id=?")
    .bind(status, roomId, id)
    .run();
}

export async function deleteSessionRow(db: D1Database, id: string): Promise<void> {
  await db.prepare("DELETE FROM streams WHERE session_id=?").bind(id).run();
  await db.prepare("DELETE FROM sessions WHERE id=?").bind(id).run();
}

// ── streams ───────────────────────────────────────────────

export async function listStreams(db: D1Database, sessionId: string): Promise<StreamRecord[]> {
  const { results } = await db
    .prepare("SELECT * FROM streams WHERE session_id=? ORDER BY platform, lang")
    .bind(sessionId)
    .all<StreamRecord>();
  return results;
}

export async function createStreamManual(
  db: D1Database,
  sessionId: string,
  lang: string,
  platform: string,
  rtmpUrl: string,
  streamKey: string,
): Promise<StreamRecord> {
  const id = uuid();
  const createdAt = now();
  await db
    .prepare(
      `INSERT INTO streams (id, session_id, lang, platform, rtmp_url, stream_key, status, created_at)
       VALUES (?,?,?,?,?,?,?,?)`,
    )
    .bind(id, sessionId, lang, platform, rtmpUrl, streamKey, "ready", createdAt)
    .run();
  return {
    id,
    session_id: sessionId,
    lang,
    platform,
    platform_broadcast_id: null,
    platform_stream_id: null,
    stream_key: streamKey,
    rtmp_url: rtmpUrl,
    status: "ready",
    created_at: createdAt,
  };
}

export async function updateStreamPlatform(
  db: D1Database,
  streamId: string,
  broadcastId: string,
  platformStreamId: string,
  streamKey: string,
  rtmpUrl: string,
): Promise<void> {
  await db
    .prepare(
      `UPDATE streams SET platform_broadcast_id=?, platform_stream_id=?, stream_key=?, rtmp_url=?, status='ready'
       WHERE id=?`,
    )
    .bind(broadcastId, platformStreamId, streamKey, rtmpUrl, streamId)
    .run();
}

export async function deleteStreamRow(db: D1Database, streamId: string): Promise<void> {
  await db.prepare("DELETE FROM streams WHERE id=?").bind(streamId).run();
}

// ── platform credentials ──────────────────────────────────

export async function listCredentials(
  db: D1Database,
  userId: string,
): Promise<PlatformCredential[]> {
  const { results } = await db
    .prepare("SELECT * FROM platform_credentials WHERE user_id=? ORDER BY platform")
    .bind(userId)
    .all<PlatformCredential>();
  return results;
}

export async function getCredential(
  db: D1Database,
  userId: string,
  platform: string,
): Promise<PlatformCredential | null> {
  const row = await db
    .prepare(
      "SELECT * FROM platform_credentials WHERE user_id=? AND platform=?",
    )
    .bind(userId, platform)
    .first<PlatformCredential>();
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
  const id = uuid();
  const ts = now();
  await db
    .prepare(
      `INSERT INTO platform_credentials
         (id, user_id, platform, rtmp_url, stream_key, display_name, created_at, updated_at)
       VALUES (?,?,?,?,?,?,?,?)
       ON CONFLICT(user_id, platform) DO UPDATE SET
         rtmp_url = excluded.rtmp_url,
         stream_key = excluded.stream_key,
         display_name = COALESCE(excluded.display_name, platform_credentials.display_name),
         updated_at = excluded.updated_at`,
    )
    .bind(id, userId, platform, rtmpUrl, streamKey, displayName, ts, ts)
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
  await db
    .prepare("DELETE FROM platform_credentials WHERE user_id=? AND platform=?")
    .bind(userId, platform)
    .run();
}
