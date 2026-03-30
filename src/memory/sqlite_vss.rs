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
}

impl SqliteVssMemory {
    /// Create a persistent store at `db_path`.
    ///
    /// Without the `sqlite-vec` feature this silently uses an in-memory store.
    pub fn open(db_path: impl Into<String>) -> SwarmResult<Self> {
        let path = db_path.into();
        #[cfg(feature = "sqlite-vec")]
        {
            // TODO: open SQLite at `path`, load vec extension, create virtual table.
            // let conn = rusqlite::Connection::open(&path)?;
            // conn.load_extension("sqlite_vec", None)?;
            // conn.execute("CREATE VIRTUAL TABLE IF NOT EXISTS embeddings USING vec0(...)", [])?;
            let _ = &path;
        }
        #[cfg(not(feature = "sqlite-vec"))]
        tracing::debug!(
            "sqlite-vec feature disabled; SqliteVssMemory at '{}' falling back to in-memory store",
            path
        );
        Ok(Self {
            inner: InMemoryVectorStore::new(),
            db_path: Some(path),
        })
    }

    /// Create an in-memory store (no path persistence).
    pub fn in_memory() -> Self {
        Self {
            inner: InMemoryVectorStore::new(),
            db_path: None,
        }
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
}
