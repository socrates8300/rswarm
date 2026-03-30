//! Versioned checkpoint envelope and serialization for session resume.
//!
//! A `CheckpointEnvelope` is a self-describing, migration-ready snapshot of
//! all state needed to resume an interrupted agent session. The `version`
//! field allows future schema changes to be detected and handled gracefully.

use crate::error::{SwarmError, SwarmResult};
use crate::phase::TokenUsage;
use crate::types::{ContextVariables, Message};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The checkpoint format version produced by this build.
///
/// Bump this constant when the `CheckpointData` layout changes in an
/// incompatible way so that `CheckpointEnvelope::is_compatible` can reject
/// stale checkpoints.
pub const CURRENT_CHECKPOINT_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// CheckpointData — the inner payload
// ---------------------------------------------------------------------------

/// The complete agent state captured at a loop boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckpointData {
    /// Full message history at the time of the checkpoint.
    pub messages: Vec<Message>,

    /// Context variables in effect at the time of the checkpoint.
    pub context_variables: ContextVariables,

    /// Name of the active agent when the checkpoint was taken.
    pub current_agent: String,

    /// Zero-based iteration index of the loop at the checkpoint boundary.
    pub iteration: u32,

    /// Cumulative token usage up to the checkpoint.
    pub token_usage: TokenUsage,
}

impl CheckpointData {
    pub fn new(
        messages: Vec<Message>,
        context_variables: ContextVariables,
        current_agent: impl Into<String>,
        iteration: u32,
        token_usage: TokenUsage,
    ) -> Self {
        Self {
            messages,
            context_variables,
            current_agent: current_agent.into(),
            iteration,
            token_usage,
        }
    }
}

// ---------------------------------------------------------------------------
// CheckpointEnvelope — the outer versioned wrapper
// ---------------------------------------------------------------------------

/// A versioned, self-describing checkpoint suitable for persistence and resume.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckpointEnvelope {
    /// Format version. Must equal [`CURRENT_CHECKPOINT_VERSION`] to be
    /// considered compatible with this build.
    pub version: u32,

    /// The session this checkpoint belongs to.
    pub session_id: String,

    /// Wall-clock timestamp when the checkpoint was captured.
    pub created_at: DateTime<Utc>,

    /// The captured agent state.
    pub payload: CheckpointData,
}

impl CheckpointEnvelope {
    /// Create a new envelope at [`CURRENT_CHECKPOINT_VERSION`].
    pub fn new(session_id: impl Into<String>, payload: CheckpointData) -> Self {
        Self {
            version: CURRENT_CHECKPOINT_VERSION,
            session_id: session_id.into(),
            created_at: Utc::now(),
            payload,
        }
    }

    /// Returns `true` if this envelope's version matches the current build.
    ///
    /// An incompatible envelope should not be resumed without a migration path.
    pub fn is_compatible(&self) -> bool {
        self.version == CURRENT_CHECKPOINT_VERSION
    }

    /// Validate that the envelope can be resumed safely.
    ///
    /// Returns a structured error if the version is incompatible or the
    /// session ID is empty.
    pub fn validate(&self) -> SwarmResult<()> {
        if self.session_id.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "CheckpointEnvelope session_id cannot be empty".to_string(),
            ));
        }
        if !self.is_compatible() {
            return Err(SwarmError::Other(format!(
                "Checkpoint version {} is incompatible with current version {}; \
                 manual migration required",
                self.version, CURRENT_CHECKPOINT_VERSION
            )));
        }
        if self.payload.current_agent.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "CheckpointData current_agent cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    /// Serialize the envelope to a JSON string for opaque storage.
    pub fn to_json(&self) -> SwarmResult<String> {
        serde_json::to_string(self).map_err(|e| {
            SwarmError::SerializationError(format!("checkpoint serialization failed: {}", e))
        })
    }

    /// Deserialize an envelope from a JSON string.
    pub fn from_json(s: &str) -> SwarmResult<Self> {
        serde_json::from_str(s).map_err(|e| {
            SwarmError::DeserializationError(format!("checkpoint deserialization failed: {}", e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MessageRole;

    fn sample_payload() -> CheckpointData {
        CheckpointData::new(
            vec![Message::new(MessageRole::User, Some("hello".to_string()), None, None).unwrap()],
            ContextVariables::new(),
            "test-agent",
            3,
            TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
        )
    }

    #[test]
    fn test_roundtrip() {
        let env = CheckpointEnvelope::new("session-1", sample_payload());
        let json = env.to_json().unwrap();
        let restored = CheckpointEnvelope::from_json(&json).unwrap();
        assert_eq!(restored.session_id, "session-1");
        assert_eq!(restored.payload.iteration, 3);
        assert!(restored.is_compatible());
    }

    #[test]
    fn test_validate_empty_session() {
        let env = CheckpointEnvelope::new("", sample_payload());
        assert!(env.validate().is_err());
    }

    #[test]
    fn test_validate_version_mismatch() {
        let mut env = CheckpointEnvelope::new("s1", sample_payload());
        env.version = 999;
        assert!(env.validate().is_err());
    }

    #[test]
    fn test_validate_ok() {
        let env = CheckpointEnvelope::new("session-1", sample_payload());
        assert!(env.validate().is_ok());
    }
}
