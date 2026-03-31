//! Embedded vector backend using sqlite-vec (task #36).
//!
//! # Activation
//!
//! Enable the `sqlite-vec` feature in Cargo.toml and uncomment
//! the `sqlite_vec` optional dependency.
//!
//! Without the feature, this module exposes [`SqliteVssMemory`] as a
//! **transparent fallback** to [`InMemoryVectorStore`] so callers compile
//! and work identically — they just don't get on-disk persistence.
//!
//! # Feature-enabled path (not yet wired)
//!
//! With `features = ["sqlite-vec"]` and the `sqlite_vec` dep uncommented:
//! - Opens (or creates) a SQLite database at the given path.
//! - Loads the `sqlite-vec` extension.
//! - Stores embeddings in a virtual `vec0` table.
//! - `search()` issues a KNN query against the table.

use crate::error::SwarmResult;
use crate::memory::vector::{InMemoryVectorStore, MemoryEntry, RetrievalPolicy, VectorMemory};
use async_trait::async_trait;
use serde_json::Value;

/// Embedded vector store backed by sqlite-vec.
///
/// Falls back to [`InMemoryVectorStore`] when the `sqlite-vec` feature is
/// disabled, so downstream code compiles and runs without the native extension.
pub struct SqliteVssMemory {
    inner: InMemoryVectorStore,
    #[allow(dead_code)]
    db_path: Option<String>,
    persistent: bool,
}

impl SqliteVssMemory {
    /// Create a persistent store at `db_path`.
    ///
    /// Requires the `sqlite-vec` feature. Returns a [`SwarmError::ConfigError`]
    /// in all other cases — the adapter is not yet implemented, or the feature
    /// is disabled. Use [`SqliteVssMemory::in_memory`] for a non-persistent store.
    pub fn open(db_path: impl Into<String>) -> SwarmResult<Self> {
        let path = db_path.into();
        #[cfg(feature = "sqlite-vec")]
        {
            Err(crate::error::SwarmError::ConfigError(format!(
                "sqlite-vec support for '{}' is not implemented yet; \
                 use SqliteVssMemory::in_memory() or disable the feature",
                path
            )))
        }
        #[cfg(not(feature = "sqlite-vec"))]
        {
            Err(crate::error::SwarmError::ConfigError(format!(
                "SqliteVssMemory::open('{}') requires the 'sqlite-vec' feature; \
                 use SqliteVssMemory::in_memory() for a non-persistent store",
                path
            )))
        }
    }

    /// Create an in-memory store (no path persistence).
    pub fn in_memory() -> Self {
        Self {
            inner: InMemoryVectorStore::new(),
            db_path: None,
            persistent: false,
        }
    }

    /// Returns `true` if this store was opened with the `sqlite-vec` feature
    /// enabled and is actually persisting data to disk.
    ///
    /// When `false`, all writes are in-memory only and will be lost on exit.
    pub fn is_persistent(&self) -> bool {
        self.persistent
    }
}

#[async_trait]
impl VectorMemory for SqliteVssMemory {
    async fn store(
        &self,
        id: &str,
        text: &str,
        embedding: Vec<f32>,
        metadata: Value,
    ) -> SwarmResult<()> {
        self.inner.store(id, text, embedding, metadata).await
    }

    async fn search(
        &self,
        query_embedding: Vec<f32>,
        policy: RetrievalPolicy,
    ) -> SwarmResult<Vec<MemoryEntry>> {
        self.inner.search(query_embedding, policy).await
    }

    async fn delete(&self, id: &str) -> SwarmResult<()> {
        self.inner.delete(id).await
    }

    async fn len(&self) -> SwarmResult<usize> {
        self.inner.len().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vec2(x: f32, y: f32) -> Vec<f32> {
        vec![x, y]
    }

    #[tokio::test]
    async fn test_fallback_store_and_search() {
        let store = SqliteVssMemory::in_memory();
        store
            .store("a", "hello", vec2(1.0, 0.0), json!({}))
            .await
            .unwrap();
        let results = store
            .search(vec2(1.0, 0.0), RetrievalPolicy::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a");
    }

    #[cfg(not(feature = "sqlite-vec"))]
    #[test]
    fn test_open_requires_feature() {
        let err = match SqliteVssMemory::open("memory.db") {
            Ok(_) => panic!("open without feature must fail"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("sqlite-vec' feature"));
    }

    #[cfg(feature = "sqlite-vec")]
    #[test]
    fn test_open_returns_error_until_backend_is_implemented() {
        let error = match SqliteVssMemory::open("memory.db") {
            Ok(_) => panic!("feature path is not implemented"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("not implemented yet"));
    }
}
