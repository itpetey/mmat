//! Qdrant vector-memory backend implementation.

use std::collections::HashMap;

use qdrant_client::{
    Qdrant,
    qdrant::{
        CreateCollectionBuilder, DeletePointsBuilder, Distance, PointId, PointStruct,
        SearchPointsBuilder, Value, VectorParams, VectorsConfig,
    },
};
use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, Result},
    types::MemoryId,
    vector_backend::VectorMemoryBackend,
};

/// Configuration for connecting to a Qdrant vector database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QdrantMemoryConfig {
    /// URL of the Qdrant server.
    pub url: String,
    /// Optional API key for authentication.
    pub api_key: Option<String>,
    /// Name of the Qdrant collection to use.
    pub collection_name: String,
    /// Dimensionality of the vectors stored in the collection.
    pub vector_dimension: u64,
}

/// A [`VectorMemoryBackend`] backed by a Qdrant vector database.
pub struct QdrantMemoryBackend {
    client: Qdrant,
    collection_name: String,
}

/// Defaults to connecting to `http://localhost:6334`, the `"memories"` collection,
/// and a vector dimension of 64.
impl Default for QdrantMemoryConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:6334".to_string(),
            api_key: None,
            collection_name: "memories".to_string(),
            vector_dimension: 64,
        }
    }
}

impl QdrantMemoryBackend {
    /// Creates a new Qdrant backend, connecting to the configured server and
    /// creating the collection if it does not exist.
    pub async fn new(config: QdrantMemoryConfig) -> Result<Self> {
        let mut client_builder = Qdrant::from_url(&config.url);
        if let Some(ref api_key) = config.api_key {
            client_builder = client_builder.api_key(api_key.clone());
        }
        let client = client_builder
            .build()
            .map_err(|e| Error::Qdrant(e.to_string()))?;

        let collection_exists = client
            .collection_exists(&config.collection_name)
            .await
            .map_err(|e| Error::Qdrant(e.to_string()))?;

        if !collection_exists {
            client
                .create_collection(
                    CreateCollectionBuilder::new(&config.collection_name).vectors_config(
                        VectorsConfig::from(VectorParams {
                            size: config.vector_dimension,
                            distance: Distance::Cosine.into(),
                            ..Default::default()
                        }),
                    ),
                )
                .await
                .map_err(|e| Error::Qdrant(e.to_string()))?;
        }

        Ok(Self {
            client,
            collection_name: config.collection_name,
        })
    }

    /// Inserts or updates a point in the Qdrant collection.
    pub async fn upsert(
        &self,
        id: MemoryId,
        embedding: Vec<f32>,
        payload: HashMap<String, Value>,
    ) -> Result<()> {
        let point = PointStruct::new(id.0.to_string(), embedding, payload);

        self.client
            .upsert_points(qdrant_client::qdrant::UpsertPointsBuilder::new(
                &self.collection_name,
                vec![point],
            ))
            .await
            .map_err(|e| Error::Qdrant(e.to_string()))?;

        Ok(())
    }

    /// Searches the collection for the nearest neighbours to the query embedding.
    pub async fn search(
        &self,
        query_embedding: Vec<f32>,
        limit: u64,
    ) -> Result<Vec<(MemoryId, f32)>> {
        let results = self
            .client
            .search_points(
                SearchPointsBuilder::new(&self.collection_name, query_embedding, limit)
                    .with_payload(false)
                    .with_vectors(false),
            )
            .await
            .map_err(|e| Error::Qdrant(e.to_string()))?;

        let scored = results
            .result
            .into_iter()
            .filter_map(|point| {
                let point_id = point.id?;
                let id_str = match point_id.point_id_options {
                    Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)) => n.to_string(),
                    Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(uuid)) => uuid,
                    _ => return None,
                };
                let uuid = uuid::Uuid::parse_str(&id_str).ok()?;
                Some((MemoryId(uuid), point.score))
            })
            .collect();

        Ok(scored)
    }

    /// Deletes a point from the Qdrant collection.
    pub async fn delete(&self, id: MemoryId) -> Result<()> {
        self.client
            .delete_points(
                DeletePointsBuilder::new(&self.collection_name)
                    .points([PointId::from(id.0.to_string())]),
            )
            .await
            .map_err(|e| Error::Qdrant(e.to_string()))?;
        Ok(())
    }

    #[cfg(test)]
    pub fn client(&self) -> &Qdrant {
        &self.client
    }
}

#[async_trait::async_trait]
impl VectorMemoryBackend for QdrantMemoryBackend {
    async fn upsert(
        &self,
        id: MemoryId,
        embedding: Vec<f32>,
        payload: HashMap<String, Value>,
    ) -> Result<()> {
        QdrantMemoryBackend::upsert(self, id, embedding, payload).await
    }

    async fn search(&self, query_embedding: Vec<f32>, limit: u64) -> Result<Vec<(MemoryId, f32)>> {
        QdrantMemoryBackend::search(self, query_embedding, limit).await
    }

    async fn delete(&self, id: MemoryId) -> Result<()> {
        QdrantMemoryBackend::delete(self, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let config = QdrantMemoryConfig::default();
        assert_eq!(config.url, "http://localhost:6334");
        assert_eq!(config.collection_name, "memories");
        assert_eq!(config.vector_dimension, 64);
        assert!(config.api_key.is_none());
    }
}
