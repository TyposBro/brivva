-- Per-session usage rollup. Fargate increments these via
-- PATCH /internal/sessions/:id/metrics; /api/sessions/:id/usage + the
-- billing summary read from here. Rows are lazily created on the first
-- PATCH after a session starts.
CREATE TABLE IF NOT EXISTS session_metrics (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id),
    source_seconds REAL NOT NULL DEFAULT 0,
    output_seconds_json TEXT NOT NULL DEFAULT '{}',
    updated_at INTEGER NOT NULL
);
