//! PostgreSQL-backed implementation of the persistence traits.
//!
//! This backend is feature-gated so the default crate remains dependency-light,
//! while open source consumers can opt into a networked SQL store instead of
//! being limited to SQLite.

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
use std::sync::Arc;
use tokio_postgres::tls::MakeTlsConnect;
use tokio_postgres::{Client, NoTls, Socket};

#[cfg(feature = "postgres-tls")]
use {
    rustls_native_certs::load_native_certs,
    tokio_postgres_rustls::MakeRustlsConnect,
};

const MIGRATION_001: &str = include_str!("../../migrations/postgres/001_initial.sql");
static MIGRATIONS: &[(&str, &str)] = &[("001", MIGRATION_001)];

#[derive(Clone, Debug)]
pub struct PostgresStore {
    client: Arc<Client>,
}

impl PostgresStore {
    /// Connect to a PostgreSQL database **without TLS** (`NoTls`).
    ///
    /// This is suitable only for Unix-socket connections or `localhost`-only
    /// deployments where the transport is already secured by the OS.  For any
    /// connection that crosses a network boundary you **must** provide a TLS
    /// connector; see `connect_tls` (planned) or upstream `tokio-postgres` docs.
    pub async fn connect(connection_string: &str) -> SwarmResult<Self> {
        // Warn loudly when the connection string does not look local, because
        // NoTls over a real network sends credentials in the clear.
        let looks_remote = !connection_string.contains("localhost")
            && !connection_string.contains("127.0.0.1")
            && !connection_string.starts_with('/');
        if looks_remote {
            return Err(SwarmError::ConfigError(
                "PostgresStore::connect uses NoTls and is only safe for Unix-socket \
                 or localhost connections. For remote hosts supply a TLS connector via \
                 PostgresStore::connect_tls, or enable the `postgres-tls` feature and \
                 use PostgresStore::connect_with_native_roots."
                    .to_string(),
            ));
        }

        let (client, connection) = tokio_postgres::connect(connection_string, NoTls)
            .await
            .map_err(pg_err)?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!("postgres connection error: {}", error);
            }
        });

        let store = Self {
            client: Arc::new(client),
        };
        store.run_migrations().await?;
        Ok(store)
    }

    /// Connect using a caller-supplied TLS connector.
    ///
    /// Any type implementing [`MakeTlsConnect<Socket>`] is accepted, e.g. from
    /// `tokio-postgres-native-tls` or `tokio-postgres-rustls`.  For a zero-config
    /// path with system root certificates enable the `postgres-tls` feature and
    /// use [`PostgresStore::connect_with_native_roots`].
    pub async fn connect_tls<T>(connection_string: &str, tls: T) -> SwarmResult<Self>
    where
        T: MakeTlsConnect<Socket> + Send + 'static,
        T::Stream: Send,
        T::TlsConnect: Send,
        <T::TlsConnect as tokio_postgres::tls::TlsConnect<Socket>>::Future: Send,
    {
        let (client, connection) = tokio_postgres::connect(connection_string, tls)
            .await
            .map_err(pg_err)?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!("postgres connection error: {}", error);
            }
        });
        let store = Self {
            client: Arc::new(client),
        };
        store.run_migrations().await?;
        Ok(store)
    }

    /// Connect to a remote PostgreSQL server using TLS with the system's native
    /// root certificate store.
    ///
    /// Requires the `postgres-tls` feature.  For custom trust anchors or client
    /// certificates use [`PostgresStore::connect_tls`] with a manually constructed
    /// `rustls::ClientConfig`.
    #[cfg(feature = "postgres-tls")]
    pub async fn connect_with_native_roots(connection_string: &str) -> SwarmResult<Self> {
        let mut roots = rustls::RootCertStore::empty();
        let certs = load_native_certs().map_err(|e| {
            SwarmError::ConfigError(format!("failed to load native TLS certs: {e}"))
        })?;
        for cert in certs {
            roots.add(cert).map_err(|e| {
                SwarmError::ConfigError(format!("TLS root cert error: {e}"))
            })?;
        }
        let tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Self::connect_tls(connection_string, MakeRustlsConnect::new(tls_config)).await
    }

    async fn run_migrations(&self) -> SwarmResult<()> {
        self.client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version TEXT PRIMARY KEY,
                    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                )",
            )
            .await
            .map_err(pg_err)?;

        for (version, migration) in MIGRATIONS {
            let applied = self
                .client
                .query_opt(
                    "SELECT version FROM schema_migrations WHERE version = $1",
                    &[version],
                )
                .await
                .map_err(pg_err)?;
            if applied.is_none() {
                self.client.batch_execute(migration).await.map_err(pg_err)?;
                self.client
                    .execute(
                        "INSERT INTO schema_migrations (version) VALUES ($1)
                         ON CONFLICT (version) DO NOTHING",
                        &[version],
                    )
                    .await
                    .map_err(pg_err)?;
            }
        }

        Ok(())
    }
}

fn pg_err(error: tokio_postgres::Error) -> SwarmError {
    SwarmError::Other(format!("PostgreSQL error: {}", error))
}

#[async_trait]
impl SessionStore for PostgresStore {
    async fn create_session(
        &self,
        session_id: &str,
        agent_name: &str,
        trace_id: &str,
    ) -> SwarmResult<()> {
        self.client
            .execute(
                "INSERT INTO sessions (session_id, agent_name, trace_id, started_at)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (session_id) DO NOTHING",
                &[&session_id, &agent_name, &trace_id, &Utc::now()],
            )
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> SwarmResult<Option<SessionRecord>> {
        let row = self
            .client
            .query_opt(
                "SELECT session_id, agent_name, trace_id, started_at, ended_at, outcome
                 FROM sessions
                 WHERE session_id = $1",
                &[&session_id],
            )
            .await
            .map_err(pg_err)?;

        Ok(row.map(|row| SessionRecord {
            session_id: row.get(0),
            agent_name: row.get(1),
            trace_id: row.get(2),
            started_at: row.get(3),
            ended_at: row.get(4),
            outcome: row.get(5),
        }))
    }

    async fn list_sessions(&self, limit: usize, offset: usize) -> SwarmResult<Vec<SessionRecord>> {
        let rows = self
            .client
            .query(
                "SELECT session_id, agent_name, trace_id, started_at, ended_at, outcome
                 FROM sessions
                 ORDER BY started_at DESC
                 LIMIT $1 OFFSET $2",
                &[&(limit as i64), &(offset as i64)],
            )
            .await
            .map_err(pg_err)?;

        Ok(rows
            .into_iter()
            .map(|row| SessionRecord {
                session_id: row.get(0),
                agent_name: row.get(1),
                trace_id: row.get(2),
                started_at: row.get(3),
                ended_at: row.get(4),
                outcome: row.get(5),
            })
            .collect())
    }

    async fn list_sessions_by_trace(&self, trace_id: &str) -> SwarmResult<Vec<SessionRecord>> {
        let rows = self
            .client
            .query(
                "SELECT session_id, agent_name, trace_id, started_at, ended_at, outcome
                 FROM sessions
                 WHERE trace_id = $1
                 ORDER BY started_at DESC",
                &[&trace_id],
            )
            .await
            .map_err(pg_err)?;

        Ok(rows
            .into_iter()
            .map(|row| SessionRecord {
                session_id: row.get(0),
                agent_name: row.get(1),
                trace_id: row.get(2),
                started_at: row.get(3),
                ended_at: row.get(4),
                outcome: row.get(5),
            })
            .collect())
    }

    async fn complete_session(&self, session_id: &str, outcome: &str) -> SwarmResult<()> {
        self.client
            .execute(
                "UPDATE sessions
                 SET ended_at = $1, outcome = $2
                 WHERE session_id = $3",
                &[&Utc::now(), &outcome, &session_id],
            )
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    async fn store_messages(&self, session_id: &str, messages: &[Message]) -> SwarmResult<()> {
        // Serialize all messages first so a serialization failure leaves the DB untouched.
        let payloads: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::to_value(m)
                    .map_err(|error| SwarmError::SerializationError(error.to_string()))
            })
            .collect::<SwarmResult<_>>()?;

        if payloads.is_empty() {
            self.client
                .execute("DELETE FROM messages WHERE session_id = $1", &[&session_id])
                .await
                .map_err(pg_err)?;
            return Ok(());
        }

        let positions: Vec<i64> = (0..payloads.len() as i64).collect();
        // A writeable CTE executes the DELETE and INSERT as a single atomic statement —
        // no explicit transaction needed and Arc<Client> requires no structural change.
        self.client
            .execute(
                "WITH del AS (DELETE FROM messages WHERE session_id = $1)
                 INSERT INTO messages (session_id, position, payload)
                 SELECT $1, pos, payload
                 FROM UNNEST($2::bigint[], $3::jsonb[]) AS t(pos, payload)",
                &[&session_id, &positions, &payloads],
            )
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    async fn load_messages(&self, session_id: &str) -> SwarmResult<Vec<Message>> {
        let rows = self
            .client
            .query(
                "SELECT payload
                 FROM messages
                 WHERE session_id = $1
                 ORDER BY position ASC",
                &[&session_id],
            )
            .await
            .map_err(pg_err)?;

        rows.into_iter()
            .map(|row| {
                let payload: serde_json::Value = row.get(0);
                serde_json::from_value(payload)
                    .map_err(|error| SwarmError::DeserializationError(error.to_string()))
            })
            .collect()
    }
}

#[async_trait]
impl EventStore for PostgresStore {
    async fn append_event(&self, session_id: &str, event: &AgentEvent) -> SwarmResult<()> {
        let event_type = format!("{}", event)
            .split('(')
            .next()
            .unwrap_or("unknown")
            .to_string();
        let payload = serde_json::to_value(event)
            .map_err(|error| SwarmError::SerializationError(error.to_string()))?;
        self.client
            .execute(
                "INSERT INTO events (session_id, event_type, payload, timestamp)
                 VALUES ($1, $2, $3, $4)",
                &[&session_id, &event_type, &payload, &event.timestamp()],
            )
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    async fn read_events(&self, session_id: &str) -> SwarmResult<Vec<AgentEvent>> {
        let rows = self
            .client
            .query(
                "SELECT payload
                 FROM events
                 WHERE session_id = $1
                 ORDER BY id ASC",
                &[&session_id],
            )
            .await
            .map_err(pg_err)?;

        rows.into_iter()
            .map(|row| {
                let payload: serde_json::Value = row.get(0);
                serde_json::from_value(payload)
                    .map_err(|error| SwarmError::DeserializationError(error.to_string()))
            })
            .collect()
    }

    async fn read_events_since(
        &self,
        session_id: &str,
        after: DateTime<Utc>,
    ) -> SwarmResult<Vec<AgentEvent>> {
        let rows = self
            .client
            .query(
                "SELECT payload
                 FROM events
                 WHERE session_id = $1 AND timestamp > $2
                 ORDER BY id ASC",
                &[&session_id, &after],
            )
            .await
            .map_err(pg_err)?;

        rows.into_iter()
            .map(|row| {
                let payload: serde_json::Value = row.get(0);
                serde_json::from_value(payload)
                    .map_err(|error| SwarmError::DeserializationError(error.to_string()))
            })
            .collect()
    }

    async fn count_events(&self, session_id: &str) -> SwarmResult<u64> {
        let row = self
            .client
            .query_one(
                "SELECT COUNT(*)
                 FROM events
                 WHERE session_id = $1",
                &[&session_id],
            )
            .await
            .map_err(pg_err)?;
        let count: i64 = row.get(0);
        Ok(count as u64)
    }
}

#[async_trait]
impl CheckpointStore for PostgresStore {
    async fn save_checkpoint(&self, envelope: &CheckpointEnvelope) -> SwarmResult<()> {
        let payload = serde_json::to_value(envelope)
            .map_err(|error| SwarmError::SerializationError(error.to_string()))?;
        self.client
            .execute(
                "INSERT INTO checkpoints (session_id, version, payload, created_at)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (session_id, version)
                 DO UPDATE SET payload = EXCLUDED.payload, created_at = EXCLUDED.created_at",
                &[
                    &envelope.session_id,
                    &(envelope.version as i32),
                    &payload,
                    &envelope.created_at,
                ],
            )
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    async fn load_checkpoint(&self, session_id: &str) -> SwarmResult<Option<CheckpointEnvelope>> {
        let row = self
            .client
            .query_opt(
                "SELECT payload
                 FROM checkpoints
                 WHERE session_id = $1
                 ORDER BY version DESC
                 LIMIT 1",
                &[&session_id],
            )
            .await
            .map_err(pg_err)?;

        match row {
            Some(row) => {
                let payload: serde_json::Value = row.get(0);
                let json = serde_json::to_string(&payload)
                    .map_err(|error| SwarmError::SerializationError(error.to_string()))?;
                Ok(Some(CheckpointEnvelope::from_json(&json)?))
            }
            None => Ok(None),
        }
    }

    async fn load_checkpoint_at_version(
        &self,
        session_id: &str,
        version: u32,
    ) -> SwarmResult<Option<CheckpointEnvelope>> {
        let row = self
            .client
            .query_opt(
                "SELECT payload
                 FROM checkpoints
                 WHERE session_id = $1 AND version = $2",
                &[&session_id, &(version as i32)],
            )
            .await
            .map_err(pg_err)?;

        match row {
            Some(row) => {
                let payload: serde_json::Value = row.get(0);
                let json = serde_json::to_string(&payload)
                    .map_err(|error| SwarmError::SerializationError(error.to_string()))?;
                Ok(Some(CheckpointEnvelope::from_json(&json)?))
            }
            None => Ok(None),
        }
    }

    async fn list_checkpoints(&self, session_id: &str) -> SwarmResult<Vec<CheckpointSummary>> {
        let rows = self
            .client
            .query(
                "SELECT session_id, version, created_at
                 FROM checkpoints
                 WHERE session_id = $1
                 ORDER BY version DESC",
                &[&session_id],
            )
            .await
            .map_err(pg_err)?;

        Ok(rows
            .into_iter()
            .map(|row| CheckpointSummary {
                session_id: row.get(0),
                version: row.get::<_, i32>(1) as u32,
                created_at: row.get(2),
            })
            .collect())
    }

    async fn delete_checkpoints(&self, session_id: &str) -> SwarmResult<()> {
        self.client
            .execute(
                "DELETE FROM checkpoints WHERE session_id = $1",
                &[&session_id],
            )
            .await
            .map_err(pg_err)?;
        Ok(())
    }
}

#[async_trait]
impl MemoryStore for PostgresStore {
    async fn persist_memory(&self, session_id: &str, key: &str, value: &str) -> SwarmResult<()> {
        let now = Utc::now();
        self.client
            .execute(
                "INSERT INTO memory (session_id, key, value, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $4)
                 ON CONFLICT (session_id, key)
                 DO UPDATE SET value = EXCLUDED.value, updated_at = EXCLUDED.updated_at",
                &[&session_id, &key, &value, &now],
            )
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    async fn restore_memory(&self, session_id: &str) -> SwarmResult<Vec<MemoryRecord>> {
        let rows = self
            .client
            .query(
                "SELECT session_id, key, value, created_at, updated_at
                 FROM memory
                 WHERE session_id = $1
                 ORDER BY key ASC",
                &[&session_id],
            )
            .await
            .map_err(pg_err)?;

        Ok(rows
            .into_iter()
            .map(|row| MemoryRecord {
                session_id: row.get(0),
                key: row.get(1),
                value: row.get(2),
                created_at: row.get(3),
                updated_at: row.get(4),
            })
            .collect())
    }

    async fn delete_memory(&self, session_id: &str) -> SwarmResult<()> {
        self.client
            .execute("DELETE FROM memory WHERE session_id = $1", &[&session_id])
            .await
            .map_err(pg_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn test_connect_rejects_invalid_connection_string() {
        let result = PostgresStore::connect("postgres://invalid host").await;
        assert!(result.is_err());
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn test_connect_rejects_remote_no_tls() {
        let err = PostgresStore::connect("postgres://user:pass@db.example.com/mydb")
            .await
            .expect_err("remote NoTls must be rejected");
        assert!(
            err.to_string().contains("NoTls"),
            "error should mention NoTls: {err}"
        );
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn test_roundtrip_when_test_database_url_is_configured() {
        let Some(database_url) = std::env::var("TEST_DATABASE_URL").ok() else {
            return;
        };

        let store = PostgresStore::connect(&database_url)
            .await
            .expect("postgres store should connect");
        let session_id = format!("pg-test-{}", uuid::Uuid::new_v4());

        store
            .create_session(&session_id, "agent-a", "trace-a")
            .await
            .expect("session should be created");
        store
            .store_messages(
                &session_id,
                &[Message::user("hello").expect("valid message")],
            )
            .await
            .expect("messages should persist");
        store
            .persist_memory(&session_id, "summary", "hello world")
            .await
            .expect("memory should persist");

        let loaded_messages = store
            .load_messages(&session_id)
            .await
            .expect("messages should load");
        let loaded_memory = store
            .restore_memory(&session_id)
            .await
            .expect("memory should load");

        assert_eq!(loaded_messages.len(), 1);
        assert_eq!(loaded_memory.len(), 1);
    }
}
