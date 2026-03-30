-- Migration 001: initial schema
-- All statements use IF NOT EXISTS so the migration is safe to replay.

CREATE TABLE IF NOT EXISTS schema_migrations (
    version  TEXT PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Sessions ------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sessions (
    session_id  TEXT PRIMARY KEY,
    agent_name  TEXT NOT NULL,
    trace_id    TEXT NOT NULL,
    started_at  TEXT NOT NULL,
    ended_at    TEXT,
    outcome     TEXT
);
CREATE INDEX IF NOT EXISTS idx_sessions_trace_id   ON sessions (trace_id);
CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions (started_at);

-- Messages ------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT    NOT NULL REFERENCES sessions (session_id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    payload     TEXT    NOT NULL,  -- JSON-encoded Message
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages (session_id);

-- Events --------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL,
    event_type  TEXT NOT NULL,
    payload     TEXT NOT NULL,  -- JSON-encoded AgentEvent
    timestamp   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_session_id ON events (session_id);
CREATE INDEX IF NOT EXISTS idx_events_timestamp  ON events (timestamp);

-- Checkpoints ---------------------------------------------------------------
CREATE TABLE IF NOT EXISTS checkpoints (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT    NOT NULL,
    version     INTEGER NOT NULL,
    payload     TEXT    NOT NULL,  -- JSON-encoded CheckpointEnvelope
    created_at  TEXT    NOT NULL,
    UNIQUE (session_id, version)
);
CREATE INDEX IF NOT EXISTS idx_checkpoints_session_id ON checkpoints (session_id);

-- Memory --------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS memory (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (session_id, key)
);
CREATE INDEX IF NOT EXISTS idx_memory_session_id ON memory (session_id);
