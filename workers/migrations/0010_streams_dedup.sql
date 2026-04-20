-- Dedup guard for streams. Without this, two concurrent POST
-- /api/sessions/:id/streams calls with the same (lang, platform) from the
-- FE (double-click, retry-on-timeout, or the Grip auto-provision path
-- racing a manual add) both insert rows — Fargate then spawns two ffmpeg
-- processes for one destination, producing duplicate RTMP pushes.
-- Unique index enforces one destination per (session, lang, platform) at
-- the DB layer; the handler uses ON CONFLICT DO NOTHING + a follow-up
-- select so callers see a single canonical row.
CREATE UNIQUE INDEX IF NOT EXISTS streams_session_lang_platform_unique
    ON streams (session_id, lang, platform);
