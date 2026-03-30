//! VectorMemory trait and in-memory cosine-similarity backend.
//!
//! The trait is embedding-backend-agnostic: the caller is responsible for
//! producing float vectors. The default [`InMemoryVectorStore`] keeps all
//! entries in a `Mutex`-guarded `Vec` and performs exact nearest-neighbour
//! search with cosine similarity. It is suitable for development and for
//! small-scale production use where a full vector database is not needed.
//!
//! Production deployments can swap in the `sqlite-vec` or Qdrant adapters
//! (behind feature flags) without changing any call sites.

use crate::error::{SwarmError, SwarmResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Policy that controls how semantic search results are filtered and ranked.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetrievalPolicy {
    /// Maximum number of results to return.
    pub top_k: usize,

    /// Minimum cosine similarity score to include a result (0.0–1.0).
    pub score_threshold: f32,

    /// Weight applied to recency when re-ranking candidates.
    /// 0.0 = pure semantic score; 1.0 = most-recent wins.
    pub recency_weight: f32,
}

impl Default for RetrievalPolicy {
    fn default() -> Self {
        Self {
            top_k: 5,
            score_threshold: 0.0,
            recency_weight: 0.0,
        }
    }
}

impl RetrievalPolicy {
    pub fn new(top_k: usize, score_threshold: f32, recency_weight: f32) -> SwarmResult<Self> {
        if !(0.0..=1.0).contains(&score_threshold) {
            return Err(SwarmError::ValidationError(
                "score_threshold must be in [0.0, 1.0]".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&recency_weight) {
            return Err(SwarmError::ValidationError(
                "recency_weight must be in [0.0, 1.0]".to_string(),
            ));
        }
        Ok(Self {
            top_k,
            score_threshold,
            recency_weight,
        })
    }
}

/// A single stored memory entry returned from a search.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Stable identifier for this entry.
    pub id: String,

    /// The original text that was embedded.
    pub text: String,

    /// The embedding vector stored with the entry.
    pub embedding: Vec<f32>,

    /// Arbitrary structured metadata (e.g., session id, tags, source).
    pub metadata: Value,

    /// Wall-clock time the entry was stored.
    pub stored_at: DateTime<Utc>,

    /// Cosine similarity score relative to the query, populated by `search`.
    pub score: f32,
}

// ---------------------------------------------------------------------------
// VectorMemory trait
// ---------------------------------------------------------------------------

/// An embedding-backed semantic memory store.
///
/// Implementations may be local (in-process) or remote (Qdrant, etc.)
/// without changing caller code. All methods take `&self` so the store can
/// be shared via `Arc<dyn VectorMemory>`.
#[async_trait]
pub trait VectorMemory: Send + Sync {
    /// Insert or replace an entry. If an entry with `id` already exists it
    /// is overwritten.
    async fn store(
        &self,
        id: &str,
        text: &str,
        embedding: Vec<f32>,
        metadata: Value,
    ) -> SwarmResult<()>;

    /// Retrieve the `top_k` entries most similar to `query_embedding` that
    /// meet the policy's score threshold, optionally re-ranked by recency.
    async fn search(
        &self,
        query_embedding: Vec<f32>,
        policy: RetrievalPolicy,
    ) -> SwarmResult<Vec<MemoryEntry>>;

    /// Delete the entry with the given ID. Returns `Ok(())` if not found.
    async fn delete(&self, id: &str) -> SwarmResult<()>;

    /// Return the number of entries in the store.
    async fn len(&self) -> SwarmResult<usize>;

    /// Return `true` when the store is empty.
    async fn is_empty(&self) -> SwarmResult<bool> {
        Ok(self.len().await? == 0)
    }
}

// ---------------------------------------------------------------------------
// InMemoryVectorStore — default exact-search backend
// ---------------------------------------------------------------------------

struct StoredEntry {
    id: String,
    text: String,
    embedding: Vec<f32>,
    metadata: Value,
    stored_at: DateTime<Utc>,
}

/// In-process vector store using exact cosine-similarity search.
///
/// Thread-safe via an internal `Mutex`. Suitable for tests, prototypes, and
/// workloads where the entry count stays below a few thousand.
pub struct InMemoryVectorStore {
    entries: Mutex<Vec<StoredEntry>>,
}

impl InMemoryVectorStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Cosine similarity between two equal-length vectors.
    /// Returns 0.0 if either vector has zero magnitude.
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if mag_a == 0.0 || mag_b == 0.0 {
            0.0
        } else {
            (dot / (mag_a * mag_b)).clamp(-1.0, 1.0)
        }
    }
}

impl Default for InMemoryVectorStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VectorMemory for InMemoryVectorStore {
    async fn store(
        &self,
        id: &str,
        text: &str,
        embedding: Vec<f32>,
        metadata: Value,
    ) -> SwarmResult<()> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|e| SwarmError::Other(format!("InMemoryVectorStore lock poisoned: {}", e)))?;
        // Upsert: replace existing entry with the same id.
        if let Some(pos) = entries.iter().position(|e| e.id == id) {
            entries[pos] = StoredEntry {
                id: id.to_string(),
                text: text.to_string(),
                embedding,
                metadata,
                stored_at: Utc::now(),
            };
        } else {
            entries.push(StoredEntry {
                id: id.to_string(),
                text: text.to_string(),
                embedding,
                metadata,
                stored_at: Utc::now(),
            });
        }
        Ok(())
    }

    async fn search(
        &self,
        query_embedding: Vec<f32>,
        policy: RetrievalPolicy,
    ) -> SwarmResult<Vec<MemoryEntry>> {
        let entries = self
            .entries
            .lock()
            .map_err(|e| SwarmError::Other(format!("InMemoryVectorStore lock poisoned: {}", e)))?;

        if entries.is_empty() {
            return Ok(vec![]);
        }

        // Score every entry.
        let now = Utc::now();
        let mut scored: Vec<(f32, usize)> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let semantic = Self::cosine_similarity(&query_embedding, &e.embedding);
                // Recency score: 1.0 for brand-new, decays toward 0 over 24 h.
                let age_secs = (now - e.stored_at).num_seconds().max(0) as f32;
                let recency = (-age_secs / 86_400.0).exp();
                let combined =
                    (1.0 - policy.recency_weight) * semantic + policy.recency_weight * recency;
                (combined, i)
            })
            .filter(|(score, _)| *score >= policy.score_threshold)
            .collect();

        // Sort descending by combined score.
        scored.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(policy.top_k);

        let results = scored
            .into_iter()
            .map(|(score, i)| {
                let e = &entries[i];
                MemoryEntry {
                    id: e.id.clone(),
                    text: e.text.clone(),
                    embedding: e.embedding.clone(),
                    metadata: e.metadata.clone(),
                    stored_at: e.stored_at,
                    score,
                }
            })
            .collect();

        Ok(results)
    }

    async fn delete(&self, id: &str) -> SwarmResult<()> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|e| SwarmError::Other(format!("InMemoryVectorStore lock poisoned: {}", e)))?;
        entries.retain(|e| e.id != id);
        Ok(())
    }

    async fn len(&self) -> SwarmResult<usize> {
        let entries = self
            .entries
            .lock()
            .map_err(|e| SwarmError::Other(format!("InMemoryVectorStore lock poisoned: {}", e)))?;
        Ok(entries.len())
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
    async fn test_store_and_search() {
        let store = InMemoryVectorStore::new();
        store
            .store("a", "text a", vec2(1.0, 0.0), json!({"tag": "a"}))
            .await
            .unwrap();
        store
            .store("b", "text b", vec2(0.0, 1.0), json!({"tag": "b"}))
            .await
            .unwrap();

        let results = store
            .search(vec2(1.0, 0.0), RetrievalPolicy::default())
            .await
            .unwrap();
        assert_eq!(results[0].id, "a");
        assert!((results[0].score - 1.0).abs() < 1e-5);
    }

    #[tokio::test]
    async fn test_upsert() {
        let store = InMemoryVectorStore::new();
        store
            .store("x", "v1", vec2(1.0, 0.0), json!({}))
            .await
            .unwrap();
        store
            .store("x", "v2", vec2(1.0, 0.0), json!({}))
            .await
            .unwrap();
        assert_eq!(store.len().await.unwrap(), 1);
        let results = store
            .search(vec2(1.0, 0.0), RetrievalPolicy::default())
            .await
            .unwrap();
        assert_eq!(results[0].text, "v2");
    }

    #[tokio::test]
    async fn test_delete() {
        let store = InMemoryVectorStore::new();
        store
            .store("d", "text", vec2(1.0, 0.0), json!({}))
            .await
            .unwrap();
        store.delete("d").await.unwrap();
        assert_eq!(store.len().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_score_threshold() {
        let store = InMemoryVectorStore::new();
        store
            .store("a", "text a", vec2(1.0, 0.0), json!({}))
            .await
            .unwrap();
        store
            .store("b", "text b", vec2(0.0, 1.0), json!({}))
            .await
            .unwrap();

        let policy = RetrievalPolicy::new(5, 0.9, 0.0).unwrap();
        let results = store.search(vec2(1.0, 0.0), policy).await.unwrap();
        // Only "a" should pass threshold 0.9
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a");
    }

    #[test]
    fn test_cosine_zero_vector() {
        assert_eq!(
            InMemoryVectorStore::cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]),
            0.0
        );
    }
}
