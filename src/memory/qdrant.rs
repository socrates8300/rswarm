//! Remote vector backend using Qdrant (task #37).
//!
//! # Activation
//!
//! Enable the `qdrant` feature in Cargo.toml and uncomment the `qdrant-client`
//! optional dependency. Then configure a [`QdrantConfig`] and call
//! [`QdrantMemory::connect`].
//!
//! Without the feature this module compiles but [`QdrantMemory::connect`]
//! returns an error explaining the feature is not enabled.

use crate::error::{SwarmError, SwarmResult};
use crate::memory::vector::{MemoryEntry, RetrievalPolicy, VectorMemory};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// QdrantConfig
// ---------------------------------------------------------------------------

/// Configuration for the Qdrant remote vector store adapter.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QdrantConfig {
    /// Qdrant gRPC/REST host (e.g. `http://localhost:6334`).
    pub host: String,
    /// Collection name to use.
    pub collection: String,
    /// Optional API key for Qdrant Cloud.
    pub api_key: Option<String>,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Embedding vector dimension (must match the collection's config).
    pub vector_dim: u32,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            host: "http://localhost:6334".to_string(),
            collection: "rswarm_memory".to_string(),
            api_key: None,
            timeout_secs: 10,
            vector_dim: 1536,
        }
    }
}

// ---------------------------------------------------------------------------
// QdrantMemory
// ---------------------------------------------------------------------------

/// Remote vector store backed by Qdrant.
///
/// Requires the `qdrant` feature and `qdrant-client` optional dependency.
pub struct QdrantMemory {
    #[allow(dead_code)]
    config: QdrantConfig,
}

impl QdrantMemory {
    fn not_implemented_error() -> SwarmError {
        SwarmError::ConfigError(
            "QdrantMemory is not implemented yet; the feature is reserved until the adapter lands"
                .to_string(),
        )
    }

    /// Connect to a Qdrant instance described by `config`.
    pub async fn connect(_config: QdrantConfig) -> SwarmResult<Self> {
        Err(Self::not_implemented_error())
    }
}

#[async_trait]
impl VectorMemory for QdrantMemory {
    async fn store(
        &self,
        _id: &str,
        _text: &str,
        _embedding: Vec<f32>,
        _metadata: Value,
    ) -> SwarmResult<()> {
        Err(Self::not_implemented_error())
    }

    async fn search(
        &self,
        _query_embedding: Vec<f32>,
        _policy: RetrievalPolicy,
    ) -> SwarmResult<Vec<MemoryEntry>> {
        Err(Self::not_implemented_error())
    }

    async fn delete(&self, _id: &str) -> SwarmResult<()> {
        Err(Self::not_implemented_error())
    }

    async fn len(&self) -> SwarmResult<usize> {
        Err(Self::not_implemented_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connect_reports_backend_unavailable() {
        let result = QdrantMemory::connect(QdrantConfig::default()).await;
        assert!(result.is_err());
    }
}
