//! SQLite-backed memory store with vector-search integration.

use std::path::Path;

use chrono::{DateTime, Utc};
use mmat_event_stream::event::{EventId, RoleId};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::{
    error::Result,
    qdrant::VectorMemoryBackend,
    types::{Authority, Confidence, DecayPolicy, Memory, MemoryId, MemoryScope, MemoryType},
};

/// Persistent store for memories backed by SQLite.
///
/// Provides CRUD operations, querying by type/scope/authority, decay scanning,
/// and supersession chain traversal.
pub struct MemoryStore {
    conn: Mutex<Connection>,
}

impl MemoryStore {
    /// Opens (or creates) the SQLite database at the given path and runs migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
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

        let id = Uuid::parse_str(&id).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;
        let memory_type = MemoryType::try_from(memory_type.as_str()).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;
        let scope = MemoryScope::try_from(scope.as_str()).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;
        let authority = Authority::try_from(authority.as_str()).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;
        let confidence = Confidence::new(confidence).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Real, Box::new(e))
        })?;
        let decay_policy = DecayPolicy::try_from(decay_policy.as_str()).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;

        let evidence_refs: Vec<EventId> =
            serde_json::from_str(&evidence_refs_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;

        let created_at: DateTime<Utc> = DateTime::parse_from_rfc3339(&created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
        let last_accessed_at: DateTime<Utc> = DateTime::parse_from_rfc3339(&last_accessed_at)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;

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
                .map(|s| {
                    Uuid::parse_str(&s).map(MemoryId).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })
                })
                .transpose()?,
            superseded_by: superseded_by
                .map(|s| {
                    Uuid::parse_str(&s).map(MemoryId).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })
                })
                .transpose()?,
            created_at,
            last_accessed_at,
            source_agent: RoleId::new(source_agent),
        })
    }

    /// Inserts a memory into the store (without its embedding).
    pub fn insert(&self, memory: &Memory) -> Result<()> {
        let conn = self.conn.lock();
        let evidence_refs_json = serde_json::to_string(&memory.evidence_refs)?;
        conn.execute(
            "INSERT INTO memories (id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                memory.id.0.to_string(),
                memory.memory_type.discriminant_str(),
                memory.content,
                memory.scope.discriminant_str(),
                match memory.authority {
                    Authority::CompilerOutput => "CompilerOutput",
                    Authority::UserInstruction => "UserInstruction",
                    Authority::RepositoryState => "RepositoryState",
                    Authority::AcceptedADR => "AcceptedADR",
                    Authority::ReviewFindings => "ReviewFindings",
                    Authority::LLMInference => "LLMInference",
                    Authority::SpeculativeReasoning => "SpeculativeReasoning",
                },
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

    /// Retrieves a memory by its identifier.
    pub fn get_by_id(&self, id: MemoryId) -> Result<Option<Memory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent FROM memories WHERE id = ?1")?;
        let memory = stmt
            .query_row(params![id.0.to_string()], Self::row_to_memory)
            .optional()?;
        Ok(memory)
    }

    /// Queries all current (non-superseded) memories of the given type.
    pub fn query_by_type(&self, memory_type: MemoryType) -> Result<Vec<Memory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent FROM memories WHERE memory_type = ?1 AND superseded_by IS NULL")?;
        let rows = stmt.query_map(params![memory_type.discriminant_str()], Self::row_to_memory)?;
        let mut memories = Vec::new();
        for row in rows {
            memories.push(row.map_err(|e| crate::error::Error::Store(e.to_string()))?);
        }
        Ok(memories)
    }

    /// Queries all current (non-superseded) memories of the given scope.
    pub fn query_by_scope(&self, scope: MemoryScope) -> Result<Vec<Memory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, memory_type, content, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent FROM memories WHERE scope = ?1 AND superseded_by IS NULL")?;
        let rows = stmt.query_map(params![scope.discriminant_str()], Self::row_to_memory)?;
        let mut memories = Vec::new();
        for row in rows {
            memories.push(row.map_err(|e| crate::error::Error::Store(e.to_string()))?);
        }
        Ok(memories)
    }

    /// Queries all current (non-superseded) memories whose authority falls
    /// within the given inclusive range.
    pub fn query_by_authority(&self, min: Authority, max: Authority) -> Result<Vec<Memory>> {
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

    /// Queries all current memories whose decay policy indicates they are expired.
    pub fn query_decayed(&self) -> Result<Vec<Memory>> {
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

    /// Marks `old_id` as superseded by `new_id` and sets the reverse link.
    /// Returns an error if either memory does not exist.
    pub fn supersede(&self, old_id: MemoryId, new_id: MemoryId) -> Result<()> {
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

    /// Returns the full supersession chain for a memory, including ancestors and descendants.
    pub fn get_supersession_chain(&self, id: MemoryId) -> Result<Vec<Memory>> {
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

    /// Runs a query function against the store and filters the results to include
    /// only memories that have not been superseded.
    pub fn query_current_only<F>(&self, query_fn: F) -> Result<Vec<Memory>>
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

    /// Updates the `last_accessed_at` timestamp of a memory to the current time.
    pub fn update_last_accessed(&self, id: MemoryId) -> Result<()> {
        let conn = self.conn.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET last_accessed_at = ?1 WHERE id = ?2",
            params![now, id.0.to_string()],
        )?;
        Ok(())
    }

    /// Replaces the content of an existing memory.
    pub fn update_content(&self, id: MemoryId, content: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE memories SET content = ?1 WHERE id = ?2",
            params![content, id.0.to_string()],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub fn conn_for_test(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.conn.lock()
    }

    /// Inserts a memory into both the SQLite store and the vector backend
    /// atomically. If the vector upsert fails the SQLite insert is rolled back.
    pub async fn insert_with_embedding(
        &self,
        memory: &Memory,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<()> {
        let evidence_refs_json = serde_json::to_string(&memory.evidence_refs)?;
        let id_str = memory.id.0.to_string();
        let memory_type_str = memory.memory_type.discriminant_str();
        let content = memory.content.clone();
        let scope_str = memory.scope.discriminant_str();
        let authority_str = match memory.authority {
            Authority::CompilerOutput => "CompilerOutput",
            Authority::UserInstruction => "UserInstruction",
            Authority::RepositoryState => "RepositoryState",
            Authority::AcceptedADR => "AcceptedADR",
            Authority::ReviewFindings => "ReviewFindings",
            Authority::LLMInference => "LLMInference",
            Authority::SpeculativeReasoning => "SpeculativeReasoning",
        };
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

    /// Searches for memories similar to the given embedding, filtering out
    /// superseded entries and returning up to `limit` results.
    pub async fn search_similar(
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
                stmt.query_map(params!["Fact"], MemoryStore::row_to_memory)
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
}
