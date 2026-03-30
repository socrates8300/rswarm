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
    /// Connect to a Qdrant instance described by `config`.
    ///
    /// Returns `Err` when the `qdrant` feature is not enabled.
    pub async fn connect(_config: QdrantConfig) -> SwarmResult<Self> {
        #[cfg(feature = "qdrant")]
        {
            // TODO: wire qdrant-client once dep is uncommented.
            // use qdrant_client::client::QdrantClient;
            // let client = QdrantClient::from_url(&_config.host)
            //     .with_api_key(_config.api_key.clone())
            //     .build()?;
            let _ = &_config;
        }
        #[cfg(not(feature = "qdrant"))]
        return Err(SwarmError::ConfigError(
            "QdrantMemory requires the `qdrant` feature: \
             add `features = [\"qdrant\"]` to your Cargo dependency and \
             uncomment the qdrant-client dep in Cargo.toml"
                .to_string(),
        ));

        #[cfg(feature = "qdrant")]
        Ok(Self { config: _config })
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
        #[cfg(feature = "qdrant")]
        {
            // TODO: client.upsert_points(...)
        }
        Err(SwarmError::ConfigError(
            "qdrant feature not enabled".to_string(),
        ))
    }

    async fn search(
        &self,
        _query_embedding: Vec<f32>,
        _policy: RetrievalPolicy,
    ) -> SwarmResult<Vec<MemoryEntry>> {
        #[cfg(feature = "qdrant")]
        {
            // TODO: client.search_points(...)
        }
        Err(SwarmError::ConfigError(
            "qdrant feature not enabled".to_string(),
        ))
    }

    async fn delete(&self, _id: &str) -> SwarmResult<()> {
        Err(SwarmError::ConfigError(
            "qdrant feature not enabled".to_string(),
        ))
    }

    async fn len(&self) -> SwarmResult<usize> {
        Err(SwarmError::ConfigError(
            "qdrant feature not enabled".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connect_fails_without_feature() {
        let result = QdrantMemory::connect(QdrantConfig::default()).await;
        // Without the `qdrant` feature this must return an error.
        #[cfg(not(feature = "qdrant"))]
        assert!(result.is_err());
        // With the feature it would succeed (or fail to connect to localhost).
        #[cfg(feature = "qdrant")]
        let _ = result;
    }
}
