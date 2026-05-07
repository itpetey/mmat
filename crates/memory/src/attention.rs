use std::sync::Arc;

use event_stream::event::{EventType, EvidenceRef, SemanticEvent};
use event_stream::event_bus::EventBus;
use llm::client::LlmClient;
use llm::message::{CompletionRequest, Message};
use serde::{Deserialize, Serialize};
use tokio::time::{Duration, interval};

use crate::error::{Error, Result};
use crate::qdrant::VectorMemoryBackend;
use crate::store::MemoryStore;
use crate::types::{Authority, MemoryId, MemoryScope, MemoryType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionConfig {
    pub salience_threshold: f64,
    pub similarity_threshold: f64,
    pub salience_model: String,
    pub salience_batch_size: usize,
    pub salience_batch_timeout_ms: u64,
}

impl Default for AttentionConfig {
    fn default() -> Self {
        Self {
            salience_threshold: 0.5,
            similarity_threshold: 0.95,
            salience_model: "gpt-4o-mini".to_string(),
            salience_batch_size: 8,
            salience_batch_timeout_ms: 1_000,
        }
    }
}

pub struct AttentionEngine {
    config: AttentionConfig,
    salience_llm: Option<Arc<dyn LlmClient>>,
}

impl AttentionEngine {
    pub fn new(config: AttentionConfig) -> Self {
        Self {
            config,
            salience_llm: None,
        }
    }

    pub fn new_with_llm(config: AttentionConfig, salience_llm: Arc<dyn LlmClient>) -> Self {
        Self {
            config,
            salience_llm: Some(salience_llm),
        }
    }

    pub async fn run(
        &self,
        bus: Arc<EventBus>,
        store: Arc<MemoryStore>,
        qdrant: Arc<dyn VectorMemoryBackend>,
    ) -> Result<()> {
        let mut rx = bus.subscribe(&[
            EventType::ClaimMade,
            EventType::DecisionRecorded,
            EventType::ToolExecuted,
            EventType::HumanFeedbackReceived,
            EventType::ArtefactProduced,
        ]);
        let mut batch = Vec::new();
        let mut batch_timer =
            interval(Duration::from_millis(self.config.salience_batch_timeout_ms));

        loop {
            tokio::select! {
                event = rx.recv() => {
                    let event = match event {
                        Ok(event) => event,
                        Err(_) => break,
                    };
                    batch.push(event);

                    if batch.len() >= self.config.salience_batch_size.max(1) {
                        self.process_batch(&bus, &store, qdrant.as_ref(), &mut batch).await?;
                    }
                }
                _ = batch_timer.tick() => {
                    if !batch.is_empty() {
                        self.process_batch(&bus, &store, qdrant.as_ref(), &mut batch).await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn process_batch(
        &self,
        bus: &EventBus,
        store: &MemoryStore,
        qdrant: &dyn VectorMemoryBackend,
        batch: &mut Vec<Arc<SemanticEvent>>,
    ) -> Result<()> {
        let events = std::mem::take(batch);
        let salience_scores = self.score_salience_batch(&events).await?;

        for (event, salience) in events.iter().zip(salience_scores) {
            if salience < self.config.salience_threshold {
                continue;
            }

            let content = Self::extract_content(event);
            if content.is_empty() {
                continue;
            }

            if let Some(existing_id) = self.find_duplicate(&content, store, qdrant).await? {
                self.rehearse(existing_id, store).await?;
                continue;
            }

            let (memory_type, scope, authority) = Self::infer_metadata(event);

            let proposal = SemanticEvent::MemoryProposed {
                event_id: event_stream::event::EventId::new(),
                source_agent: Self::extract_source_agent(event),
                timestamp_ns: Self::extract_timestamp(event),
                memory_type: memory_type.discriminant_str().to_string(),
                content: content.clone(),
                scope: scope.discriminant_str().to_string(),
                proposed_authority: Self::authority_to_role(&authority),
                evidence_refs: Self::extract_evidence_refs(event),
                confidence: Self::extract_confidence(event),
            };

            if let Err(e) = bus.publish(proposal) {
                tracing::error!("Failed to publish MemoryProposed event: {}", e);
            }
        }

        Ok(())
    }

    async fn score_salience_batch(&self, events: &[Arc<SemanticEvent>]) -> Result<Vec<f64>> {
        let Some(llm) = &self.salience_llm else {
            return Ok(events
                .iter()
                .map(|event| Self::score_salience(event))
                .collect());
        };

        let payload = events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                serde_json::json!({
                    "index": index,
                    "variant": event.variant_name(),
                    "content": Self::extract_content(event),
                    "source_agent": Self::extract_source_agent(event).to_string(),
                    "confidence": Self::extract_confidence(event),
                    "evidence_ref_count": Self::extract_evidence_refs(event).len(),
                })
            })
            .collect::<Vec<_>>();

        let mut request = CompletionRequest::new(
            self.config.salience_model.clone(),
            vec![
                Message::system(
                    "Score each event for durable, consequential memory salience. Return only a JSON array of numbers between 0 and 1, in the same order.",
                ),
                Message::user(serde_json::to_string(&payload)?),
            ],
        );
        request.temperature = Some(0.0);
        request.max_tokens = Some((events.len() as u32).saturating_mul(8).max(16));

        let response = llm
            .complete(request)
            .await
            .map_err(|e| Error::Llm(e.to_string()))?;
        let content = response
            .choices
            .first()
            .and_then(|choice| match &choice.message {
                Message::Assistant { content, .. } => content.clone(),
                _ => None,
            })
            .ok_or_else(|| {
                Error::Llm("salience response did not contain assistant content".into())
            })?;

        let scores: Vec<f64> = serde_json::from_str(&content)
            .map_err(|e| Error::Llm(format!("invalid salience score response: {}", e)))?;

        if scores.len() != events.len() {
            return Err(Error::Llm(format!(
                "salience response length {} did not match batch length {}",
                scores.len(),
                events.len()
            )));
        }

        Ok(scores
            .into_iter()
            .map(|score| score.clamp(0.0, 1.0))
            .collect())
    }

    pub fn score_salience(event: &SemanticEvent) -> f64 {
        let event_weight = match event.event_type() {
            EventType::HumanFeedbackReceived => 1.0,
            EventType::ToolExecuted => 0.8,
            EventType::ClaimMade => 0.6,
            EventType::DecisionRecorded => 0.7,
            EventType::ArtefactProduced => 0.5,
            _ => 0.0,
        };
        let (_, _, authority) = Self::infer_metadata(event);
        let authority_weight = authority.rank() as f64 / Authority::CompilerOutput.rank() as f64;
        let evidence_weight = if Self::extract_evidence_refs(event).is_empty() {
            0.0
        } else {
            1.0
        };
        let confidence = Self::extract_confidence(event).clamp(0.0, 1.0);

        (event_weight * 0.55)
            + (authority_weight * 0.2)
            + (evidence_weight * 0.1)
            + (confidence * 0.15)
    }

    pub fn extract_content(event: &SemanticEvent) -> String {
        match event {
            SemanticEvent::ClaimMade { claim_text, .. } => claim_text.clone(),
            SemanticEvent::DecisionRecorded { decision_text, .. } => decision_text.clone(),
            SemanticEvent::ToolExecuted {
                tool_name,
                stdout,
                stderr,
                ..
            } => {
                let mut content = format!("Tool executed: {}", tool_name);
                if !stdout.is_empty() {
                    content.push_str(&format!("\nOutput: {}", stdout));
                }
                if !stderr.is_empty() {
                    content.push_str(&format!("\nError: {}", stderr));
                }
                content
            }
            SemanticEvent::HumanFeedbackReceived { answer, .. } => answer.clone(),
            SemanticEvent::ArtefactProduced {
                artefact_type,
                reference,
                ..
            } => format!("Artefact produced: {} ({})", artefact_type, reference),
            _ => String::new(),
        }
    }

    fn extract_source_agent(event: &SemanticEvent) -> event_stream::event::RoleId {
        match event {
            SemanticEvent::ClaimMade { source_agent, .. }
            | SemanticEvent::DecisionRecorded { source_agent, .. }
            | SemanticEvent::ToolExecuted { source_agent, .. }
            | SemanticEvent::HumanFeedbackReceived { source_agent, .. }
            | SemanticEvent::ArtefactProduced { source_agent, .. } => source_agent.clone(),
            _ => event_stream::event::RoleId::new("unknown"),
        }
    }

    fn extract_timestamp(event: &SemanticEvent) -> u64 {
        match event {
            SemanticEvent::ClaimMade { timestamp_ns, .. }
            | SemanticEvent::DecisionRecorded { timestamp_ns, .. }
            | SemanticEvent::ToolExecuted { timestamp_ns, .. }
            | SemanticEvent::HumanFeedbackReceived { timestamp_ns, .. }
            | SemanticEvent::ArtefactProduced { timestamp_ns, .. } => *timestamp_ns,
            _ => 0,
        }
    }

    pub fn extract_evidence_refs(event: &SemanticEvent) -> Vec<EvidenceRef> {
        let mut refs = match event {
            SemanticEvent::ClaimMade { evidence_refs, .. } => evidence_refs.clone(),
            SemanticEvent::DecisionRecorded { rationale_refs, .. } => rationale_refs.clone(),
            _ => Vec::new(),
        };
        let source_ref = EvidenceRef {
            event_id: event.event_id(),
            description: "source event".to_string(),
        };
        if !refs
            .iter()
            .any(|evidence_ref| evidence_ref.event_id == source_ref.event_id)
        {
            refs.push(source_ref);
        }
        refs
    }

    pub fn extract_confidence(event: &SemanticEvent) -> f64 {
        match event {
            SemanticEvent::ClaimMade {
                confidence_score, ..
            } => *confidence_score as f64,
            SemanticEvent::ToolExecuted { exit_code, .. } => {
                if *exit_code == 0 {
                    0.9
                } else {
                    0.3
                }
            }
            SemanticEvent::HumanFeedbackReceived { .. } => 1.0,
            SemanticEvent::DecisionRecorded { .. } => 0.8,
            SemanticEvent::ArtefactProduced { .. } => 0.7,
            _ => 0.5,
        }
    }

    pub fn infer_metadata(event: &SemanticEvent) -> (MemoryType, MemoryScope, Authority) {
        match event {
            SemanticEvent::ToolExecuted {
                exit_code,
                tool_name,
                ..
            } => {
                let memory_type = if *exit_code == 0 {
                    MemoryType::Fact
                } else {
                    MemoryType::Incident
                };
                let authority = if tool_name.contains("compile") || tool_name.contains("build") {
                    Authority::CompilerOutput
                } else {
                    Authority::RepositoryState
                };
                (memory_type, MemoryScope::Project, authority)
            }
            SemanticEvent::ClaimMade { .. } => (
                MemoryType::Fact,
                MemoryScope::Project,
                Authority::LLMInference,
            ),
            SemanticEvent::DecisionRecorded { .. } => (
                MemoryType::Decision,
                MemoryScope::Project,
                Authority::UserInstruction,
            ),
            SemanticEvent::HumanFeedbackReceived { .. } => (
                MemoryType::Preference,
                MemoryScope::Project,
                Authority::UserInstruction,
            ),
            SemanticEvent::ArtefactProduced { .. } => (
                MemoryType::SOP,
                MemoryScope::Project,
                Authority::ReviewFindings,
            ),
            _ => (
                MemoryType::Fact,
                MemoryScope::Ephemeral,
                Authority::SpeculativeReasoning,
            ),
        }
    }

    fn authority_to_role(authority: &Authority) -> event_stream::event::RoleId {
        match authority {
            Authority::CompilerOutput => event_stream::event::RoleId::new("compiler"),
            Authority::UserInstruction => event_stream::event::RoleId::new("user"),
            Authority::RepositoryState => event_stream::event::RoleId::new("repository"),
            Authority::AcceptedADR => event_stream::event::RoleId::new("architect"),
            Authority::ReviewFindings => event_stream::event::RoleId::new("reviewer"),
            Authority::LLMInference => event_stream::event::RoleId::new("llm"),
            Authority::SpeculativeReasoning => event_stream::event::RoleId::new("llm"),
        }
    }

    async fn find_duplicate(
        &self,
        content: &str,
        store: &MemoryStore,
        qdrant: &dyn VectorMemoryBackend,
    ) -> Result<Option<MemoryId>> {
        let embedding = Self::compute_simple_embedding(content);
        let similar = store.search_similar(embedding, 1, qdrant).await?;

        Ok(similar
            .into_iter()
            .find(|(_, score)| *score >= self.config.similarity_threshold as f32)
            .map(|(id, _)| id))
    }

    pub fn compute_simple_embedding(content: &str) -> Vec<f32> {
        Self::compute_simple_embedding_with_dim(content, 64)
    }

    pub fn compute_simple_embedding_with_dim(content: &str, dim: usize) -> Vec<f32> {
        let words: Vec<&str> = content.split_whitespace().collect();
        let mut embedding = vec![0.0f32; dim];
        for (i, word) in words.iter().take(dim).enumerate() {
            let hash: u32 = word.bytes().enumerate().fold(0u32, |acc, (j, b)| {
                acc.wrapping_add((b as u32) << ((j % 4) * 8))
            });
            embedding[i] = (hash as f64 / u32::MAX as f64) as f32;
        }
        embedding
    }

    pub async fn rehearse(&self, memory_id: MemoryId, store: &MemoryStore) -> Result<()> {
        store.update_last_accessed(memory_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use qdrant_client::qdrant::Value;
    use std::collections::HashMap;

    use crate::types::{Confidence, Memory};

    #[derive(Default)]
    struct FakeVectorBackend {
        results: Mutex<Vec<(MemoryId, f32)>>,
    }

    #[async_trait::async_trait]
    impl VectorMemoryBackend for FakeVectorBackend {
        async fn upsert(
            &self,
            _id: MemoryId,
            _embedding: Vec<f32>,
            _payload: HashMap<String, Value>,
        ) -> Result<()> {
            Ok(())
        }

        async fn search(
            &self,
            _query_embedding: Vec<f32>,
            _limit: u64,
        ) -> Result<Vec<(MemoryId, f32)>> {
            Ok(self.results.lock().clone())
        }

        async fn delete(&self, _id: MemoryId) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn attention_config_defaults() {
        let config = AttentionConfig::default();
        assert!((config.salience_threshold - 0.5).abs() < f64::EPSILON);
        assert!((config.similarity_threshold - 0.95).abs() < f64::EPSILON);
        assert_eq!(config.salience_batch_size, 8);
    }

    #[test]
    fn salience_scoring_by_event_type() {
        let feedback = SemanticEvent::new_human_feedback_received(
            event_stream::event::RoleId::new("user"),
            "yes",
        );
        assert!(AttentionEngine::score_salience(&feedback) >= 0.9);

        let tool = SemanticEvent::new_tool_executed(
            event_stream::event::RoleId::new("worker"),
            "test",
            "{}",
            0,
            "",
            "",
        );
        assert!(AttentionEngine::score_salience(&tool) >= 0.7);
    }

    #[test]
    fn extract_content_from_claim() {
        let event = SemanticEvent::new_claim_made(
            event_stream::event::RoleId::new("llm"),
            "The API returns 404",
            vec![],
            0.8,
        );
        let content = AttentionEngine::extract_content(&event);
        assert_eq!(content, "The API returns 404");
    }

    #[test]
    fn infer_metadata_from_tool_executed_success() {
        let event = SemanticEvent::new_tool_executed(
            event_stream::event::RoleId::new("worker"),
            "cargo_build",
            "{}",
            0,
            "Build succeeded",
            "",
        );
        let (mem_type, scope, authority) = AttentionEngine::infer_metadata(&event);
        assert!(matches!(mem_type, MemoryType::Fact));
        assert!(matches!(scope, MemoryScope::Project));
        assert!(matches!(authority, Authority::CompilerOutput));
    }

    #[test]
    fn infer_metadata_from_tool_executed_failure() {
        let event = SemanticEvent::new_tool_executed(
            event_stream::event::RoleId::new("worker"),
            "cargo_build",
            "{}",
            1,
            "",
            "Build failed",
        );
        let (mem_type, _, _) = AttentionEngine::infer_metadata(&event);
        assert!(matches!(mem_type, MemoryType::Incident));
    }

    #[test]
    fn compute_simple_embedding_produces_vector() {
        let embedding = AttentionEngine::compute_simple_embedding("hello world test");
        assert_eq!(embedding.len(), 64);
    }

    #[test]
    fn extract_evidence_refs_includes_source_event() {
        let tool_event_id = event_stream::event::EventId::new();
        let claim = SemanticEvent::new_claim_made(
            event_stream::event::RoleId::new("llm"),
            "The API returns 404 for missing resources",
            vec![EvidenceRef {
                event_id: tool_event_id,
                description: "tool output".to_string(),
            }],
            0.85,
        );

        let refs = AttentionEngine::extract_evidence_refs(&claim);

        assert_eq!(refs.len(), 2);
        assert!(refs.iter().any(|r| r.event_id == tool_event_id));
        assert!(refs.iter().any(|r| r.event_id == claim.event_id()));
    }

    #[tokio::test]
    async fn near_duplicate_rehearses_existing_memory() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = Arc::new(MemoryStore::open(tmp.path()).unwrap());
        let memory = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("The database schema requires a migration because a column was added")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(event_stream::event::RoleId::new("user"))
            .build()
            .unwrap();
        store.insert(&memory).unwrap();
        let before = store
            .get_by_id(memory.id)
            .unwrap()
            .unwrap()
            .last_accessed_at;

        let fake = Arc::new(FakeVectorBackend::default());
        fake.results.lock().push((memory.id, 0.99));
        let bus = Arc::new(EventBus::new(16));
        let engine = AttentionEngine::new(AttentionConfig {
            salience_batch_size: 1,
            ..AttentionConfig::default()
        });

        let mut batch = vec![Arc::new(SemanticEvent::new_claim_made(
            event_stream::event::RoleId::new("llm"),
            "The database schema requires a migration because a column was added",
            vec![EvidenceRef {
                event_id: event_stream::event::EventId::new(),
                description: "tool output".to_string(),
            }],
            0.9,
        ))];

        engine
            .process_batch(&bus, &store, fake.as_ref(), &mut batch)
            .await
            .unwrap();

        let after = store
            .get_by_id(memory.id)
            .unwrap()
            .unwrap()
            .last_accessed_at;
        assert!(after > before);
    }
}
