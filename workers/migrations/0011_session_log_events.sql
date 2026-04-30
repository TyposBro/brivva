CREATE TABLE IF NOT EXISTS session_log_events (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  live_session_id TEXT,
  source TEXT NOT NULL,
  level TEXT NOT NULL,
  event TEXT NOT NULL,
  message TEXT,
  fields_json TEXT NOT NULL DEFAULT '{}',
  ts_ms INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS session_log_events_session_ts_idx
  ON session_log_events(session_id, ts_ms);

CREATE INDEX IF NOT EXISTS session_log_events_session_source_idx
  ON session_log_events(session_id, source);
