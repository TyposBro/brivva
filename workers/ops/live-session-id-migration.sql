-- One-off upgrade for an already-deployed D1 database that still has
-- `sessions.room_id` instead of `sessions.live_session_id`.
--
-- Apply manually with Wrangler before deploying code that expects the new
-- column name everywhere.
--
-- Local example:
--   wrangler d1 execute brivva --local --file=ops/live-session-id-migration.sql
--
-- Remote example:
--   wrangler d1 execute brivva --remote --file=ops/live-session-id-migration.sql

PRAGMA foreign_keys=OFF;

CREATE TABLE sessions_new (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    voice_id TEXT REFERENCES voices(id),
    title TEXT NOT NULL,
    source_lang TEXT NOT NULL,
    target_langs TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'setup',
    live_session_id TEXT,
    created_at INTEGER NOT NULL
);

INSERT INTO sessions_new (
    id,
    user_id,
    voice_id,
    title,
    source_lang,
    target_langs,
    status,
    live_session_id,
    created_at
)
SELECT
    id,
    user_id,
    voice_id,
    title,
    source_lang,
    target_langs,
    status,
    room_id,
    created_at
FROM sessions;

DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;
CREATE INDEX IF NOT EXISTS sessions_user_id_idx ON sessions(user_id);

PRAGMA foreign_keys=ON;
