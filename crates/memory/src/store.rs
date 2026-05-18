use chrono::{DateTime, Utc};
use mmat_db::{AsyncPgConnection, models::NewMemory};
use mmat_event_stream::event::{EventId, RoleId};
use uuid::Uuid;

use crate::{
    error::Result,
    types::{Authority, Confidence, DecayPolicy, Memory, MemoryId, MemoryScope, MemoryType},
    vector_backend::VectorMemoryBackend,
};

pub struct PgMemoryStore {
    pool: mmat_db::Pool<AsyncPgConnection>,
}

pub struct MemoryStore {
    inner: PgMemoryStore,
}

impl PgMemoryStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = mmat_db::new_pool(database_url)
            .await
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string())))?;
        Ok(Self { pool })
    }

    pub fn new_with_pool(pool: mmat_db::Pool<AsyncPgConnection>) -> Self {
        Self { pool }
    }

    fn block_on<F, T>(&self, future: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>> + Send,
        T: Send,
    {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| rt.block_on(future))
    }

    fn memory_to_new_memory(&self, memory: &Memory) -> Result<NewMemory> {
        Ok(NewMemory {
            memory_type: memory.memory_type.discriminant_str().to_string(),
            content: memory.content.clone(),
            scope: memory.scope.discriminant_str().to_string(),
            authority: authority_discriminant_str(memory.authority).to_string(),
            confidence: memory.confidence.value(),
            decay_policy: memory.decay_policy.discriminant_str().to_string(),
            evidence_refs: serde_json::to_string(&memory.evidence_refs)?,
            supersedes: memory.supersedes.map(|id| id.0),
            superseded_by: memory.superseded_by.map(|id| id.0),
            created_at: memory.created_at.to_rfc3339(),
            last_accessed_at: memory.last_accessed_at.to_rfc3339(),
            source_agent: memory.source_agent.0.clone(),
        })
    }

    fn db_memory_to_domain(db_memory: mmat_db::models::Memory) -> Result<Memory> {
        row_to_memory_common(
            db_memory.id.to_string(),
            db_memory.memory_type,
            db_memory.content,
            db_memory.scope,
            db_memory.authority,
            db_memory.confidence,
            db_memory.decay_policy,
            db_memory.evidence_refs,
            db_memory.supersedes.map(|id| id.to_string()),
            db_memory.superseded_by.map(|id| id.to_string()),
            db_memory.created_at,
            db_memory.last_accessed_at,
            db_memory.source_agent,
        )
        .map_err(|e| crate::error::Error::Store(e.to_string()))
    }

    fn insert(&self, memory: &Memory) -> Result<Memory> {
        let new_memory = self.memory_to_new_memory(memory)?;
        let pool = self.pool.clone();

        self.block_on(async {
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;
            let row = mmat_db::memory::insert_memory(&mut conn, &new_memory)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            Self::db_memory_to_domain(row)
        })
    }

    fn get_by_id(&self, id: MemoryId) -> Result<Option<Memory>> {
        let pool = self.pool.clone();
        let id_uuid = id.0;

        self.block_on(async {
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;
            let row = mmat_db::memory::get_memory_by_id(&mut conn, id_uuid)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            match row {
                Some(r) => Ok(Some(Self::db_memory_to_domain(r)?)),
                None => Ok(None),
            }
        })
    }

    fn query_by_type(&self, memory_type: MemoryType) -> Result<Vec<Memory>> {
        let pool = self.pool.clone();
        let type_str = memory_type.discriminant_str().to_string();

        self.block_on(async {
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;
            let rows = mmat_db::memory::query_memories_not_superseded_by_type(&mut conn, &type_str)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            rows.into_iter().map(Self::db_memory_to_domain).collect()
        })
    }

    fn query_by_scope(&self, scope: MemoryScope) -> Result<Vec<Memory>> {
        let pool = self.pool.clone();
        let scope_str = scope.discriminant_str().to_string();

        self.block_on(async {
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;
            let rows =
                mmat_db::memory::query_memories_not_superseded_by_scope(&mut conn, &scope_str)
                    .await
                    .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            rows.into_iter().map(Self::db_memory_to_domain).collect()
        })
    }

    fn query_by_authority(&self, min: Authority, max: Authority) -> Result<Vec<Memory>> {
        let pool = self.pool.clone();

        self.block_on(async {
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;
            let rows = mmat_db::memory::query_memories_not_superseded(&mut conn)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            let mut memories = Vec::new();
            for row in rows {
                let memory = Self::db_memory_to_domain(row)?;
                if memory.authority >= min && memory.authority <= max {
                    memories.push(memory);
                }
            }
            Ok(memories)
        })
    }

    fn query_decayed(&self) -> Result<Vec<Memory>> {
        let pool = self.pool.clone();

        self.block_on(async {
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;
            let rows = mmat_db::memory::query_memories_not_superseded(&mut conn)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            let mut memories = Vec::new();
            for row in rows {
                let memory = Self::db_memory_to_domain(row)?;
                if memory.decay_policy.is_decayed(memory.created_at) {
                    memories.push(memory);
                }
            }
            Ok(memories)
        })
    }

    fn supersede(&self, old_id: MemoryId, new_id: MemoryId) -> Result<()> {
        let pool = self.pool.clone();
        let old_uuid = old_id.0;
        let new_uuid = new_id.0;

        self.block_on(async {
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;

            let old_exists = mmat_db::memory::memory_exists(&mut conn, old_uuid)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            let new_exists = mmat_db::memory::memory_exists(&mut conn, new_uuid)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;

            if !old_exists {
                return Err(crate::error::Error::Store(format!(
                    "cannot supersede missing memory {}",
                    old_id
                )));
            }
            if !new_exists {
                return Err(crate::error::Error::Store(format!(
                    "cannot supersede with missing memory {}",
                    new_id
                )));
            }

            mmat_db::begin_transaction(&mut conn)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            if let Err(e) =
                mmat_db::memory::update_memory_superseded_by(&mut conn, old_uuid, Some(new_uuid))
                    .await
            {
                let _ = mmat_db::rollback_transaction(&mut conn).await;
                return Err(crate::error::Error::Database(mmat_db::DbError::Diesel(e)));
            }
            if let Err(e) =
                mmat_db::memory::update_memory_supersedes(&mut conn, new_uuid, Some(old_uuid)).await
            {
                let _ = mmat_db::rollback_transaction(&mut conn).await;
                return Err(crate::error::Error::Database(mmat_db::DbError::Diesel(e)));
            }
            mmat_db::commit_transaction(&mut conn)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            Ok(())
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
        let id_uuid = id.0;
        let now = Utc::now().to_rfc3339();

        self.block_on(async {
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;
            mmat_db::memory::update_memory_last_accessed(&mut conn, id_uuid, &now)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            Ok(())
        })
    }

    fn update_content(&self, id: MemoryId, content: &str) -> Result<()> {
        let pool = self.pool.clone();
        let id_uuid = id.0;
        let content_owned = content.to_string();

        self.block_on(async {
            let mut conn = pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;
            mmat_db::memory::update_memory_content(&mut conn, id_uuid, &content_owned)
                .await
                .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
            Ok(())
        })
    }

    async fn insert_with_embedding(
        &self,
        memory: &Memory,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<Memory> {
        let new_memory = self.memory_to_new_memory(memory)?;

        let mut conn =
            self.pool.get().await.map_err(|e| {
                crate::error::Error::Database(mmat_db::DbError::Pool(e.to_string()))
            })?;

        mmat_db::begin_transaction(&mut conn)
            .await
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;

        let inserted = match mmat_db::memory::insert_memory(&mut conn, &new_memory).await {
            Ok(row) => Self::db_memory_to_domain(row)?,
            Err(_) => {
                let _ = mmat_db::rollback_transaction(&mut conn).await;
                return Err(crate::error::Error::Store(
                    "failed to insert memory".to_string(),
                ));
            }
        };

        if let Some(ref embedding) = memory.embedding {
            let mut payload = std::collections::HashMap::new();
            payload.insert(
                "memory_type".to_string(),
                memory.memory_type.discriminant_str().into(),
            );
            payload.insert("scope".to_string(), memory.scope.discriminant_str().into());
            payload.insert("content".to_string(), memory.content.clone().into());

            if let Err(e) = qdrant.upsert(inserted.id, embedding.clone(), payload).await {
                let _ = mmat_db::rollback_transaction(&mut conn).await;
                return Err(e);
            }
        }

        mmat_db::commit_transaction(&mut conn)
            .await
            .map_err(|e| crate::error::Error::Database(mmat_db::DbError::Diesel(e)))?;
        Ok(inserted)
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

    pub fn pool(&self) -> &mmat_db::Pool<AsyncPgConnection> {
        &self.pool
    }
}

impl MemoryStore {
    pub fn new(database_url: &str) -> Result<Self> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| crate::error::Error::Runtime(e.to_string()))?;
        let pg = tokio::task::block_in_place(|| rt.block_on(PgMemoryStore::connect(database_url)))?;
        Ok(Self { inner: pg })
    }

    pub async fn new_async(database_url: &str) -> Result<Self> {
        let pg = PgMemoryStore::connect(database_url).await?;
        Ok(Self { inner: pg })
    }

    pub fn new_with_pool(pool: mmat_db::Pool<AsyncPgConnection>) -> Self {
        Self {
            inner: PgMemoryStore { pool },
        }
    }

    pub fn insert(&self, memory: &Memory) -> Result<Memory> {
        self.inner.insert(memory)
    }

    pub fn get_by_id(&self, id: MemoryId) -> Result<Option<Memory>> {
        self.inner.get_by_id(id)
    }

    pub fn query_by_type(&self, memory_type: MemoryType) -> Result<Vec<Memory>> {
        self.inner.query_by_type(memory_type)
    }

    pub fn query_by_scope(&self, scope: MemoryScope) -> Result<Vec<Memory>> {
        self.inner.query_by_scope(scope)
    }

    pub fn query_by_authority(&self, min: Authority, max: Authority) -> Result<Vec<Memory>> {
        self.inner.query_by_authority(min, max)
    }

    pub fn query_decayed(&self) -> Result<Vec<Memory>> {
        self.inner.query_decayed()
    }

    pub fn supersede(&self, old_id: MemoryId, new_id: MemoryId) -> Result<()> {
        self.inner.supersede(old_id, new_id)
    }

    pub fn get_supersession_chain(&self, id: MemoryId) -> Result<Vec<Memory>> {
        self.inner.get_supersession_chain(id)
    }

    pub fn update_last_accessed(&self, id: MemoryId) -> Result<()> {
        self.inner.update_last_accessed(id)
    }

    pub fn update_content(&self, id: MemoryId, content: &str) -> Result<()> {
        self.inner.update_content(id, content)
    }

    pub async fn insert_with_embedding(
        &self,
        memory: &Memory,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<Memory> {
        self.inner.insert_with_embedding(memory, qdrant).await
    }

    pub async fn search_similar(
        &self,
        embedding: Vec<f32>,
        limit: u64,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<Vec<(MemoryId, f32)>> {
        self.inner.search_similar(embedding, limit, qdrant).await
    }

    pub fn pool(&self) -> &mmat_db::Pool<AsyncPgConnection> {
        &self.inner.pool
    }
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryStore").finish_non_exhaustive()
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

#[cfg(test)]
pub(crate) mod tests {
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

    fn now_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    pub(crate) async fn postgres_test_database(
        prefix: &str,
    ) -> Option<(mmat_db::Pool<AsyncPgConnection>, String)> {
        let base_url = std::env::var("MMAT_DB_URL").ok()?;
        let schema = format!("{}_{}", prefix, now_nanos());
        let admin_pool = mmat_db::new_pool(&base_url).await.ok()?;
        let mut conn = admin_pool.get().await.ok()?;
        mmat_db::execute_sql(&mut conn, &format!("CREATE SCHEMA \"{schema}\""))
            .await
            .ok()?;
        let separator = if base_url.contains('?') { '&' } else { '?' };
        let database_url = format!("{base_url}{separator}options=-c%20search_path%3D{schema}");
        let pool = mmat_db::new_pool(&database_url).await.ok()?;
        // Run migration
        let migrate_pool = pool.clone();
        let mut migrator_conn = migrate_pool.get().await.ok()?;
        mmat_db::execute_sql(
            &mut migrator_conn,
            include_str!("../../db/migrations/2026-05-14-000001_init/up.sql"),
        )
        .await
        .ok()?;
        Some((pool, schema))
    }

    pub(crate) async fn drop_postgres_schema(
        pool: &mmat_db::Pool<AsyncPgConnection>,
        schema: &str,
    ) {
        if let Ok(mut conn) = pool.get().await {
            let _ = mmat_db::execute_sql(
                &mut conn,
                &format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn postgres_crud_queries_and_supersession() {
        let Some((pool, schema)) = postgres_test_database("memory_store_tests").await else {
            return;
        };

        {
            let store = MemoryStore::new_with_pool(pool.clone());
            let memory = test_memory();
            let memory = store.insert(&memory).unwrap();

            let retrieved = store.get_by_id(memory.id).unwrap().unwrap();
            assert_eq!(retrieved.id, memory.id);
            assert_eq!(retrieved.content, memory.content);

            let facts = store.query_by_type(MemoryType::Fact).unwrap();
            assert_eq!(facts.len(), 1);

            let scoped = store.query_by_scope(MemoryScope::Project).unwrap();
            assert_eq!(scoped.len(), 1);

            let authority_results = store
                .query_by_authority(Authority::CompilerOutput, Authority::SpeculativeReasoning)
                .unwrap();
            assert!(!authority_results.is_empty());
        }

        {
            let store = MemoryStore::new_with_pool(pool.clone());
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

            let a = store.insert(&a).unwrap();
            let b = store.insert(&b).unwrap();
            let c = store.insert(&c).unwrap();

            store.supersede(a.id, b.id).unwrap();
            store.supersede(b.id, c.id).unwrap();

            let chain = store.get_supersession_chain(a.id).unwrap();
            assert_eq!(chain.len(), 3);
            assert_eq!(chain[0].content, "A");
            assert_eq!(chain[1].content, "B");
            assert_eq!(chain[2].content, "C");
        }

        {
            let store = MemoryStore::new_with_pool(pool.clone());
            let memory = Memory::builder()
                .memory_type(MemoryType::Fact)
                .content("X")
                .scope(MemoryScope::Project)
                .authority(Authority::UserInstruction)
                .confidence(Confidence::new(0.9).unwrap())
                .source_agent(RoleId::new("test"))
                .build()
                .unwrap();

            let memory = store.insert(&memory).unwrap();

            let bad_id = MemoryId(Uuid::nil());
            let err = store.supersede(memory.id, bad_id).unwrap_err();
            assert!(err.to_string().contains("missing memory"));

            let stored = store.get_by_id(memory.id).unwrap().unwrap();
            assert!(stored.superseded_by.is_none());
        }

        {
            let store = MemoryStore::new_with_pool(pool.clone());
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

        drop_postgres_schema(&pool, &schema).await;
    }
}
