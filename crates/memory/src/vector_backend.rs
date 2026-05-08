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
