CREATE TABLE IF NOT EXISTS session_provider_failures (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    live_session_id TEXT,
    output_id TEXT,
    provider TEXT NOT NULL,
    scope TEXT NOT NULL,
    state TEXT NOT NULL,
    reason TEXT NOT NULL,
    recoverable INTEGER NOT NULL,
    billable INTEGER NOT NULL,
    lang TEXT,
    platform TEXT,
    status_code INTEGER,
    error_code TEXT,
    message TEXT,
    started_at_ms INTEGER NOT NULL,
    recovered_at_ms INTEGER,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS session_provider_failures_session_idx
    ON session_provider_failures(session_id, started_at_ms);

CREATE INDEX IF NOT EXISTS session_provider_failures_provider_idx
    ON session_provider_failures(session_id, provider, state);
