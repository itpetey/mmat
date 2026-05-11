use std::collections::HashMap;

use mmat_event_stream::{
    event::{RoleId, SemanticEvent, StoredArtefactRef, stable_content_hash},
    event_bus::EventBus,
};
use parking_lot::Mutex;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use crate::error::Result;

/// Artefact storage backend that stores artefact payloads in Postgres
/// (when `database_url` is configured) or falls back to an in-memory store.
pub struct ArtefactStore {
    pool: Option<PgPool>,
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

    /// Creates a Postgres-backed artefact store, running the schema migration.
    pub fn new_postgres(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_lazy(database_url)?;
        let store = Self {
            pool: Some(pool),
            memory: Mutex::new(HashMap::new()),
        };
        store.migrate()?;
        Ok(store)
    }

    /// Runs the schema migration to create the artefacts table.
    fn migrate(&self) -> Result<()> {
        let pool = self.pool.clone().expect("migrate called without pool");
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| -> Result<()> {
            rt.block_on(async {
                sqlx::query(
                    "CREATE TABLE IF NOT EXISTS artefacts (
                        artefact_id TEXT PRIMARY KEY,
                        artefact_type TEXT NOT NULL,
                        content_hash TEXT NOT NULL,
                        payload JSONB NOT NULL,
                        producer_role TEXT NOT NULL DEFAULT '',
                        created_at TEXT NOT NULL
                    )",
                )
                .execute(&pool)
                .await?;
                sqlx::query(
                    "CREATE INDEX IF NOT EXISTS idx_artefacts_type ON artefacts(artefact_type)",
                )
                .execute(&pool)
                .await?;
                sqlx::query(
                    "CREATE TABLE IF NOT EXISTS events (
                        event_id UUID PRIMARY KEY,
                        rowid BIGSERIAL NOT NULL,
                        variant TEXT NOT NULL,
                        payload JSONB NOT NULL,
                        timestamp_ns BIGINT NOT NULL,
                        source_agent TEXT NOT NULL
                    )",
                )
                .execute(&pool)
                .await?;
                sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_events_rowid ON events(rowid)")
                    .execute(&pool)
                    .await?;
                sqlx::query("CREATE INDEX IF NOT EXISTS idx_events_variant ON events(variant)")
                    .execute(&pool)
                    .await
            })
            .map_err(crate::error::Error::Postgres)?;
            Ok(())
        })?;
        Ok(())
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
        pool: &PgPool,
        artefact_type: &str,
        payload: &str,
    ) -> Result<StoredArtefactRef> {
        let artefact_id = format!("{}-{}", artefact_type, Uuid::new_v4());
        let content_hash = stable_content_hash(payload);
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO artefacts (artefact_id, artefact_type, content_hash, payload, created_at)
             VALUES ($1, $2, $3, $4::jsonb, $5)",
        )
        .bind(&artefact_id)
        .bind(artefact_type)
        .bind(&content_hash)
        .bind(payload)
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(StoredArtefactRef {
            artefact_id: artefact_id.clone(),
            content_hash,
            storage_uri: format!("db://artefacts/{artefact_id}"),
        })
    }

    fn store_memory(&self, artefact_type: &str, payload: &str) -> Result<StoredArtefactRef> {
        let artefact_id = format!("{}-{}", artefact_type, Uuid::new_v4());
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
        pool: &PgPool,
        storage_uri: &str,
    ) -> Result<Option<String>> {
        if let Some(artefact_id) = storage_uri.strip_prefix("db://artefacts/") {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT payload::text FROM artefacts WHERE artefact_id = $1")
                    .bind(artefact_id)
                    .fetch_optional(pool)
                    .await?;
            return Ok(row.map(|r| r.0));
        }

        Ok(None)
    }

    fn get_payload_memory(&self, storage_uri: &str) -> Result<Option<String>> {
        if let Some(artefact_id) = storage_uri.strip_prefix("db://artefacts/") {
            return Ok(self.memory.lock().get(artefact_id).cloned());
        }

        // Legacy inline payload format: "uri|payload"
        Ok(storage_uri
            .split_once('|')
            .map(|(_, payload)| payload.to_string()))
    }
    /// Stores an artefact and inserts a corresponding event row in a single Postgres transaction.
    ///
    /// For Postgres: both the artefact and event are inserted atomically.
    /// For file-based fallback: only the artefact is stored (no cross-store transaction possible).
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
                let artefact_id = format!("{}-{}", artefact_type, Uuid::new_v4());
                let content_hash = stable_content_hash(payload);
                let storage_uri = format!("db://artefacts/{artefact_id}");
                let now_ts = chrono::Utc::now().to_rfc3339();

                let event = SemanticEvent::new_artefact_produced_ref(
                    RoleId(source_agent.to_string()),
                    &artefact_id,
                    artefact_type,
                    &content_hash,
                    &storage_uri,
                    RoleId(producer_role.to_string()),
                    vec![],
                );
                let event_json = serde_json::to_string(&event)?;
                let variant = event.variant_name();
                let event_id = event.event_id();
                let ts_ns = match &event {
                    SemanticEvent::ArtefactProduced { timestamp_ns, .. } => *timestamp_ns,
                    _ => unreachable!(),
                };

                let mut tx = pool.begin().await?;

                sqlx::query(
                    "INSERT INTO artefacts (artefact_id, artefact_type, content_hash, payload, created_at)
                     VALUES ($1, $2, $3, $4::jsonb, $5)",
                )
                .bind(&artefact_id)
                .bind(artefact_type)
                .bind(&content_hash)
                .bind(payload)
                .bind(&now_ts)
                .execute(&mut *tx)
                .await?;

                sqlx::query(
                    "INSERT INTO events (event_id, variant, payload, timestamp_ns, source_agent)
                     VALUES ($1::uuid, $2, $3::jsonb, $4, $5)",
                )
                .bind(event_id.to_string())
                .bind(variant)
                .bind(&event_json)
                .bind(ts_ns as i64)
                .bind(source_agent)
                .execute(&mut *tx)
                .await?;

                tx.commit().await?;
                bus.broadcast_stored(event);

                Ok(StoredArtefactRef {
                    artefact_id,
                    content_hash,
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
