use std::collections::HashMap;

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
}

impl ArtefactStore {
    /// Creates an in-memory artefact store (no persistence).
    pub fn new() -> Self {
        Self {
            pool: None,
            memory: Mutex::new(HashMap::new()),
        }
    }

    /// Creates a Postgres-backed artefact store.
    pub fn new_postgres(database_url: &str) -> Result<Self> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        let pool = tokio::task::block_in_place(|| rt.block_on(mmat_db::new_pool(database_url)))
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string())))?;
        Ok(Self {
            pool: Some(pool),
            memory: Mutex::new(HashMap::new()),
        })
    }

    /// Stores an artefact payload and returns a reference with a `db://` URI.
    pub async fn store(&self, artefact_type: &str, payload: &str) -> Result<StoredArtefactRef> {
        match &self.pool {
            Some(pool) => self.store_postgres(pool, artefact_type, payload).await,
            None => self.store_memory(artefact_type, payload),
        }
    }

    /// Retrieves the payload for a given storage URI.
    ///
    /// Supports `db://artefacts/<id>` URIs and legacy inline `type|payload` URIs.
    pub async fn get_payload(&self, storage_uri: &str) -> Result<Option<String>> {
        match &self.pool {
            Some(pool) => self.get_payload_postgres(pool, storage_uri).await,
            None => self.get_payload_memory(storage_uri),
        }
    }

    async fn store_postgres(
        &self,
        pool: &mmat_db::Pool<AsyncPgConnection>,
        artefact_type: &str,
        payload: &str,
    ) -> Result<StoredArtefactRef> {
        let artefact_id = Uuid::new_v4();
        let id_str = artefact_id.to_string();
        let content_hash = stable_content_hash(payload);
        let now = chrono::Utc::now().to_rfc3339();

        let new_artefact = mmat_db::models::NewArtefact {
            id: artefact_id,
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
        mmat_db::insert_artefact(&mut conn, &new_artefact)
            .await
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;

        Ok(StoredArtefactRef {
            artefact_id: id_str.clone(),
            content_hash,
            storage_uri: format!("db://artefacts/{id_str}"),
        })
    }

    fn store_memory(&self, _artefact_type: &str, payload: &str) -> Result<StoredArtefactRef> {
        let artefact_id = Uuid::new_v4().to_string();
        let content_hash = stable_content_hash(payload);
        let storage_uri = format!("db://artefacts/{artefact_id}");
        self.memory
            .lock()
            .insert(artefact_id.clone(), payload.to_string());
        Ok(StoredArtefactRef {
            artefact_id,
            content_hash,
            storage_uri,
        })
    }

    async fn get_payload_postgres(
        &self,
        pool: &mmat_db::Pool<AsyncPgConnection>,
        storage_uri: &str,
    ) -> Result<Option<String>> {
        if let Some(id_str) = storage_uri.strip_prefix("db://artefacts/") {
            let id =
                Uuid::parse_str(id_str).map_err(|e| crate::error::Error::Store(e.to_string()))?;
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;
            let payload = mmat_db::get_artefact_payload(&mut conn, id)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            return Ok(payload);
        }

        Ok(None)
    }

    fn get_payload_memory(&self, storage_uri: &str) -> Result<Option<String>> {
        if let Some(artefact_id) = storage_uri.strip_prefix("db://artefacts/") {
            return Ok(self.memory.lock().get(artefact_id).cloned());
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
        match &self.pool {
            Some(pool) => {
                let artefact_id = Uuid::new_v4();
                let id_str = artefact_id.to_string();
                let content_hash = stable_content_hash(payload);
                let storage_uri = format!("db://artefacts/{id_str}");
                let now_ts = chrono::Utc::now().to_rfc3339();

                let event = SemanticEvent::new_artefact_produced_ref(
                    RoleId(source_agent.to_string()),
                    &id_str,
                    artefact_type,
                    &content_hash,
                    &storage_uri,
                    RoleId(producer_role.to_string()),
                    vec![],
                );
                let event_json = serde_json::to_string(&event)?;
                let variant = event.variant_name();
                let event_uuid = event.event_id().0;
                let ts_ns = match &event {
                    SemanticEvent::ArtefactProduced { timestamp_ns, .. } => *timestamp_ns,
                    _ => unreachable!(),
                };

                let new_artefact = mmat_db::models::NewArtefact {
                    id: artefact_id,
                    artefact_type: artefact_type.to_string(),
                    content_hash,
                    payload: serde_json::from_str(payload).map_err(crate::error::Error::Json)?,
                    producer_role: producer_role.to_string(),
                    created_at: now_ts,
                };

                let new_event = mmat_db::models::NewEvent {
                    id: event_uuid,
                    variant: variant.to_string(),
                    payload: serde_json::from_str(&event_json)
                        .map_err(crate::error::Error::Json)?,
                    timestamp_ns: ts_ns as i64,
                    source_agent: source_agent.to_string(),
                };

                let mut conn = pool.get().await.map_err(|e| {
                    crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
                })?;

                mmat_db::begin_transaction(&mut conn)
                    .await
                    .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;

                if let Err(e) = mmat_db::insert_artefact(&mut conn, &new_artefact).await {
                    let _ = mmat_db::rollback_transaction(&mut conn).await;
                    return Err(crate::error::Error::Database(mmat_db::DbError::Diesel(e)));
                }
                if let Err(e) = mmat_db::insert_event(&mut conn, &new_event).await {
                    let _ = mmat_db::rollback_transaction(&mut conn).await;
                    return Err(crate::error::Error::Database(mmat_db::DbError::Diesel(e)));
                }

                mmat_db::commit_transaction(&mut conn)
                    .await
                    .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;

                bus.broadcast_stored(event);

                Ok(StoredArtefactRef {
                    artefact_id: id_str,
                    content_hash: new_artefact.content_hash,
                    storage_uri,
                })
            }
            None => {
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
                Ok(stored)
            }
        }
    }
}

impl Default for ArtefactStore {
    fn default() -> Self {
        Self::new()
    }
}
