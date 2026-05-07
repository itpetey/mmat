use std::time::Duration;

use memory::attention::AttentionEngine;
use memory::qdrant::VectorMemoryBackend;
use memory::store::MemoryStore;
use memory::types::{Authority, Memory, MemoryScope, MemoryType};
use serde::{Deserialize, Serialize};

use crate::role::RoleType;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalProfile {
    pub allowed_scopes: Vec<MemoryScope>,
    pub allowed_types: Vec<MemoryType>,
    pub min_authority: Authority,
    pub max_age: Option<Duration>,
    pub result_limit: usize,
}

pub struct RetrievalPlanner;

impl RetrievalProfile {
    pub fn all() -> Self {
        Self {
            allowed_scopes: vec![
                MemoryScope::Ephemeral,
                MemoryScope::Project,
                MemoryScope::Organisational,
                MemoryScope::WorldModel,
            ],
            allowed_types: vec![
                MemoryType::Fact,
                MemoryType::Decision,
                MemoryType::Constraint,
                MemoryType::Preference,
                MemoryType::Risk,
                MemoryType::Lesson,
                MemoryType::SOP,
                MemoryType::Incident,
                MemoryType::Assumption,
                MemoryType::OpenQuestion,
                MemoryType::Relationship,
            ],
            min_authority: Authority::SpeculativeReasoning,
            max_age: None,
            result_limit: 50,
        }
    }
}

impl RetrievalPlanner {
    pub fn new() -> Self {
        Self
    }

    pub fn retrieve(
        &self,
        memory_store: &MemoryStore,
        profile: &RetrievalProfile,
        task_context: &str,
    ) -> Vec<Memory> {
        let mut results: Vec<Memory> = Vec::new();

        // Structured query: apply filters
        for scope in &profile.allowed_scopes {
            if let Ok(memories) = memory_store.query_by_scope(*scope) {
                for memory in memories {
                    if profile.allowed_types.contains(&memory.memory_type)
                        && memory.authority >= profile.min_authority
                        && !is_too_old(&memory, profile.max_age)
                        && !is_duplicate(&results, &memory)
                    {
                        results.push(memory);
                    }
                }
            }
        }

        // Semantic query: if task_context is non-empty, perform text-based search
        // as a fallback when embeddings are not available.
        if !task_context.trim().is_empty() {
            let keywords: Vec<&str> = task_context
                .split_whitespace()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
                .filter(|w| !w.is_empty())
                .collect();

            if !keywords.is_empty() {
                for scope in &profile.allowed_scopes {
                    if let Ok(memories) = memory_store.query_by_scope(*scope) {
                        for memory in memories {
                            if profile.allowed_types.contains(&memory.memory_type)
                                && memory.authority >= profile.min_authority
                                && !is_too_old(&memory, profile.max_age)
                                && !is_duplicate(&results, &memory)
                                && content_matches_keywords(&memory.content, &keywords)
                            {
                                results.push(memory);
                            }
                        }
                    }
                }
            }
        }

        // Sort by recency (semantic score not available without embeddings)
        results.sort_by_key(|b| std::cmp::Reverse(b.created_at));

        // Apply result limit
        results.truncate(profile.result_limit);
        results
    }

    /// Async retrieval with optional semantic search via Qdrant.
    pub async fn retrieve_async(
        &self,
        memory_store: &MemoryStore,
        profile: &RetrievalProfile,
        task_context: &str,
        qdrant: Option<&dyn VectorMemoryBackend>,
    ) -> Vec<Memory> {
        let structured_results = self.retrieve(memory_store, profile, task_context);

        // Semantic search: if Qdrant is available and task_context is non-empty
        if let Some(backend) = qdrant
            && !task_context.trim().is_empty()
        {
            let mut results = Vec::new();
            let embedding = AttentionEngine::compute_simple_embedding(task_context);
            if let Ok(similar_ids) = memory_store
                .search_similar(embedding, profile.result_limit as u64, backend)
                .await
            {
                for (id, _score) in similar_ids {
                    if let Ok(Some(memory)) = memory_store.get_by_id(id)
                        && profile.allowed_scopes.contains(&memory.scope)
                        && profile.allowed_types.contains(&memory.memory_type)
                        && memory.authority >= profile.min_authority
                        && !is_too_old(&memory, profile.max_age)
                        && !is_duplicate(&results, &memory)
                    {
                        results.push(memory);
                    }
                }
            }

            for memory in structured_results {
                if !is_duplicate(&results, &memory) {
                    results.push(memory);
                }
            }

            results.truncate(profile.result_limit);
            return results;
        }

        structured_results
    }
}

impl Default for RetrievalPlanner {
    fn default() -> Self {
        Self::new()
    }
}

pub fn default_profile_for_role_type(role_type: RoleType) -> RetrievalProfile {
    match role_type {
        RoleType::Worker => RetrievalProfile {
            allowed_scopes: vec![MemoryScope::Project],
            allowed_types: vec![
                MemoryType::Constraint,
                MemoryType::Decision,
                MemoryType::Fact,
                MemoryType::SOP,
            ],
            min_authority: Authority::ReviewFindings,
            max_age: None,
            result_limit: 20,
        },
        RoleType::Scholar => RetrievalProfile::all(),
        RoleType::Architect => RetrievalProfile {
            allowed_scopes: vec![MemoryScope::Project, MemoryScope::Organisational],
            allowed_types: vec![
                MemoryType::Decision,
                MemoryType::Constraint,
                MemoryType::Risk,
                MemoryType::Lesson,
            ],
            min_authority: Authority::LLMInference,
            max_age: None,
            result_limit: 30,
        },
        RoleType::ProjectManager => RetrievalProfile {
            allowed_scopes: vec![MemoryScope::Project],
            allowed_types: vec![
                MemoryType::Constraint,
                MemoryType::Decision,
                MemoryType::Fact,
                MemoryType::Risk,
            ],
            min_authority: Authority::ReviewFindings,
            max_age: None,
            result_limit: 20,
        },
        RoleType::Reviewer => RetrievalProfile {
            allowed_scopes: vec![MemoryScope::Project, MemoryScope::Organisational],
            allowed_types: vec![
                MemoryType::SOP,
                MemoryType::Constraint,
                MemoryType::Decision,
            ],
            min_authority: Authority::ReviewFindings,
            max_age: None,
            result_limit: 20,
        },
        RoleType::Auditor => RetrievalProfile {
            allowed_scopes: vec![
                MemoryScope::Ephemeral,
                MemoryScope::Project,
                MemoryScope::Organisational,
                MemoryScope::WorldModel,
            ],
            allowed_types: vec![MemoryType::Fact],
            min_authority: Authority::CompilerOutput,
            max_age: None,
            result_limit: 50,
        },
        RoleType::IntentLead => RetrievalProfile {
            allowed_scopes: vec![MemoryScope::Project],
            allowed_types: vec![
                MemoryType::Preference,
                MemoryType::Constraint,
                MemoryType::OpenQuestion,
            ],
            min_authority: Authority::UserInstruction,
            max_age: None,
            result_limit: 15,
        },
        RoleType::OpsManager => RetrievalProfile {
            allowed_scopes: vec![MemoryScope::Organisational],
            allowed_types: vec![MemoryType::SOP, MemoryType::Lesson, MemoryType::Incident],
            min_authority: Authority::AcceptedADR,
            max_age: None,
            result_limit: 20,
        },
        RoleType::Librarian => RetrievalProfile::all(),
    }
}

fn content_matches_keywords(content: &str, keywords: &[&str]) -> bool {
    let lower = content.to_lowercase();
    keywords.iter().any(|kw| lower.contains(&kw.to_lowercase()))
}

fn is_duplicate(results: &[Memory], memory: &Memory) -> bool {
    results.iter().any(|m| m.id == memory.id)
}

fn is_too_old(memory: &Memory, max_age: Option<Duration>) -> bool {
    let Some(max_age) = max_age else {
        return false;
    };
    let age = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        - memory.created_at.timestamp() as u64;
    age > max_age.as_secs()
}
