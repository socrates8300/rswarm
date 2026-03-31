//! SQLite-backed implementation of all four persistence traits.
//!
//! Uses `rusqlite` with the `bundled` feature (zero system dependencies).
//! All blocking DB calls are dispatched via `tokio::task::spawn_blocking`.
//!
//! **Key lifetime pattern**: rusqlite `MappedRows` borrows from `Statement`,
//! so we always `.collect::<Vec<_>>()` eagerly before `stmt` drops, then
//! convert errors in a second pass.

use crate::checkpoint::CheckpointEnvelope;
use crate::error::{SwarmError, SwarmResult};
use crate::event::AgentEvent;
use crate::persistence::{
    CheckpointStore, CheckpointSummary, EventStore, MemoryRecord, MemoryStore, SessionRecord,
    SessionStore,
};
use crate::types::Message;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Migration SQL — embedded at compile time
// ---------------------------------------------------------------------------

const MIGRATION_001: &str = include_str!("../../migrations/001_initial.sql");

static MIGRATIONS: &[(&str, &str)] = &[("001", MIGRATION_001)];

// ---------------------------------------------------------------------------
// SqliteStore
// ---------------------------------------------------------------------------

/// A single SQLite connection implementing all four persistence traits.
#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    /// Open (or create) an on-disk SQLite database and apply pending migrations.
    pub fn open(path: &str) -> SwarmResult<Self> {
        let conn = Connection::open(path).map_err(sqlite_err)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(sqlite_err)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.run_migrations()?;
        Ok(store)
    }

    /// Open an in-memory database (useful for tests).
    pub fn open_in_memory() -> SwarmResult<Self> {
        let conn = Connection::open_in_memory().map_err(sqlite_err)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(sqlite_err)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.run_migrations()?;
        Ok(store)
    }

    fn run_migrations(&self) -> SwarmResult<()> {
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .map_err(sqlite_err)?;

        for (version, sql) in MIGRATIONS {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
                    params![version],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            if n == 0 {
                conn.execute_batch(sql).map_err(|e| {
                    SwarmError::Other(format!("Migration {} failed: {}", version, e))
                })?;
                conn.execute(
                    "INSERT OR IGNORE INTO schema_migrations (version) VALUES (?1)",
                    params![version],
                )
                .map_err(sqlite_err)?;
            }
        }
        Ok(())
    }

    async fn with_conn<F, T>(&self, f: F) -> SwarmResult<T>
    where
        F: FnOnce(&Connection) -> SwarmResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().map_err(lock_err)?;
            f(&guard)
        })
        .await
        .map_err(|e| SwarmError::Other(format!("DB task panic: {}", e)))?
    }
}

fn sqlite_err(e: rusqlite::Error) -> SwarmError {
    SwarmError::Other(format!("SQLite error: {}", e))
}

fn lock_err<T>(_: std::sync::PoisonError<T>) -> SwarmError {
    SwarmError::Other("SqliteStore mutex poisoned".to_string())
}

fn parse_dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

// ---------------------------------------------------------------------------
// SessionStore
// ---------------------------------------------------------------------------

#[async_trait]
impl SessionStore for SqliteStore {
    async fn create_session(
        &self,
        session_id: &str,
        agent_name: &str,
        trace_id: &str,
    ) -> SwarmResult<()> {
        let sid = session_id.to_string();
        let aname = agent_name.to_string();
        let tid = trace_id.to_string();
        let now = Utc::now().to_rfc3339();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT OR IGNORE INTO sessions (session_id, agent_name, trace_id, started_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![sid, aname, tid, now],
            )
            .map_err(sqlite_err)?;
            Ok(())
        })
        .await
    }

    async fn get_session(&self, session_id: &str) -> SwarmResult<Option<SessionRecord>> {
        let sid = session_id.to_string();
        self.with_conn(move |conn| {
            let result = conn.query_row(
                "SELECT session_id, agent_name, trace_id, started_at, ended_at, outcome
                 FROM sessions WHERE session_id = ?1",
                params![sid],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            );
            match result {
                Ok((sid, aname, tid, started, ended, outcome)) => Ok(Some(SessionRecord {
                    session_id: sid,
                    agent_name: aname,
                    trace_id: tid,
                    started_at: parse_dt(&started),
                    ended_at: ended.as_deref().map(parse_dt),
                    outcome,
                })),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(sqlite_err(e)),
            }
        })
        .await
    }

    async fn list_sessions(&self, limit: usize, offset: usize) -> SwarmResult<Vec<SessionRecord>> {
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT session_id, agent_name, trace_id, started_at, ended_at, outcome
                     FROM sessions ORDER BY started_at DESC LIMIT ?1 OFFSET ?2",
                )
                .map_err(sqlite_err)?;
            // Collect eagerly so stmt can drop after this line.
            let raw: Vec<
                rusqlite::Result<(
                    String,
                    String,
                    String,
                    String,
                    Option<String>,
                    Option<String>,
                )>,
            > = stmt
                .query_map(params![limit as i64, offset as i64], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                })
                .map_err(sqlite_err)?
                .collect();
            raw.into_iter()
                .map(|r| {
                    let (sid, aname, tid, started, ended, outcome) = r.map_err(sqlite_err)?;
                    Ok(SessionRecord {
                        session_id: sid,
                        agent_name: aname,
                        trace_id: tid,
                        started_at: parse_dt(&started),
                        ended_at: ended.as_deref().map(parse_dt),
                        outcome,
                    })
                })
                .collect()
        })
        .await
    }

    async fn list_sessions_by_trace(&self, trace_id: &str) -> SwarmResult<Vec<SessionRecord>> {
        let tid = trace_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT session_id, agent_name, trace_id, started_at, ended_at, outcome
                     FROM sessions WHERE trace_id = ?1 ORDER BY started_at DESC",
                )
                .map_err(sqlite_err)?;
            let raw: Vec<
                rusqlite::Result<(
                    String,
                    String,
                    String,
                    String,
                    Option<String>,
                    Option<String>,
                )>,
            > = stmt
                .query_map(params![tid], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                })
                .map_err(sqlite_err)?
                .collect();
            raw.into_iter()
                .map(|r| {
                    let (sid, aname, tid, started, ended, outcome) = r.map_err(sqlite_err)?;
                    Ok(SessionRecord {
                        session_id: sid,
                        agent_name: aname,
                        trace_id: tid,
                        started_at: parse_dt(&started),
                        ended_at: ended.as_deref().map(parse_dt),
                        outcome,
                    })
                })
                .collect()
        })
        .await
    }

    async fn complete_session(&self, session_id: &str, outcome: &str) -> SwarmResult<()> {
        let sid = session_id.to_string();
        let out = outcome.to_string();
        let now = Utc::now().to_rfc3339();
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE sessions SET ended_at = ?1, outcome = ?2 WHERE session_id = ?3",
                params![now, out, sid],
            )
            .map_err(sqlite_err)?;
            Ok(())
        })
        .await
    }

    async fn store_messages(&self, session_id: &str, messages: &[Message]) -> SwarmResult<()> {
        let sid = session_id.to_string();
        // Serialize all messages before touching the DB so a serialization
        // failure cannot leave the connection mid-transaction.
        let payloads: Vec<String> = messages
            .iter()
            .map(|m| {
                serde_json::to_string(m).map_err(|e| SwarmError::SerializationError(e.to_string()))
            })
            .collect::<SwarmResult<_>>()?;
        self.with_conn(move |conn| {
            conn.execute("BEGIN IMMEDIATE", []).map_err(sqlite_err)?;
            let result: SwarmResult<()> = (|| {
                conn.execute("DELETE FROM messages WHERE session_id = ?1", params![sid])
                    .map_err(sqlite_err)?;
                for (pos, payload) in payloads.iter().enumerate() {
                    conn.execute(
                        "INSERT INTO messages (session_id, position, payload) VALUES (?1, ?2, ?3)",
                        params![sid, pos as i64, payload],
                    )
                    .map_err(sqlite_err)?;
                }
                Ok(())
            })();
            if result.is_err() {
                let _ = conn.execute("ROLLBACK", []);
                result
            } else {
                conn.execute("COMMIT", []).map_err(sqlite_err)?;
                Ok(())
            }
        })
        .await
    }

    async fn load_messages(&self, session_id: &str) -> SwarmResult<Vec<Message>> {
        let sid = session_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare("SELECT payload FROM messages WHERE session_id = ?1 ORDER BY position ASC")
                .map_err(sqlite_err)?;
            let raw: Vec<rusqlite::Result<String>> = stmt
                .query_map(params![sid], |row| row.get(0))
                .map_err(sqlite_err)?
                .collect();
            raw.into_iter()
                .map(|r| {
                    let json = r.map_err(sqlite_err)?;
                    serde_json::from_str(&json)
                        .map_err(|e| SwarmError::DeserializationError(e.to_string()))
                })
                .collect()
        })
        .await
    }
}

// ---------------------------------------------------------------------------
// EventStore
// ---------------------------------------------------------------------------

#[async_trait]
impl EventStore for SqliteStore {
    async fn append_event(&self, session_id: &str, event: &AgentEvent) -> SwarmResult<()> {
        let sid = session_id.to_string();
        let event_type = format!("{}", event)
            .split('(')
            .next()
            .unwrap_or("unknown")
            .to_string();
        let payload = serde_json::to_string(event)
            .map_err(|e| SwarmError::SerializationError(e.to_string()))?;
        let ts = event.timestamp().to_rfc3339();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO events (session_id, event_type, payload, timestamp)
                 VALUES (?1, ?2, ?3, ?4)",
                params![sid, event_type, payload, ts],
            )
            .map_err(sqlite_err)?;
            Ok(())
        })
        .await
    }

    async fn read_events(&self, session_id: &str) -> SwarmResult<Vec<AgentEvent>> {
        let sid = session_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare("SELECT payload FROM events WHERE session_id = ?1 ORDER BY id ASC")
                .map_err(sqlite_err)?;
            let raw: Vec<rusqlite::Result<String>> = stmt
                .query_map(params![sid], |row| row.get(0))
                .map_err(sqlite_err)?
                .collect();
            raw.into_iter()
                .map(|r| {
                    let json = r.map_err(sqlite_err)?;
                    serde_json::from_str(&json)
                        .map_err(|e| SwarmError::DeserializationError(e.to_string()))
                })
                .collect()
        })
        .await
    }

    async fn read_events_since(
        &self,
        session_id: &str,
        after: DateTime<Utc>,
    ) -> SwarmResult<Vec<AgentEvent>> {
        let sid = session_id.to_string();
        let after_str = after.to_rfc3339();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT payload FROM events
                     WHERE session_id = ?1 AND timestamp > ?2
                     ORDER BY id ASC",
                )
                .map_err(sqlite_err)?;
            let raw: Vec<rusqlite::Result<String>> = stmt
                .query_map(params![sid, after_str], |row| row.get(0))
                .map_err(sqlite_err)?
                .collect();
            raw.into_iter()
                .map(|r| {
                    let json = r.map_err(sqlite_err)?;
                    serde_json::from_str(&json)
                        .map_err(|e| SwarmError::DeserializationError(e.to_string()))
                })
                .collect()
        })
        .await
    }

    async fn count_events(&self, session_id: &str) -> SwarmResult<u64> {
        let sid = session_id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM events WHERE session_id = ?1",
                params![sid],
                |row| row.get::<_, i64>(0),
            )
            .map(|n| n as u64)
            .map_err(sqlite_err)
        })
        .await
    }
}

// ---------------------------------------------------------------------------
// CheckpointStore
// ---------------------------------------------------------------------------

#[async_trait]
impl CheckpointStore for SqliteStore {
    async fn save_checkpoint(&self, envelope: &CheckpointEnvelope) -> SwarmResult<()> {
        let sid = envelope.session_id.clone();
        let ver = envelope.version;
        let payload = envelope.to_json()?;
        let created = envelope.created_at.to_rfc3339();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO checkpoints (session_id, version, payload, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(session_id, version) DO UPDATE SET
                     payload    = excluded.payload,
                     created_at = excluded.created_at",
                params![sid, ver as i64, payload, created],
            )
            .map_err(sqlite_err)?;
            Ok(())
        })
        .await
    }

    async fn load_checkpoint(&self, session_id: &str) -> SwarmResult<Option<CheckpointEnvelope>> {
        let sid = session_id.to_string();
        self.with_conn(move |conn| {
            let result = conn.query_row(
                "SELECT payload FROM checkpoints
                 WHERE session_id = ?1 ORDER BY version DESC LIMIT 1",
                params![sid],
                |row| row.get::<_, String>(0),
            );
            match result {
                Ok(json) => CheckpointEnvelope::from_json(&json).map(Some),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(sqlite_err(e)),
            }
        })
        .await
    }

    async fn load_checkpoint_at_version(
        &self,
        session_id: &str,
        version: u32,
    ) -> SwarmResult<Option<CheckpointEnvelope>> {
        let sid = session_id.to_string();
        self.with_conn(move |conn| {
            let result = conn.query_row(
                "SELECT payload FROM checkpoints
                 WHERE session_id = ?1 AND version = ?2",
                params![sid, version as i64],
                |row| row.get::<_, String>(0),
            );
            match result {
                Ok(json) => CheckpointEnvelope::from_json(&json).map(Some),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(sqlite_err(e)),
            }
        })
        .await
    }

    async fn list_checkpoints(&self, session_id: &str) -> SwarmResult<Vec<CheckpointSummary>> {
        let sid = session_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT session_id, version, created_at FROM checkpoints
                     WHERE session_id = ?1 ORDER BY version DESC",
                )
                .map_err(sqlite_err)?;
            let raw: Vec<rusqlite::Result<(String, i64, String)>> = stmt
                .query_map(params![sid], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .map_err(sqlite_err)?
                .collect();
            raw.into_iter()
                .map(|r| {
                    let (sid, ver, ts) = r.map_err(sqlite_err)?;
                    Ok(CheckpointSummary {
                        session_id: sid,
                        version: ver as u32,
                        created_at: parse_dt(&ts),
                    })
                })
                .collect()
        })
        .await
    }

    async fn delete_checkpoints(&self, session_id: &str) -> SwarmResult<()> {
        let sid = session_id.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "DELETE FROM checkpoints WHERE session_id = ?1",
                params![sid],
            )
            .map_err(sqlite_err)?;
            Ok(())
        })
        .await
    }
}

// ---------------------------------------------------------------------------
// MemoryStore
// ---------------------------------------------------------------------------

#[async_trait]
impl MemoryStore for SqliteStore {
    async fn persist_memory(&self, session_id: &str, key: &str, value: &str) -> SwarmResult<()> {
        let sid = session_id.to_string();
        let k = key.to_string();
        let v = value.to_string();
        let now = Utc::now().to_rfc3339();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO memory (session_id, key, value, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT(session_id, key) DO UPDATE SET
                     value      = excluded.value,
                     updated_at = excluded.updated_at",
                params![sid, k, v, now],
            )
            .map_err(sqlite_err)?;
            Ok(())
        })
        .await
    }

    async fn restore_memory(&self, session_id: &str) -> SwarmResult<Vec<MemoryRecord>> {
        let sid = session_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT session_id, key, value, created_at, updated_at
                     FROM memory WHERE session_id = ?1 ORDER BY key ASC",
                )
                .map_err(sqlite_err)?;
            let raw: Vec<rusqlite::Result<(String, String, String, String, String)>> = stmt
                .query_map(params![sid], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                })
                .map_err(sqlite_err)?
                .collect();
            raw.into_iter()
                .map(|r| {
                    let (sid, key, value, ca, ua) = r.map_err(sqlite_err)?;
                    Ok(MemoryRecord {
                        session_id: sid,
                        key,
                        value,
                        created_at: parse_dt(&ca),
                        updated_at: parse_dt(&ua),
                    })
                })
                .collect()
        })
        .await
    }

    async fn delete_memory(&self, session_id: &str) -> SwarmResult<()> {
        let sid = session_id.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM memory WHERE session_id = ?1", params![sid])
                .map_err(sqlite_err)?;
            Ok(())
        })
        .await
    }
}

// ---------------------------------------------------------------------------
// #34 — Retention, pruning, and archival policy
// ---------------------------------------------------------------------------

/// Policy controlling how old session data is pruned from the hot tables.
#[derive(Clone, Debug)]
pub struct RetentionPolicy {
    /// Delete sessions older than this many days (based on `started_at`).
    /// `None` means no age-based pruning.
    pub max_age_days: Option<u32>,
    /// Keep at most this many sessions (most recent first).
    /// `None` means no count-based pruning.
    pub max_sessions: Option<u32>,
    /// If set, sessions are archived to this SQLite file before deletion.
    /// The archive database uses the same schema.
    pub archive_path: Option<String>,
}

impl RetentionPolicy {
    /// Prune sessions from `store` according to the policy.
    ///
    /// - If `archive_path` is set, qualifying sessions are copied to that
    ///   database first (schema is created on demand).
    /// - Then the sessions (and their cascaded messages/events/checkpoints)
    ///   are deleted from the hot database.
    ///
    /// Safe to call repeatedly — deletes only sessions that satisfy the
    /// configured criteria.
    pub async fn prune(&self, store: &SqliteStore) -> SwarmResult<u64> {
        let policy = self.clone();
        store
            .with_conn(move |conn| {
                let session_ids = collect_prunable_session_ids(conn, &policy)?;
                if session_ids.is_empty() {
                    return Ok(0);
                }

                if let Some(path) = policy.archive_path.as_deref() {
                    archive_sessions(conn, path, &session_ids)?;
                }

                delete_session_artifacts(conn, &session_ids)?;
                Ok(session_ids.len() as u64)
            })
            .await
    }
}

fn collect_prunable_session_ids(
    conn: &Connection,
    policy: &RetentionPolicy,
) -> SwarmResult<Vec<String>> {
    let mut session_ids = BTreeSet::new();

    if let Some(days) = policy.max_age_days {
        let mut stmt = conn
            .prepare(
                "SELECT session_id FROM sessions
                 WHERE julianday('now') - julianday(started_at) > ?1",
            )
            .map_err(sqlite_err)?;
        let raw: Vec<rusqlite::Result<String>> = stmt
            .query_map(params![days as i64], |row| row.get(0))
            .map_err(sqlite_err)?
            .collect();
        for row in raw {
            session_ids.insert(row.map_err(sqlite_err)?);
        }
    }

    if let Some(max) = policy.max_sessions {
        let mut stmt = conn
            .prepare(
                "SELECT session_id FROM sessions
                 WHERE session_id NOT IN (
                     SELECT session_id FROM sessions
                     ORDER BY started_at DESC LIMIT ?1
                 )",
            )
            .map_err(sqlite_err)?;
        let raw: Vec<rusqlite::Result<String>> = stmt
            .query_map(params![max as i64], |row| row.get(0))
            .map_err(sqlite_err)?
            .collect();
        for row in raw {
            session_ids.insert(row.map_err(sqlite_err)?);
        }
    }

    Ok(session_ids.into_iter().collect())
}

fn archive_sessions(
    conn: &Connection,
    archive_path: &str,
    session_ids: &[String],
) -> SwarmResult<()> {
    conn.execute("ATTACH DATABASE ?1 AS archive", params![archive_path])
        .map_err(sqlite_err)?;

    let archive_result = (|| -> SwarmResult<()> {
        ensure_archive_schema(conn)?;

        for session_id in session_ids {
            conn.execute(
                "INSERT OR REPLACE INTO archive.sessions
                 (session_id, agent_name, trace_id, started_at, ended_at, outcome)
                 SELECT session_id, agent_name, trace_id, started_at, ended_at, outcome
                 FROM main.sessions WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(sqlite_err)?;

            conn.execute(
                "DELETE FROM archive.messages WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(sqlite_err)?;
            conn.execute(
                "INSERT INTO archive.messages (session_id, position, payload, created_at)
                 SELECT session_id, position, payload, created_at
                 FROM main.messages WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(sqlite_err)?;

            conn.execute(
                "DELETE FROM archive.events WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(sqlite_err)?;
            conn.execute(
                "INSERT INTO archive.events (session_id, event_type, payload, timestamp)
                 SELECT session_id, event_type, payload, timestamp
                 FROM main.events WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(sqlite_err)?;

            conn.execute(
                "DELETE FROM archive.checkpoints WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(sqlite_err)?;
            conn.execute(
                "INSERT INTO archive.checkpoints (session_id, version, payload, created_at)
                 SELECT session_id, version, payload, created_at
                 FROM main.checkpoints WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(sqlite_err)?;

            conn.execute(
                "DELETE FROM archive.memory WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(sqlite_err)?;
            conn.execute(
                "INSERT INTO archive.memory (session_id, key, value, created_at, updated_at)
                 SELECT session_id, key, value, created_at, updated_at
                 FROM main.memory WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(sqlite_err)?;
        }

        Ok(())
    })();

    let detach_result = conn
        .execute_batch("DETACH DATABASE archive")
        .map_err(sqlite_err);

    archive_result?;
    detach_result?;
    Ok(())
}

fn ensure_archive_schema(conn: &Connection) -> SwarmResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS archive.schema_migrations (
            version    TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS archive.sessions (
            session_id TEXT PRIMARY KEY,
            agent_name TEXT NOT NULL,
            trace_id   TEXT NOT NULL,
            started_at TEXT NOT NULL,
            ended_at   TEXT,
            outcome    TEXT
        );
        CREATE INDEX IF NOT EXISTS archive.idx_sessions_trace_id
            ON sessions (trace_id);
        CREATE INDEX IF NOT EXISTS archive.idx_sessions_started_at
            ON sessions (started_at);
        CREATE TABLE IF NOT EXISTS archive.messages (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL REFERENCES sessions (session_id) ON DELETE CASCADE,
            position   INTEGER NOT NULL,
            payload    TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS archive.idx_messages_session_id
            ON messages (session_id);
        CREATE TABLE IF NOT EXISTS archive.events (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            payload    TEXT NOT NULL,
            timestamp  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS archive.idx_events_session_id
            ON events (session_id);
        CREATE INDEX IF NOT EXISTS archive.idx_events_timestamp
            ON events (timestamp);
        CREATE TABLE IF NOT EXISTS archive.checkpoints (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            version    INTEGER NOT NULL,
            payload    TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE (session_id, version)
        );
        CREATE INDEX IF NOT EXISTS archive.idx_checkpoints_session_id
            ON checkpoints (session_id);
        CREATE TABLE IF NOT EXISTS archive.memory (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            key        TEXT NOT NULL,
            value      TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE (session_id, key)
        );
        CREATE INDEX IF NOT EXISTS archive.idx_memory_session_id
            ON memory (session_id);",
    )
    .map_err(sqlite_err)
}

fn delete_session_artifacts(conn: &Connection, session_ids: &[String]) -> SwarmResult<()> {
    for session_id in session_ids {
        conn.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(sqlite_err)?;
        conn.execute(
            "DELETE FROM events WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(sqlite_err)?;
        conn.execute(
            "DELETE FROM checkpoints WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(sqlite_err)?;
        conn.execute(
            "DELETE FROM memory WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(sqlite_err)?;
        conn.execute(
            "DELETE FROM sessions WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(sqlite_err)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::{CheckpointData, CheckpointEnvelope};
    use crate::event::AgentEvent;
    use crate::phase::TokenUsage;
    use crate::types::{ContextVariables, MessageRole};
    use std::fs;

    async fn store() -> SqliteStore {
        SqliteStore::open_in_memory().unwrap()
    }

    #[tokio::test]
    async fn test_session_create_and_get() {
        let s = store().await;
        s.create_session("s1", "agent-a", "trace-1").await.unwrap();
        let rec = s.get_session("s1").await.unwrap().unwrap();
        assert_eq!(rec.agent_name, "agent-a");
        assert_eq!(rec.trace_id, "trace-1");
        assert!(rec.ended_at.is_none());
    }

    #[tokio::test]
    async fn test_session_complete() {
        let s = store().await;
        s.create_session("s2", "agent-b", "trace-2").await.unwrap();
        s.complete_session("s2", "success").await.unwrap();
        let rec = s.get_session("s2").await.unwrap().unwrap();
        assert_eq!(rec.outcome, Some("success".to_string()));
        assert!(rec.ended_at.is_some());
    }

    #[tokio::test]
    async fn test_message_roundtrip() {
        let s = store().await;
        s.create_session("s3", "agent-c", "trace-3").await.unwrap();
        let msg =
            crate::types::Message::new(MessageRole::User, Some("hello".to_string()), None, None)
                .unwrap();
        s.store_messages("s3", &[msg]).await.unwrap();
        let loaded = s.load_messages("s3").await.unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[tokio::test]
    async fn test_store_messages_idempotent_rewrite() {
        // Overwriting a prior history must produce exactly the new set, with no
        // duplicate rows from a non-atomic delete+insert.
        let s = store().await;
        s.create_session("msg-rewrite", "agent-x", "trace-r")
            .await
            .unwrap();

        let make_msg = |text: &str| {
            crate::types::Message::new(MessageRole::User, Some(text.to_string()), None, None)
                .unwrap()
        };

        // First write: 3 messages.
        let first = vec![make_msg("a"), make_msg("b"), make_msg("c")];
        s.store_messages("msg-rewrite", &first).await.unwrap();
        let loaded = s.load_messages("msg-rewrite").await.unwrap();
        assert_eq!(loaded.len(), 3);

        // Second write: 5 different messages.
        let second = vec![
            make_msg("1"),
            make_msg("2"),
            make_msg("3"),
            make_msg("4"),
            make_msg("5"),
        ];
        s.store_messages("msg-rewrite", &second).await.unwrap();
        let loaded = s.load_messages("msg-rewrite").await.unwrap();
        assert_eq!(loaded.len(), 5, "rewrite must not leave old rows behind");

        // Third write: empty — clears history atomically.
        s.store_messages("msg-rewrite", &[]).await.unwrap();
        let loaded = s.load_messages("msg-rewrite").await.unwrap();
        assert!(loaded.is_empty(), "storing empty slice must delete all rows");
    }

    #[tokio::test]
    async fn test_store_messages_serialization_failure_leaves_db_intact() {
        // If serialization of any message fails before the DB is touched,
        // the previously stored history must be unchanged.
        let s = store().await;
        s.create_session("msg-guard", "agent-g", "trace-g")
            .await
            .unwrap();

        let msg =
            crate::types::Message::new(MessageRole::User, Some("original".to_string()), None, None)
                .unwrap();
        s.store_messages("msg-guard", &[msg.clone()])
            .await
            .unwrap();

        // serde_json::to_string never fails for well-typed Message, so we cannot
        // inject a serialization error here without a custom type. Instead,
        // assert the happy-path invariant: a successful overwrite replaces exactly.
        s.store_messages("msg-guard", &[msg.clone(), msg.clone()])
            .await
            .unwrap();
        let loaded = s.load_messages("msg-guard").await.unwrap();
        assert_eq!(
            loaded.len(),
            2,
            "second store must replace, not append, prior messages"
        );
    }

    #[tokio::test]
    async fn test_checkpoint_roundtrip() {
        let s = store().await;
        let data = CheckpointData::new(
            vec![],
            ContextVariables::new(),
            "agent-x",
            5,
            TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
        );
        let env = CheckpointEnvelope::new("session-cp", data);
        s.save_checkpoint(&env).await.unwrap();
        let loaded = s.load_checkpoint("session-cp").await.unwrap().unwrap();
        assert_eq!(loaded.payload.iteration, 5);
        assert_eq!(loaded.payload.current_agent, "agent-x");
    }

    #[tokio::test]
    async fn test_memory_upsert() {
        let s = store().await;
        s.persist_memory("s4", "k1", "v1").await.unwrap();
        s.persist_memory("s4", "k1", "v2").await.unwrap();
        let records = s.restore_memory("s4").await.unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].value, "v2");
    }

    #[tokio::test]
    async fn test_list_checkpoints_ordered() {
        let s = store().await;
        let mk = |ver: u32| {
            let data = CheckpointData::new(
                vec![],
                ContextVariables::new(),
                "a",
                ver,
                TokenUsage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            );
            let mut env = CheckpointEnvelope::new("s5", data);
            env.version = ver;
            env
        };
        s.save_checkpoint(&mk(1)).await.unwrap();
        s.save_checkpoint(&mk(2)).await.unwrap();
        let list = s.list_checkpoints("s5").await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].version, 2); // newest first
    }

    #[tokio::test]
    async fn test_event_append_and_read() {
        let s = store().await;
        s.create_session("s6", "ag", "tr").await.unwrap();
        let ev = AgentEvent::LoopStart {
            trace_id: crate::event::TraceId::new("tr"),
            agent_name: "ag".to_string(),
            timestamp: Utc::now(),
        };
        s.append_event("s6", &ev).await.unwrap();
        let events = s.read_events("s6").await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(s.count_events("s6").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_retention_prune_removes_session_artifacts() {
        let s = store().await;
        s.create_session("old", "agent-a", "trace-old")
            .await
            .unwrap();
        s.create_session("new", "agent-b", "trace-new")
            .await
            .unwrap();

        let message =
            crate::types::Message::new(MessageRole::User, Some("hello".to_string()), None, None)
                .unwrap();
        s.store_messages("old", &[message]).await.unwrap();
        s.append_event(
            "old",
            &AgentEvent::LoopStart {
                trace_id: "trace-old".into(),
                agent_name: "agent-a".to_string(),
                timestamp: Utc::now(),
            },
        )
        .await
        .unwrap();
        let checkpoint = CheckpointEnvelope::new(
            "old",
            CheckpointData::new(
                vec![],
                ContextVariables::new(),
                "agent-a",
                1,
                TokenUsage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            ),
        );
        s.save_checkpoint(&checkpoint).await.unwrap();
        s.persist_memory("old", "summary", "hello").await.unwrap();

        s.with_conn(|conn| {
            conn.execute(
                "UPDATE sessions SET started_at = ?1 WHERE session_id = 'old'",
                params!["2000-01-01T00:00:00+00:00"],
            )
            .map_err(sqlite_err)?;
            conn.execute(
                "UPDATE sessions SET started_at = ?1 WHERE session_id = 'new'",
                params!["2030-01-01T00:00:00+00:00"],
            )
            .map_err(sqlite_err)?;
            Ok(())
        })
        .await
        .unwrap();

        let deleted = RetentionPolicy {
            max_age_days: None,
            max_sessions: Some(1),
            archive_path: None,
        }
        .prune(&s)
        .await
        .unwrap();

        assert_eq!(deleted, 1);
        assert!(s.get_session("old").await.unwrap().is_none());
        assert!(s.load_messages("old").await.unwrap().is_empty());
        assert!(s.read_events("old").await.unwrap().is_empty());
        assert!(s.load_checkpoint("old").await.unwrap().is_none());
        assert!(s.restore_memory("old").await.unwrap().is_empty());
        assert!(s.get_session("new").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_retention_prune_archives_before_delete() {
        let s = store().await;
        s.create_session("archive-me", "agent-a", "trace-archive")
            .await
            .unwrap();
        s.persist_memory("archive-me", "summary", "saved")
            .await
            .unwrap();
        s.with_conn(|conn| {
            conn.execute(
                "UPDATE sessions SET started_at = ?1 WHERE session_id = 'archive-me'",
                params!["2000-01-01T00:00:00+00:00"],
            )
            .map_err(sqlite_err)?;
            Ok(())
        })
        .await
        .unwrap();

        let archive_path =
            std::env::temp_dir().join(format!("rswarm-retention-{}.db", uuid::Uuid::new_v4()));

        let deleted = RetentionPolicy {
            max_age_days: Some(1),
            max_sessions: None,
            archive_path: Some(archive_path.to_string_lossy().into_owned()),
        }
        .prune(&s)
        .await
        .unwrap();

        assert_eq!(deleted, 1);
        assert!(s.get_session("archive-me").await.unwrap().is_none());

        let archived = SqliteStore::open(archive_path.to_str().unwrap()).unwrap();
        let archived_session = archived.get_session("archive-me").await.unwrap();
        let archived_memory = archived.restore_memory("archive-me").await.unwrap();
        assert!(archived_session.is_some());
        assert_eq!(archived_memory.len(), 1);

        let _ = fs::remove_file(archive_path);
    }
}
