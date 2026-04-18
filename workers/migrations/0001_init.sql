-- D1 schema for brivva-api. Port of server-rs/src/db.rs DDL.
-- SQLite dialect, identical to the Fargate-era schema so rows can be migrated
-- directly if/when the Fargate DB is dumped.

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    youtube_channel_id TEXT,
    youtube_channel_name TEXT,
    youtube_access_token TEXT,
    youtube_refresh_token TEXT,
    youtube_token_expires_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS voices (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    elevenlabs_voice_id TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS voices_user_id_idx ON voices(user_id);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    voice_id TEXT REFERENCES voices(id),
    title TEXT NOT NULL,
    source_lang TEXT NOT NULL,
    target_langs TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'setup',
    room_id TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_user_id_idx ON sessions(user_id);

CREATE TABLE IF NOT EXISTS streams (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    lang TEXT NOT NULL,
    platform TEXT NOT NULL DEFAULT 'youtube',
    platform_broadcast_id TEXT,
    platform_stream_id TEXT,
    stream_key TEXT,
    rtmp_url TEXT,
    status TEXT NOT NULL DEFAULT 'created',
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS streams_session_id_idx ON streams(session_id);

CREATE TABLE IF NOT EXISTS platform_credentials (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    platform TEXT NOT NULL,
    rtmp_url TEXT,
    stream_key TEXT,
    display_name TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(user_id, platform)
);
CREATE INDEX IF NOT EXISTS platform_creds_user_id_idx ON platform_credentials(user_id);
