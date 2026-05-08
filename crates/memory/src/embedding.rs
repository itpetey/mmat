//! Embedding provider abstraction for semantic memory operations.

use crate::error::Result;

/// Produces embedding vectors for memory content and retrieval queries.
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embeds a text string into a vector suitable for the configured vector backend.
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}

/// Deterministic local embedding provider for tests and offline development.
#[derive(Debug, Clone)]
pub struct HashEmbeddingProvider {
    dimension: usize,
}

impl HashEmbeddingProvider {
    /// Creates a deterministic hash embedding provider with the given dimension.
    pub fn new(dimension: usize) -> Self {
        Self { dimension }
    }

    fn embed_with_dimension(text: &str, dimension: usize) -> Vec<f32> {
        let mut embedding = vec![0.0f32; dimension];
        for (i, word) in text.split_whitespace().take(dimension).enumerate() {
            let hash: u32 = word.bytes().enumerate().fold(0u32, |acc, (j, byte)| {
                acc.wrapping_add((byte as u32) << ((j % 4) * 8))
            });
            embedding[i] = (hash as f64 / u32::MAX as f64) as f32;
        }
        embedding
    }
}

impl Default for HashEmbeddingProvider {
    fn default() -> Self {
        Self::new(64)
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for HashEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(Self::embed_with_dimension(text, self.dimension))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_provider_produces_configured_dimension() {
        let provider = HashEmbeddingProvider::new(8);
        let embedding = provider.embed("hello world").await.unwrap();
        assert_eq!(embedding.len(), 8);
    }
}
