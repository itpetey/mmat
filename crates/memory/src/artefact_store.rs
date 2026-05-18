use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use mmat_db::AsyncPgConnection;
use mmat_event_stream::{
    event::{RoleId, SemanticEvent, StoredArtefactRef, stable_content_hash},
    event_bus::EventBus,
};
use parking_lot::Mutex;
use uuid::Uuid;

use crate::error::Result;

/// Artefact storage backend that stores artefact payloads in Postgres
/// (when `database_url` is configured) or falls back to an in-memory store.
pub struct ArtefactStore {
    pool: Option<mmat_db::Pool<AsyncPgConnection>>,
    memory: Mutex<HashMap<String, String>>,
    next_memory_id: AtomicU64,
}

impl ArtefactStore {
    /// Creates an in-memory artefact store for tests and non-durable contexts.
    pub fn new() -> Self {
        Self {
            pool: None,
            memory: Mutex::new(HashMap::new()),
            next_memory_id: AtomicU64::new(1),
        }
    }

    /// Creates a Postgres-backed artefact store.
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = mmat_db::new_pool(database_url)
            .await
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string())))?;
        Ok(Self {
            pool: Some(pool),
            memory: Mutex::new(HashMap::new()),
            next_memory_id: AtomicU64::new(1),
        })
    }

    /// Creates a Postgres-backed artefact store from synchronous contexts.
    pub fn new_postgres(database_url: &str) -> Result<Self> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| rt.block_on(Self::connect(database_url)))
    }

    /// Stores an artefact payload and returns a reference with a `db://` URI.
    pub async fn store(&self, artefact_type: &str, payload: &str) -> Result<StoredArtefactRef> {
        let Some(pool) = &self.pool else {
            return Ok(self.store_memory(payload));
        };

        let content_hash = stable_content_hash(payload);
        let now = chrono::Utc::now().to_rfc3339();

        let new_artefact = mmat_db::models::NewArtefact {
            artefact_type: artefact_type.to_string(),
            content_hash: content_hash.clone(),
            payload: serde_json::from_str(payload).map_err(crate::error::Error::Json)?,
            producer_role: String::new(),
            created_at: now,
        };

        let mut conn = pool
            .get()
            .await
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string())))?;
        let artefact = mmat_db::artefact::insert_artefact(&mut conn, &new_artefact)
            .await
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;

        Ok(StoredArtefactRef {
            artefact_id: artefact.id.to_string(),
            content_hash,
            storage_uri: format!("db://artefacts/{}", artefact.id),
        })
    }

    fn store_memory(&self, payload: &str) -> StoredArtefactRef {
        let artefact_id = format!(
            "memory-artefact-{}",
            self.next_memory_id.fetch_add(1, Ordering::Relaxed)
        );
        let content_hash = stable_content_hash(payload);
        let storage_uri = format!("db://artefacts/{artefact_id}");
        self.memory
            .lock()
            .insert(artefact_id.clone(), payload.to_string());
        StoredArtefactRef {
            artefact_id,
            content_hash,
            storage_uri,
        }
    }

    /// Retrieves the payload for a given storage URI.
    ///
    /// Supports `db://artefacts/<id>` URIs and legacy inline `type|payload` URIs.
    pub async fn get_payload(&self, storage_uri: &str) -> Result<Option<String>> {
        if let Some(id_str) = storage_uri.strip_prefix("db://artefacts/") {
            let Some(pool) = &self.pool else {
                return Ok(self.memory.lock().get(id_str).cloned());
            };
            let id =
                Uuid::parse_str(id_str).map_err(|e| crate::error::Error::Store(e.to_string()))?;
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;
            let payload = mmat_db::artefact::get_artefact_payload(&mut conn, id)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            return Ok(payload);
        }

        Ok(storage_uri
            .split_once('|')
            .map(|(_, payload)| payload.to_string()))
    }

    /// Stores an artefact and inserts a corresponding event row in a single Postgres transaction.
    ///
    /// For Postgres: both the artefact and event are inserted atomically.
    /// For in-memory fallback: only the artefact is stored (no cross-store transaction possible).
    pub async fn store_and_publish_event(
        &self,
        artefact_type: &str,
        payload: &str,
        source_agent: &str,
        producer_role: &str,
        bus: &EventBus,
    ) -> Result<StoredArtefactRef> {
        let Some(pool) = &self.pool else {
            let stored = self.store(artefact_type, payload).await?;
            let event = SemanticEvent::new_artefact_produced_ref(
                RoleId(source_agent.to_string()),
                stored.artefact_id.clone(),
                artefact_type,
                stored.content_hash.clone(),
                stored.storage_uri.clone(),
                RoleId(producer_role.to_string()),
                vec![],
            );
            bus.publish(event)?;
            return Ok(stored);
        };

        let content_hash = stable_content_hash(payload);
        let now_ts = chrono::Utc::now().to_rfc3339();

        let new_artefact = mmat_db::models::NewArtefact {
            artefact_type: artefact_type.to_string(),
            content_hash: content_hash.clone(),
            payload: serde_json::from_str(payload).map_err(crate::error::Error::Json)?,
            producer_role: producer_role.to_string(),
            created_at: now_ts,
        };

        let mut conn = pool
            .get()
            .await
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string())))?;

        mmat_db::begin_transaction(&mut conn)
            .await
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;

        let artefact = match mmat_db::artefact::insert_artefact(&mut conn, &new_artefact).await {
            Ok(artefact) => artefact,
            Err(e) => {
                let _ = mmat_db::rollback_transaction(&mut conn).await;
                return Err(crate::error::Error::Database(mmat_db::DbError::Diesel(e)));
            }
        };

        let storage_uri = format!("db://artefacts/{}", artefact.id);
        let event = SemanticEvent::new_artefact_produced_ref(
            RoleId(source_agent.to_string()),
            artefact.id.to_string(),
            artefact_type,
            &content_hash,
            &storage_uri,
            RoleId(producer_role.to_string()),
            vec![],
        );

        if let Err(e) = mmat_db::event::append_event(&mut conn, &event).await {
            let _ = mmat_db::rollback_transaction(&mut conn).await;
            return Err(crate::error::Error::Database(e));
        }

        mmat_db::commit_transaction(&mut conn)
            .await
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;

        bus.broadcast_stored(event);

        Ok(StoredArtefactRef {
            artefact_id: artefact.id.to_string(),
            content_hash,
            storage_uri,
        })
    }
}

impl Default for ArtefactStore {
    fn default() -> Self {
        Self::new()
    }
}
