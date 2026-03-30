pub mod qdrant;
pub mod sqlite_vss;
pub mod vector;

use crate::error::{SwarmError, SwarmResult};
use async_trait::async_trait;
use std::collections::HashMap;

#[async_trait]
pub trait Memory: Send + Sync {
    async fn store(&mut self, key: &str, value: &str) -> Result<(), SwarmError>;
    async fn retrieve(&self, key: &str) -> Result<Option<String>, SwarmError>;
    async fn clear(&mut self) -> Result<(), SwarmError>;
    async fn keys(&self) -> Result<Vec<String>, SwarmError>;
}

pub struct SlidingWindowMemory {
    max_size: usize,
    storage: HashMap<String, String>,
    insertion_order: Vec<String>,
    token_estimates: HashMap<String, usize>,
}

impl SlidingWindowMemory {
    pub fn new(max_size: usize) -> SwarmResult<Self> {
        if max_size == 0 {
            return Err(SwarmError::ValidationError(
                "SlidingWindowMemory max_size must be greater than zero".to_string(),
            ));
        }
        Ok(Self {
            max_size,
            storage: HashMap::new(),
            insertion_order: Vec::new(),
            token_estimates: HashMap::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.storage.len()
    }

    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.storage.len() >= self.max_size
    }

    pub fn total_tokens(&self) -> usize {
        self.token_estimates.values().sum()
    }

    fn estimate_tokens(key: &str, value: &str) -> usize {
        key.split_whitespace().count() + value.split_whitespace().count()
    }
}

#[async_trait]
impl Memory for SlidingWindowMemory {
    async fn store(&mut self, key: &str, value: &str) -> Result<(), SwarmError> {
        let token_estimate = Self::estimate_tokens(key, value);

        if self.storage.contains_key(key) {
            self.insertion_order.retain(|k| k != key);
            self.insertion_order.push(key.to_string());
            self.storage.insert(key.to_string(), value.to_string());
            self.token_estimates.insert(key.to_string(), token_estimate);
            return Ok(());
        }

        if self.storage.len() >= self.max_size {
            if let Some(evicted_key) = self.insertion_order.first().cloned() {
                self.storage.remove(&evicted_key);
                self.token_estimates.remove(&evicted_key);
                self.insertion_order.remove(0);
            }
        }

        self.storage.insert(key.to_string(), value.to_string());
        self.insertion_order.push(key.to_string());
        self.token_estimates.insert(key.to_string(), token_estimate);

        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<String>, SwarmError> {
        Ok(self.storage.get(key).cloned())
    }

    async fn clear(&mut self) -> Result<(), SwarmError> {
        self.storage.clear();
        self.insertion_order.clear();
        self.token_estimates.clear();
        Ok(())
    }

    async fn keys(&self) -> Result<Vec<String>, SwarmError> {
        Ok(self.insertion_order.clone())
    }
}
