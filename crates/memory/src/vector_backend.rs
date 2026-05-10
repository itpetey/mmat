use std::collections::HashMap;

use qdrant_client::qdrant::Value;

use crate::{error::Result, types::MemoryId};

/// A trait for vector-memory backends that store embeddings and support
/// nearest-neighbour search.
#[async_trait::async_trait]
pub trait VectorMemoryBackend: Send + Sync {
    /// Inserts or updates a vector embedding for a memory.
    async fn upsert(
        &self,
        id: MemoryId,
        embedding: Vec<f32>,
        payload: HashMap<String, Value>,
    ) -> Result<()>;

    /// Searches for the `limit` nearest neighbours to the query embedding.
    /// Returns IDs and their cosine similarity scores.
    async fn search(&self, query_embedding: Vec<f32>, limit: u64) -> Result<Vec<(MemoryId, f32)>>;

    /// Deletes the embedding for the given memory.
    async fn delete(&self, id: MemoryId) -> Result<()>;
}

/// A no-op vector backend that accepts all operations without persistence.
/// Used as a fallback when Qdrant is not configured.
#[derive(Default)]
pub struct NoopVectorBackend;

#[async_trait::async_trait]
impl VectorMemoryBackend for NoopVectorBackend {
    async fn upsert(
        &self,
        _id: MemoryId,
        _embedding: Vec<f32>,
        _payload: HashMap<String, Value>,
    ) -> Result<()> {
        Ok(())
    }

    async fn search(
        &self,
        _query_embedding: Vec<f32>,
        _limit: u64,
    ) -> Result<Vec<(MemoryId, f32)>> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: MemoryId) -> Result<()> {
        Ok(())
    }
}
