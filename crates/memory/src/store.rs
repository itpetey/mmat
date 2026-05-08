use std::path::Path;

use chrono::{DateTime, Utc};
use mmat_event_stream::event::{EventId, RoleId};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use crate::{
    error::Result,
    types::{Authority, Confidence, DecayPolicy, Memory, MemoryId, MemoryScope, MemoryType},
    vector_backend::VectorMemoryBackend,
};

enum MemoryStoreInner {
    Sqlite(SqliteMemoryStore),
    Postgres(PgMemoryStore),
}

pub struct MemoryStore {
    inner: MemoryStoreInner,
}

struct SqliteMemoryStore {
    conn: Mutex<Connection>,
}

pub struct PgMemoryStore {
    pool: PgPool,
}

impl MemoryStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let sqlite = SqliteMemoryStore::open(path.as_ref())?;
        Ok(Self {
            inner: MemoryStoreInner::Sqlite(sqlite),
        })
    }

    pub fn new(database_url: &str) -> Result<Self> {
        let pg = PgMemoryStore::connect_lazy(database_url)?;
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| -> Result<()> { rt.block_on(pg.migrate()) })?;
        Ok(Self {
            inner: MemoryStoreInner::Postgres(pg),
        })
    }

    pub async fn new_async(database_url: &str) -> Result<Self> {
        let pg = PgMemoryStore::connect(database_url).await?;
        Ok(Self {
            inner: MemoryStoreInner::Postgres(pg),
        })
    }

    pub fn insert(&self, memory: &Memory) -> Result<()> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.insert(memory),
            MemoryStoreInner::Postgres(s) => s.insert(memory),
        }
    }

    pub fn get_by_id(&self, id: MemoryId) -> Result<Option<Memory>> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.get_by_id(id),
            MemoryStoreInner::Postgres(s) => s.get_by_id(id),
        }
    }

    pub fn query_by_type(&self, memory_type: MemoryType) -> Result<Vec<Memory>> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.query_by_type(memory_type),
            MemoryStoreInner::Postgres(s) => s.query_by_type(memory_type),
        }
    }

    pub fn query_by_scope(&self, scope: MemoryScope) -> Result<Vec<Memory>> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.query_by_scope(scope),
            MemoryStoreInner::Postgres(s) => s.query_by_scope(scope),
        }
    }

    pub fn query_by_authority(&self, min: Authority, max: Authority) -> Result<Vec<Memory>> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.query_by_authority(min, max),
            MemoryStoreInner::Postgres(s) => s.query_by_authority(min, max),
        }
    }

    pub fn query_decayed(&self) -> Result<Vec<Memory>> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.query_decayed(),
            MemoryStoreInner::Postgres(s) => s.query_decayed(),
        }
    }

    pub fn supersede(&self, old_id: MemoryId, new_id: MemoryId) -> Result<()> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.supersede(old_id, new_id),
            MemoryStoreInner::Postgres(s) => s.supersede(old_id, new_id),
        }
    }

    pub fn get_supersession_chain(&self, id: MemoryId) -> Result<Vec<Memory>> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.get_supersession_chain(id),
            MemoryStoreInner::Postgres(s) => s.get_supersession_chain(id),
        }
    }

    pub fn query_current_only<F>(&self, query_fn: F) -> Result<Vec<Memory>>
    where
        F: Fn(&Connection) -> rusqlite::Result<Vec<Memory>>,
    {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.query_current_only(query_fn),
            MemoryStoreInner::Postgres(_) => Err(crate::error::Error::Store(
                "query_current_only is not supported via Postgres".into(),
            )),
        }
    }

    pub fn update_last_accessed(&self, id: MemoryId) -> Result<()> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.update_last_accessed(id),
            MemoryStoreInner::Postgres(s) => s.update_last_accessed(id),
        }
    }

    pub fn update_content(&self, id: MemoryId, content: &str) -> Result<()> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.update_content(id, content),
            MemoryStoreInner::Postgres(s) => s.update_content(id, content),
        }
    }

    pub async fn insert_with_embedding(
        &self,
        memory: &Memory,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<()> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.insert_with_embedding(memory, qdrant).await,
            MemoryStoreInner::Postgres(s) => s.insert_with_embedding(memory, qdrant).await,
        }
    }

    pub async fn search_similar(
        &self,
        embedding: Vec<f32>,
        limit: u64,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<Vec<(MemoryId, f32)>> {
        match &self.inner {
            MemoryStoreInner::Sqlite(s) => s.search_similar(embedding, limit, qdrant).await,
            MemoryStoreInner::Postgres(s) => s.search_similar(embedding, limit, qdrant).await,
        }
    }

    pub fn pool(&self) -> Option<&PgPool> {
        match &self.inner {
            MemoryStoreInner::Postgres(s) => Some(&s.pool),
            _ => None,
        }
    }
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryStore").finish_non_exhaustive()
    }
}

impl SqliteMemoryStore {
    fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        Self::run_migration(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn run_migration(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                memory_type TEXT NOT NULL,
                content TEXT NOT NULL,
                scope TEXT NOT NULL,
                authority TEXT NOT NULL,
                confidence REAL NOT NULL,
                decay_policy TEXT NOT NULL,
                evidence_refs TEXT NOT NULL DEFAULT '[]',
                supersedes TEXT,
                superseded_by TEXT,
                created_at TEXT NOT NULL,
                last_accessed_at TEXT NOT NULL,
                source_agent TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
            CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope);
            CREATE INDEX IF NOT EXISTS idx_memories_authority ON memories(authority);
            CREATE INDEX IF NOT EXISTS idx_memories_superseded_by ON memories(superseded_by);
            CREATE INDEX IF NOT EXISTS idx_memories_decay ON memories(decay_policy, created_at);
            ",
        )?;
        Ok(())
    }

    fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
        let id: String = row.get(0)?;
        let memory_type: String = row.get(1)?;
        let content: String = row.get(2)?;
        let scope: String = row.get(3)?;
        let authority: String = row.get(4)?;
        let confidence: f64 = row.get(5)?;
        let decay_policy: String = row.get(6)?;
        let evidence_refs_json: String = row.get(7)?;
        let supersedes: Option<String> = row.get(8)?;
        let superseded_by: Option<String> = row.get(9)?;
        let created_at: String = row.get(10)?;
        let last_accessed_at: String = row.get(11)?;
        let source_agent: String = row.get(12)?;

        row_to_memory_common(
            id,
            memory_type,
            content,
            scope,
            authority,
            confidence,
            decay_policy,
            evidence_refs_json,
            supersedes,
            superseded_by,
            created_at,
            last_accessed_at,
            source_agent,
        )
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            )
        })
    }

    fn insert(&self, memory: &Memory) -> Result<()> {
        let conn = self.conn.lock();
        let evidence_refs_json = serde_json::to_string(&memory.evidence_refs)?;
        conn.execute(
            "INSERT INTO memories (id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                memory.id.0.to_string(),
                memory.memory_type.discriminant_str(),
                memory.content,
                memory.scope.discriminant_str(),
                authority_discriminant_str(memory.authority),
                memory.confidence.value(),
                memory.decay_policy.discriminant_str(),
                evidence_refs_json,
                memory.supersedes.map(|id| id.0.to_string()),
                memory.superseded_by.map(|id| id.0.to_string()),
                memory.created_at.to_rfc3339(),
                memory.last_accessed_at.to_rfc3339(),
                memory.source_agent.0,
            ],
        )?;
        Ok(())
    }

    fn get_by_id(&self, id: MemoryId) -> Result<Option<Memory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent FROM memories WHERE id = ?1")?;
        let memory = stmt
            .query_row(params![id.0.to_string()], Self::row_to_memory)
            .optional()?;
        Ok(memory)
    }

    fn query_by_type(&self, memory_type: MemoryType) -> Result<Vec<Memory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent FROM memories WHERE memory_type = ?1 AND superseded_by IS NULL")?;
        let rows = stmt.query_map(params![memory_type.discriminant_str()], Self::row_to_memory)?;
        let mut memories = Vec::new();
        for row in rows {
            memories.push(row.map_err(|e| crate::error::Error::Store(e.to_string()))?);
        }
        Ok(memories)
    }

    fn query_by_scope(&self, scope: MemoryScope) -> Result<Vec<Memory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent FROM memories WHERE scope = ?1 AND superseded_by IS NULL")?;
        let rows = stmt.query_map(params![scope.discriminant_str()], Self::row_to_memory)?;
        let mut memories = Vec::new();
        for row in rows {
            memories.push(row.map_err(|e| crate::error::Error::Store(e.to_string()))?);
        }
        Ok(memories)
    }

    fn query_by_authority(&self, min: Authority, max: Authority) -> Result<Vec<Memory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent FROM memories WHERE superseded_by IS NULL")?;
        let rows = stmt.query_map(params![], Self::row_to_memory)?;
        let mut memories = Vec::new();
        for row in rows {
            let memory = row.map_err(|e| crate::error::Error::Store(e.to_string()))?;
            if memory.authority >= min && memory.authority <= max {
                memories.push(memory);
            }
        }
        Ok(memories)
    }

    fn query_decayed(&self) -> Result<Vec<Memory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent FROM memories WHERE superseded_by IS NULL")?;
        let rows = stmt.query_map(params![], Self::row_to_memory)?;
        let mut memories = Vec::new();
        for row in rows {
            let memory = row.map_err(|e| crate::error::Error::Store(e.to_string()))?;
            if memory.decay_policy.is_decayed(memory.created_at) {
                memories.push(memory);
            }
        }
        Ok(memories)
    }

    fn supersede(&self, old_id: MemoryId, new_id: MemoryId) -> Result<()> {
        let conn = self.conn.lock();
        let old_exists: i64 = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1)",
            params![old_id.0.to_string()],
            |row| row.get(0),
        )?;
        let new_exists: i64 = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1)",
            params![new_id.0.to_string()],
            |row| row.get(0),
        )?;

        if old_exists == 0 {
            return Err(crate::error::Error::Store(format!(
                "cannot supersede missing memory {}",
                old_id
            )));
        }
        if new_exists == 0 {
            return Err(crate::error::Error::Store(format!(
                "cannot supersede with missing memory {}",
                new_id
            )));
        }

        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE memories SET superseded_by = ?1 WHERE id = ?2",
            params![new_id.0.to_string(), old_id.0.to_string()],
        )?;
        tx.execute(
            "UPDATE memories SET supersedes = ?1 WHERE id = ?2",
            params![old_id.0.to_string(), new_id.0.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn get_supersession_chain(&self, id: MemoryId) -> Result<Vec<Memory>> {
        let mut forward = Vec::new();
        let mut current = self.get_by_id(id)?;

        while let Some(memory) = current {
            forward.push(memory.clone());
            if let Some(superseded_by) = memory.superseded_by {
                current = self.get_by_id(superseded_by)?;
            } else {
                break;
            }
        }

        let mut backward = Vec::new();
        if let Some(first) = forward.first()
            && let Some(supersedes) = first.supersedes
        {
            let mut current = self.get_by_id(supersedes)?;
            while let Some(memory) = current {
                backward.push(memory.clone());
                if let Some(s) = memory.supersedes {
                    current = self.get_by_id(s)?;
                } else {
                    break;
                }
            }
        }

        backward.reverse();
        backward.extend(forward);
        Ok(backward)
    }

    fn query_current_only<F>(&self, query_fn: F) -> Result<Vec<Memory>>
    where
        F: Fn(&Connection) -> rusqlite::Result<Vec<Memory>>,
    {
        let conn = self.conn.lock();
        let memories = query_fn(&conn)?;
        Ok(memories
            .into_iter()
            .filter(|m| m.superseded_by.is_none())
            .collect())
    }

    fn update_last_accessed(&self, id: MemoryId) -> Result<()> {
        let conn = self.conn.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET last_accessed_at = ?1 WHERE id = ?2",
            params![now, id.0.to_string()],
        )?;
        Ok(())
    }

    fn update_content(&self, id: MemoryId, content: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE memories SET content = ?1 WHERE id = ?2",
            params![content, id.0.to_string()],
        )?;
        Ok(())
    }

    async fn insert_with_embedding(
        &self,
        memory: &Memory,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<()> {
        let evidence_refs_json = serde_json::to_string(&memory.evidence_refs)?;
        let id_str = memory.id.0.to_string();
        let memory_type_str = memory.memory_type.discriminant_str();
        let content = memory.content.clone();
        let scope_str = memory.scope.discriminant_str();
        let authority_str = authority_discriminant_str(memory.authority);
        let confidence = memory.confidence.value();
        let decay_policy_str = memory.decay_policy.discriminant_str();
        let supersedes_str = memory.supersedes.map(|id| id.0.to_string());
        let superseded_by_str = memory.superseded_by.map(|id| id.0.to_string());
        let created_at = memory.created_at.to_rfc3339();
        let last_accessed_at = memory.last_accessed_at.to_rfc3339();
        let source_agent = memory.source_agent.0.clone();

        {
            let conn = self.conn.lock();
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "INSERT INTO memories (id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    id_str,
                    memory_type_str,
                    content,
                    scope_str,
                    authority_str,
                    confidence,
                    decay_policy_str,
                    evidence_refs_json,
                    supersedes_str,
                    superseded_by_str,
                    created_at,
                    last_accessed_at,
                    source_agent,
                ],
            )?;
            tx.commit()?;
        }

        if let Some(ref embedding) = memory.embedding {
            let mut payload = std::collections::HashMap::new();
            payload.insert(
                "memory_type".to_string(),
                memory.memory_type.discriminant_str().into(),
            );
            payload.insert("scope".to_string(), memory.scope.discriminant_str().into());
            payload.insert("content".to_string(), memory.content.clone().into());

            if let Err(e) = qdrant.upsert(memory.id, embedding.clone(), payload).await {
                let conn = self.conn.lock();
                conn.execute(
                    "DELETE FROM memories WHERE id = ?1",
                    params![memory.id.0.to_string()],
                )?;
                return Err(e);
            }
        }

        Ok(())
    }

    async fn search_similar(
        &self,
        embedding: Vec<f32>,
        limit: u64,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<Vec<(MemoryId, f32)>> {
        let search_limit = limit.saturating_mul(4).max(limit).max(1);
        let similar = qdrant.search(embedding, search_limit).await?;
        let mut current = Vec::new();

        for (id, score) in similar {
            if let Some(memory) = self.get_by_id(id)?
                && memory.superseded_by.is_none()
            {
                current.push((id, score));
            }

            if current.len() >= limit as usize {
                break;
            }
        }

        Ok(current)
    }
}

impl PgMemoryStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    pub fn connect_lazy(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_lazy(database_url)?;
        Ok(Self { pool })
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS memories (
                id UUID PRIMARY KEY,
                memory_type TEXT NOT NULL,
                content TEXT NOT NULL,
                scope TEXT NOT NULL,
                authority TEXT NOT NULL,
                confidence DOUBLE PRECISION NOT NULL,
                decay_policy TEXT NOT NULL,
                evidence_refs TEXT NOT NULL DEFAULT '[]',
                supersedes UUID,
                superseded_by UUID,
                created_at TEXT NOT NULL,
                last_accessed_at TEXT NOT NULL,
                source_agent TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
            CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope);
            CREATE INDEX IF NOT EXISTS idx_memories_authority ON memories(authority);
            CREATE INDEX IF NOT EXISTS idx_memories_superseded_by ON memories(superseded_by);
            CREATE INDEX IF NOT EXISTS idx_memories_decay ON memories(decay_policy, created_at);",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    fn insert(&self, memory: &Memory) -> Result<()> {
        let pool = self.pool.clone();
        let id_str = memory.id.0.to_string();
        let memory_type_str = memory.memory_type.discriminant_str().to_string();
        let content = memory.content.clone();
        let scope_str = memory.scope.discriminant_str().to_string();
        let authority_str = authority_discriminant_str(memory.authority).to_string();
        let confidence = memory.confidence.value();
        let decay_policy_str = memory.decay_policy.discriminant_str().to_string();
        let evidence_refs_json = serde_json::to_string(&memory.evidence_refs)?;
        let supersedes_str = memory.supersedes.map(|id| id.0.to_string());
        let superseded_by_str = memory.superseded_by.map(|id| id.0.to_string());
        let created_at = memory.created_at.to_rfc3339();
        let last_accessed_at = memory.last_accessed_at.to_rfc3339();
        let source_agent = memory.source_agent.0.clone();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let mut query = sqlx::query(
                    "INSERT INTO memories (id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent)
                     VALUES ($1::uuid, $2, $3, $4, $5, $6, $7, $8, $9::uuid, $10::uuid, $11, $12, $13)",
                )
                .bind(&id_str)
                .bind(&memory_type_str)
                .bind(&content)
                .bind(&scope_str)
                .bind(&authority_str)
                .bind(confidence)
                .bind(&decay_policy_str)
                .bind(&evidence_refs_json);

                if let Some(ref s) = supersedes_str {
                    query = query.bind(s);
                } else {
                    query = query.bind(Option::<&str>::None);
                }
                if let Some(ref s) = superseded_by_str {
                    query = query.bind(s);
                } else {
                    query = query.bind(Option::<&str>::None);
                }

                query
                    .bind(&created_at)
                    .bind(&last_accessed_at)
                    .bind(&source_agent)
                    .execute(&pool)
                    .await
            })
        })?;

        Ok(())
    }

    fn get_by_id(&self, id: MemoryId) -> Result<Option<Memory>> {
        let pool = self.pool.clone();
        let id_str = id.0.to_string();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let row: Option<PgMemoryRow> = sqlx::query_as(
                    "SELECT id::text, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes::text, superseded_by::text, created_at, last_accessed_at, source_agent FROM memories WHERE id = $1::uuid",
                )
                .bind(&id_str)
                .fetch_optional(&pool)
                .await?;

                match row {
                    Some(r) => Ok(Some(r.into_memory()?)),
                    None => Ok(None),
                }
            })
        })
    }

    fn query_by_type(&self, memory_type: MemoryType) -> Result<Vec<Memory>> {
        let pool = self.pool.clone();
        let type_str = memory_type.discriminant_str().to_string();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let rows: Vec<PgMemoryRow> = sqlx::query_as(
                    "SELECT id::text, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes::text, superseded_by::text, created_at, last_accessed_at, source_agent FROM memories WHERE memory_type = $1 AND superseded_by IS NULL",
                )
                .bind(&type_str)
                .fetch_all(&pool)
                .await?;

                rows.into_iter().map(|r| r.into_memory()).collect()
            })
        })
    }

    fn query_by_scope(&self, scope: MemoryScope) -> Result<Vec<Memory>> {
        let pool = self.pool.clone();
        let scope_str = scope.discriminant_str().to_string();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let rows: Vec<PgMemoryRow> = sqlx::query_as(
                    "SELECT id::text, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes::text, superseded_by::text, created_at, last_accessed_at, source_agent FROM memories WHERE scope = $1 AND superseded_by IS NULL",
                )
                .bind(&scope_str)
                .fetch_all(&pool)
                .await?;

                rows.into_iter().map(|r| r.into_memory()).collect()
            })
        })
    }

    fn query_by_authority(&self, min: Authority, max: Authority) -> Result<Vec<Memory>> {
        let pool = self.pool.clone();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let rows: Vec<PgMemoryRow> = sqlx::query_as(
                    "SELECT id::text, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes::text, superseded_by::text, created_at, last_accessed_at, source_agent FROM memories WHERE superseded_by IS NULL",
                )
                .fetch_all(&pool)
                .await?;

                let mut memories = Vec::new();
                for row in rows {
                    let memory = row.into_memory()?;
                    if memory.authority >= min && memory.authority <= max {
                        memories.push(memory);
                    }
                }
                Ok(memories)
            })
        })
    }

    fn query_decayed(&self) -> Result<Vec<Memory>> {
        let pool = self.pool.clone();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let rows: Vec<PgMemoryRow> = sqlx::query_as(
                    "SELECT id::text, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes::text, superseded_by::text, created_at, last_accessed_at, source_agent FROM memories WHERE superseded_by IS NULL",
                )
                .fetch_all(&pool)
                .await?;

                let mut memories = Vec::new();
                for row in rows {
                    let memory = row.into_memory()?;
                    if memory.decay_policy.is_decayed(memory.created_at) {
                        memories.push(memory);
                    }
                }
                Ok(memories)
            })
        })
    }

    fn supersede(&self, old_id: MemoryId, new_id: MemoryId) -> Result<()> {
        let pool = self.pool.clone();
        let old_id_str = old_id.0.to_string();
        let new_id_str = new_id.0.to_string();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let old_exists: Option<(i64,)> =
                    sqlx::query_as("SELECT 1 FROM memories WHERE id = $1::uuid")
                        .bind(&old_id_str)
                        .fetch_optional(&pool)
                        .await?;
                let new_exists: Option<(i64,)> =
                    sqlx::query_as("SELECT 1 FROM memories WHERE id = $1::uuid")
                        .bind(&new_id_str)
                        .fetch_optional(&pool)
                        .await?;

                if old_exists.is_none() {
                    return Err(crate::error::Error::Store(format!(
                        "cannot supersede missing memory {}",
                        old_id
                    )));
                }
                if new_exists.is_none() {
                    return Err(crate::error::Error::Store(format!(
                        "cannot supersede with missing memory {}",
                        new_id
                    )));
                }

                let mut tx = pool.begin().await?;
                sqlx::query("UPDATE memories SET superseded_by = $1::uuid WHERE id = $2::uuid")
                    .bind(&new_id_str)
                    .bind(&old_id_str)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("UPDATE memories SET supersedes = $1::uuid WHERE id = $2::uuid")
                    .bind(&old_id_str)
                    .bind(&new_id_str)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                Ok(())
            })
        })
    }

    fn get_supersession_chain(&self, id: MemoryId) -> Result<Vec<Memory>> {
        let mut forward = Vec::new();
        let mut current = self.get_by_id(id)?;

        while let Some(memory) = current {
            forward.push(memory.clone());
            if let Some(superseded_by) = memory.superseded_by {
                current = self.get_by_id(superseded_by)?;
            } else {
                break;
            }
        }

        let mut backward = Vec::new();
        if let Some(first) = forward.first()
            && let Some(supersedes) = first.supersedes
        {
            let mut current = self.get_by_id(supersedes)?;
            while let Some(memory) = current {
                backward.push(memory.clone());
                if let Some(s) = memory.supersedes {
                    current = self.get_by_id(s)?;
                } else {
                    break;
                }
            }
        }

        backward.reverse();
        backward.extend(forward);
        Ok(backward)
    }

    fn update_last_accessed(&self, id: MemoryId) -> Result<()> {
        let pool = self.pool.clone();
        let id_str = id.0.to_string();
        let now = Utc::now().to_rfc3339();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                sqlx::query("UPDATE memories SET last_accessed_at = $1 WHERE id = $2::uuid")
                    .bind(&now)
                    .bind(&id_str)
                    .execute(&pool)
                    .await
            })
        })?;

        Ok(())
    }

    fn update_content(&self, id: MemoryId, content: &str) -> Result<()> {
        let pool = self.pool.clone();
        let id_str = id.0.to_string();
        let content_owned = content.to_string();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                sqlx::query("UPDATE memories SET content = $1 WHERE id = $2::uuid")
                    .bind(&content_owned)
                    .bind(&id_str)
                    .execute(&pool)
                    .await
            })
        })?;

        Ok(())
    }

    async fn insert_with_embedding(
        &self,
        memory: &Memory,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<()> {
        let evidence_refs_json = serde_json::to_string(&memory.evidence_refs)?;
        let id_str = memory.id.0.to_string();
        let memory_type_str = memory.memory_type.discriminant_str().to_string();
        let content = memory.content.clone();
        let scope_str = memory.scope.discriminant_str().to_string();
        let authority_str = authority_discriminant_str(memory.authority).to_string();
        let confidence = memory.confidence.value();
        let decay_policy_str = memory.decay_policy.discriminant_str().to_string();
        let supersedes_str = memory.supersedes.map(|id| id.0.to_string());
        let superseded_by_str = memory.superseded_by.map(|id| id.0.to_string());
        let created_at = memory.created_at.to_rfc3339();
        let last_accessed_at = memory.last_accessed_at.to_rfc3339();
        let source_agent = memory.source_agent.0.clone();

        let mut tx = self.pool.begin().await?;

        let mut query = sqlx::query(
            "INSERT INTO memories (id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent)
             VALUES ($1::uuid, $2, $3, $4, $5, $6, $7, $8, $9::uuid, $10::uuid, $11, $12, $13)",
        )
        .bind(&id_str)
        .bind(&memory_type_str)
        .bind(&content)
        .bind(&scope_str)
        .bind(&authority_str)
        .bind(confidence)
        .bind(&decay_policy_str)
        .bind(&evidence_refs_json);

        if let Some(ref s) = supersedes_str {
            query = query.bind(s);
        } else {
            query = query.bind(Option::<&str>::None);
        }
        if let Some(ref s) = superseded_by_str {
            query = query.bind(s);
        } else {
            query = query.bind(Option::<&str>::None);
        }

        query
            .bind(&created_at)
            .bind(&last_accessed_at)
            .bind(&source_agent)
            .execute(&mut *tx)
            .await?;

        if let Some(ref embedding) = memory.embedding {
            let mut payload = std::collections::HashMap::new();
            payload.insert(
                "memory_type".to_string(),
                memory.memory_type.discriminant_str().into(),
            );
            payload.insert("scope".to_string(), memory.scope.discriminant_str().into());
            payload.insert("content".to_string(), memory.content.clone().into());

            if let Err(e) = qdrant.upsert(memory.id, embedding.clone(), payload).await {
                tx.rollback().await?;
                return Err(e);
            }
        }

        tx.commit().await?;
        Ok(())
    }

    async fn search_similar(
        &self,
        embedding: Vec<f32>,
        limit: u64,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<Vec<(MemoryId, f32)>> {
        let search_limit = limit.saturating_mul(4).max(limit).max(1);
        let similar = qdrant.search(embedding, search_limit).await?;
        let mut current = Vec::new();

        for (id, score) in similar {
            if let Some(memory) = self.get_by_id(id)?
                && memory.superseded_by.is_none()
            {
                current.push((id, score));
            }

            if current.len() >= limit as usize {
                break;
            }
        }

        Ok(current)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

fn authority_discriminant_str(authority: Authority) -> &'static str {
    match authority {
        Authority::CompilerOutput => "CompilerOutput",
        Authority::UserInstruction => "UserInstruction",
        Authority::RepositoryState => "RepositoryState",
        Authority::AcceptedADR => "AcceptedADR",
        Authority::ReviewFindings => "ReviewFindings",
        Authority::LLMInference => "LLMInference",
        Authority::SpeculativeReasoning => "SpeculativeReasoning",
    }
}

#[allow(clippy::too_many_arguments)]
fn row_to_memory_common(
    id: String,
    memory_type: String,
    content: String,
    scope: String,
    authority: String,
    confidence: f64,
    decay_policy: String,
    evidence_refs_json: String,
    supersedes: Option<String>,
    superseded_by: Option<String>,
    created_at: String,
    last_accessed_at: String,
    source_agent: String,
) -> std::result::Result<Memory, String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let memory_type = MemoryType::try_from(memory_type.as_str()).map_err(|e| e.to_string())?;
    let scope = MemoryScope::try_from(scope.as_str()).map_err(|e| e.to_string())?;
    let authority = Authority::try_from(authority.as_str()).map_err(|e| e.to_string())?;
    let confidence = Confidence::new(confidence).map_err(|e| e.to_string())?;
    let decay_policy = DecayPolicy::try_from(decay_policy.as_str()).map_err(|e| e.to_string())?;

    let evidence_refs: Vec<EventId> =
        serde_json::from_str(&evidence_refs_json).map_err(|e| e.to_string())?;

    let created_at: DateTime<Utc> = DateTime::parse_from_rfc3339(&created_at)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| e.to_string())?;
    let last_accessed_at: DateTime<Utc> = DateTime::parse_from_rfc3339(&last_accessed_at)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| e.to_string())?;

    Ok(Memory {
        id: MemoryId(id),
        memory_type,
        content,
        embedding: None,
        scope,
        authority,
        confidence,
        decay_policy,
        evidence_refs,
        supersedes: supersedes
            .map(|s| Uuid::parse_str(&s).map(MemoryId))
            .transpose()
            .map_err(|e| e.to_string())?,
        superseded_by: superseded_by
            .map(|s| Uuid::parse_str(&s).map(MemoryId))
            .transpose()
            .map_err(|e| e.to_string())?,
        created_at,
        last_accessed_at,
        source_agent: RoleId::new(source_agent),
    })
}

#[derive(Debug, sqlx::FromRow)]
struct PgMemoryRow {
    id: String,
    memory_type: String,
    content: String,
    scope: String,
    authority: String,
    confidence: f64,
    decay_policy: String,
    evidence_refs: String,
    supersedes: Option<String>,
    superseded_by: Option<String>,
    created_at: String,
    last_accessed_at: String,
    source_agent: String,
}

impl PgMemoryRow {
    fn into_memory(self) -> Result<Memory> {
        row_to_memory_common(
            self.id,
            self.memory_type,
            self.content,
            self.scope,
            self.authority,
            self.confidence,
            self.decay_policy,
            self.evidence_refs,
            self.supersedes,
            self.superseded_by,
            self.created_at,
            self.last_accessed_at,
            self.source_agent,
        )
        .map_err(|e| crate::error::Error::Store(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_memory() -> Memory {
        Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("Test fact")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap()
    }

    #[test]
    fn open_creates_database() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = MemoryStore::open(tmp.path()).unwrap();
        drop(store);
        assert!(tmp.path().exists());
    }

    #[test]
    fn insert_and_get_by_id() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = MemoryStore::open(tmp.path()).unwrap();
        let memory = test_memory();
        store.insert(&memory).unwrap();
        let retrieved = store.get_by_id(memory.id).unwrap().unwrap();
        assert_eq!(retrieved.id, memory.id);
        assert_eq!(retrieved.content, memory.content);
    }

    #[test]
    fn query_by_type() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = MemoryStore::open(tmp.path()).unwrap();

        let fact = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("Fact")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        let decision = Memory::builder()
            .memory_type(MemoryType::Decision)
            .content("Decision")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        store.insert(&fact).unwrap();
        store.insert(&decision).unwrap();

        let facts = store.query_by_type(MemoryType::Fact).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "Fact");
    }

    #[test]
    fn query_by_scope() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = MemoryStore::open(tmp.path()).unwrap();

        let ephemeral = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("Ephemeral")
            .scope(MemoryScope::Ephemeral)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        let project = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("Project")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        store.insert(&ephemeral).unwrap();
        store.insert(&project).unwrap();

        let project_memories = store.query_by_scope(MemoryScope::Project).unwrap();
        assert_eq!(project_memories.len(), 1);
        assert_eq!(project_memories[0].content, "Project");
    }

    #[test]
    fn query_by_authority_range() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = MemoryStore::open(tmp.path()).unwrap();

        let high = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("High")
            .scope(MemoryScope::Project)
            .authority(Authority::CompilerOutput)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        let low = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("Low")
            .scope(MemoryScope::Project)
            .authority(Authority::SpeculativeReasoning)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        store.insert(&high).unwrap();
        store.insert(&low).unwrap();

        let results = store
            .query_by_authority(Authority::ReviewFindings, Authority::CompilerOutput)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "High");
    }

    #[test]
    fn supersede_chain() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = MemoryStore::open(tmp.path()).unwrap();

        let a = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("A")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        let b = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("B")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        let c = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("C")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        store.insert(&a).unwrap();
        store.insert(&b).unwrap();
        store.insert(&c).unwrap();

        store.supersede(a.id, b.id).unwrap();
        store.supersede(b.id, c.id).unwrap();

        let chain = store.get_supersession_chain(a.id).unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].content, "A");
        assert_eq!(chain[1].content, "B");
        assert_eq!(chain[2].content, "C");
    }

    #[test]
    fn supersede_rejects_missing_replacement() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = MemoryStore::open(tmp.path()).unwrap();

        let memory = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("A")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        store.insert(&memory).unwrap();

        let err = store.supersede(memory.id, MemoryId::new()).unwrap_err();
        assert!(err.to_string().contains("missing memory"));

        let stored = store.get_by_id(memory.id).unwrap().unwrap();
        assert!(stored.superseded_by.is_none());
    }

    #[test]
    fn query_current_only_excludes_superseded() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = MemoryStore::open(tmp.path()).unwrap();

        let a = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("A")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        let b = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("B")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        store.insert(&a).unwrap();
        store.insert(&b).unwrap();
        store.supersede(a.id, b.id).unwrap();

        let current = store
            .query_current_only(|conn| {
                let mut stmt = conn.prepare("SELECT id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent FROM memories WHERE memory_type = ?1")?;
                stmt.query_map(params!["Fact"], SqliteMemoryStore::row_to_memory)
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap();

        assert_eq!(current.len(), 1);
        assert_eq!(current[0].content, "B");
    }

    #[test]
    fn query_decayed() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = MemoryStore::open(tmp.path()).unwrap();

        let stale = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("Stale")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .decay_policy(DecayPolicy::StaleAfterDays(0))
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        store.insert(&stale).unwrap();

        let decayed = store.query_decayed().unwrap();
        assert_eq!(decayed.len(), 1);
        assert_eq!(decayed[0].content, "Stale");
    }

    #[tokio::test]
    async fn postgres_insert_and_replay() {
        let Some((database_url, admin_pool, schema)) = postgres_test_database("memory_store").await
        else {
            return;
        };
        let store = PgMemoryStore::connect(&database_url).await.unwrap();
        let m1 = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("Postgres fact")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();
        let m2 = Memory::builder()
            .memory_type(MemoryType::Decision)
            .content("Postgres decision")
            .scope(MemoryScope::Organisational)
            .authority(Authority::AcceptedADR)
            .confidence(Confidence::new(0.85).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();

        store.insert(&m1).unwrap();
        store.insert(&m2).unwrap();

        let retrieved = store.get_by_id(m1.id).unwrap().unwrap();
        assert_eq!(retrieved.id, m1.id);
        assert_eq!(retrieved.content, "Postgres fact");

        let facts = store.query_by_type(MemoryType::Fact).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "Postgres fact");

        let decisions = store.query_by_type(MemoryType::Decision).unwrap();
        assert_eq!(decisions.len(), 1);

        let org = store.query_by_scope(MemoryScope::Organisational).unwrap();
        assert_eq!(org.len(), 1);

        let chain = store.get_supersession_chain(m1.id).unwrap();
        assert_eq!(chain.len(), 1);

        drop(store);
        drop_postgres_schema(&admin_pool, &schema).await;
    }

    async fn postgres_test_database(prefix: &str) -> Option<(String, sqlx::PgPool, String)> {
        let base_url = std::env::var("DATABASE_URL").ok()?;
        let schema = format!("{}_{}", prefix, now_nanos());
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&base_url)
            .await
            .ok()?;
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin_pool)
            .await
            .ok()?;
        let separator = if base_url.contains('?') { '&' } else { '?' };
        let database_url = format!("{base_url}{separator}options=-c%20search_path%3D{schema}");
        Some((database_url, admin_pool, schema))
    }

    async fn drop_postgres_schema(pool: &sqlx::PgPool, schema: &str) {
        sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
            .execute(pool)
            .await
            .unwrap();
    }

    fn now_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
