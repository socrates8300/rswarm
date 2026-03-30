//! Persistence backend traits for sessions, events, checkpoints, and memories.
//!
//! These traits define a storage-agnostic interface. SQLite is the default
//! implementation; other backends implement the same surface without changing
//! caller code.

use crate::checkpoint::CheckpointEnvelope;
use crate::error::SwarmResult;
use crate::event::AgentEvent;
use crate::types::Message;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Supporting record types
// ---------------------------------------------------------------------------

/// Metadata for a persisted agent session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub agent_name: String,
    pub trace_id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub outcome: Option<String>,
}

/// Summary of a checkpoint stored for a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckpointSummary {
    pub session_id: String,
    pub version: u32,
    pub created_at: DateTime<Utc>,
}

/// A single persisted key/value memory entry for a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub session_id: String,
    pub key: String,
    pub value: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// SessionStore — session lifecycle and message history
// ---------------------------------------------------------------------------

/// Stores and retrieves session metadata and message history.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Create a new session record.
    async fn create_session(
        &self,
        session_id: &str,
        agent_name: &str,
        trace_id: &str,
    ) -> SwarmResult<()>;

    /// Retrieve a session by its ID.
    async fn get_session(&self, session_id: &str) -> SwarmResult<Option<SessionRecord>>;

    /// List sessions ordered by start time descending.
    async fn list_sessions(&self, limit: usize, offset: usize) -> SwarmResult<Vec<SessionRecord>>;

    /// List sessions filtered by trace ID.
    async fn list_sessions_by_trace(&self, trace_id: &str) -> SwarmResult<Vec<SessionRecord>>;

    /// Mark a session as complete with an outcome string.
    async fn complete_session(&self, session_id: &str, outcome: &str) -> SwarmResult<()>;

    /// Persist the message history for a session.
    async fn store_messages(&self, session_id: &str, messages: &[Message]) -> SwarmResult<()>;

    /// Load all messages for a session.
    async fn load_messages(&self, session_id: &str) -> SwarmResult<Vec<Message>>;
}

// ---------------------------------------------------------------------------
// EventStore — append-only event log
// ---------------------------------------------------------------------------

/// Appends and reads structured agent events.
#[async_trait]
pub trait EventStore: Send + Sync {
    /// Append a single event to the event log for a session.
    async fn append_event(&self, session_id: &str, event: &AgentEvent) -> SwarmResult<()>;

    /// Read all events for a session in chronological order.
    async fn read_events(&self, session_id: &str) -> SwarmResult<Vec<AgentEvent>>;

    /// Read events for a session after a given timestamp.
    async fn read_events_since(
        &self,
        session_id: &str,
        after: DateTime<Utc>,
    ) -> SwarmResult<Vec<AgentEvent>>;

    /// Count events stored for a session.
    async fn count_events(&self, session_id: &str) -> SwarmResult<u64>;
}

// ---------------------------------------------------------------------------
// CheckpointStore — versioned checkpoint persistence
// ---------------------------------------------------------------------------

/// Saves and loads versioned session checkpoints.
#[async_trait]
pub trait CheckpointStore: Send + Sync {
    /// Save a checkpoint envelope. Replaces any existing checkpoint at the
    /// same (session_id, version) combination.
    async fn save_checkpoint(&self, envelope: &CheckpointEnvelope) -> SwarmResult<()>;

    /// Load the most recent checkpoint for a session, or `None` if absent.
    async fn load_checkpoint(&self, session_id: &str) -> SwarmResult<Option<CheckpointEnvelope>>;

    /// Load a specific versioned checkpoint, or `None` if that version does
    /// not exist.
    async fn load_checkpoint_at_version(
        &self,
        session_id: &str,
        version: u32,
    ) -> SwarmResult<Option<CheckpointEnvelope>>;

    /// List checkpoint summaries for a session, newest first.
    async fn list_checkpoints(&self, session_id: &str) -> SwarmResult<Vec<CheckpointSummary>>;

    /// Delete all checkpoints for a session.
    async fn delete_checkpoints(&self, session_id: &str) -> SwarmResult<()>;
}

// ---------------------------------------------------------------------------
// MemoryStore — key/value memory persistence
// ---------------------------------------------------------------------------

/// Persists and restores the key/value memory associated with a session.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Upsert a single memory entry for a session.
    async fn persist_memory(&self, session_id: &str, key: &str, value: &str) -> SwarmResult<()>;

    /// Load all memory entries for a session.
    async fn restore_memory(&self, session_id: &str) -> SwarmResult<Vec<MemoryRecord>>;

    /// Delete all memory entries for a session.
    async fn delete_memory(&self, session_id: &str) -> SwarmResult<()>;
}

// ---------------------------------------------------------------------------
// PersistenceBackend — convenience aggregate trait
// ---------------------------------------------------------------------------

/// A single object that implements all four persistence traits.
///
/// Implementations can delegate to the same underlying store or compose
/// multiple distinct stores.
pub trait PersistenceBackend:
    SessionStore + EventStore + CheckpointStore + MemoryStore + Send + Sync
{
}

// Blanket impl: anything that implements all four traits automatically
// implements PersistenceBackend.
impl<T> PersistenceBackend for T where
    T: SessionStore + EventStore + CheckpointStore + MemoryStore + Send + Sync
{
}

#[cfg(feature = "postgres")]
pub mod postgres;
pub mod sqlite;
