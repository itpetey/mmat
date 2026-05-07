use std::sync::Arc;

use chrono::Utc;
use event_stream::event::{EventId, EventType, EvidenceRef, RoleId, SemanticEvent};
use event_stream::event_bus::EventBus;
use tokio::time::{Duration, interval};

use crate::attention::AttentionEngine;
use crate::error::Result;
use crate::qdrant::VectorMemoryBackend;
use crate::store::MemoryStore;
use crate::types::{Authority, Confidence, Memory, MemoryId, MemoryScope, MemoryType};

pub struct Librarian {
    store: Arc<MemoryStore>,
    qdrant: Arc<dyn VectorMemoryBackend>,
    decay_scan_interval: Duration,
}

#[derive(Clone, Copy)]
enum ContradictionResult {
    NoContradiction,
    HigherAuthorityNew { existing_id: MemoryId },
    LowerAuthorityNew,
    EqualAuthorityNew { existing_id: MemoryId },
}

impl Librarian {
    pub fn new(
        store: Arc<MemoryStore>,
        qdrant: Arc<dyn VectorMemoryBackend>,
        decay_scan_interval: Duration,
    ) -> Self {
        Self {
            store,
            qdrant,
            decay_scan_interval,
        }
    }

    pub async fn run(&self, bus: Arc<EventBus>) -> Result<()> {
        let mut rx = bus.subscribe(&[
            EventType::MemoryProposed,
            EventType::MemorySuperseded,
            EventType::PolicyViolationDetected,
        ]);

        let mut decay_timer = interval(self.decay_scan_interval);

        loop {
            tokio::select! {
                event = rx.recv() => {
                    let event = match event {
                        Ok(e) => e,
                        Err(_) => break,
                    };
                    self.handle_event(&bus, &event).await?;
                }
                _ = decay_timer.tick() => {
                    self.run_decay_scan(&bus).await?;
                }
            }
        }

        Ok(())
    }

    async fn handle_event(&self, bus: &EventBus, event: &SemanticEvent) -> Result<()> {
        match event {
            SemanticEvent::MemoryProposed {
                memory_type,
                content,
                scope,
                proposed_authority,
                source_agent,
                event_id,
                timestamp_ns,
                evidence_refs,
                confidence,
            } => {
                self.process_proposal(
                    bus,
                    memory_type,
                    content,
                    scope,
                    proposed_authority,
                    source_agent,
                    evidence_refs,
                    *confidence,
                    *event_id,
                    *timestamp_ns,
                )
                .await?;
            }
            SemanticEvent::MemorySuperseded {
                old_memory_id,
                new_memory_id,
                ..
            } => {
                self.process_superseded(bus, *old_memory_id, *new_memory_id)
                    .await?;
            }
            SemanticEvent::PolicyViolationDetected {
                violation_type,
                description,
                related_event_id,
                ..
            } => {
                self.process_audit_violation(bus, violation_type, description, *related_event_id)
                    .await?;
            }
            _ => {}
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_proposal(
        &self,
        bus: &EventBus,
        memory_type: &str,
        content: &str,
        scope: &str,
        proposed_authority: &RoleId,
        source_agent: &RoleId,
        evidence_refs: &[EvidenceRef],
        confidence: f64,
        event_id: EventId,
        _timestamp_ns: u64,
    ) -> Result<()> {
        let memory_type = MemoryType::try_from(memory_type)?;
        let scope = MemoryScope::try_from(scope)?;
        let mut authority = Self::role_to_authority(proposed_authority);
        let mut downgraded_ungrounded_fact = false;

        if let Err(reason) = self.durability_gate(content) {
            self.publish_rejection(
                bus,
                source_agent,
                memory_type.discriminant_str(),
                content,
                "durability",
                &reason,
            )
            .await;
            return Ok(());
        }

        let has_supporting_evidence = Self::has_supporting_evidence(evidence_refs);
        if memory_type == MemoryType::Fact
            && authority == Authority::LLMInference
            && !has_supporting_evidence
            && source_agent.0 != "user"
        {
            authority = Authority::SpeculativeReasoning;
            downgraded_ungrounded_fact = true;
            self.publish_policy_violation(
                bus,
                source_agent,
                "authority_downgrade",
                "Fact proposed without supporting evidence was downgraded to SpeculativeReasoning",
                Some(event_id),
            )
            .await;
        }

        if let Err(reason) = self.grounding_gate(
            source_agent,
            evidence_refs,
            &authority,
            downgraded_ungrounded_fact,
        ) {
            self.publish_rejection(
                bus,
                source_agent,
                memory_type.discriminant_str(),
                content,
                "grounding",
                &reason,
            )
            .await;
            return Ok(());
        }

        if let Err(reason) = self.scope_gate(&memory_type, &scope) {
            self.publish_rejection(
                bus,
                source_agent,
                memory_type.discriminant_str(),
                content,
                "scope",
                &reason,
            )
            .await;
            self.publish_policy_violation(
                bus,
                source_agent,
                "scope_violation",
                &reason,
                Some(event_id),
            )
            .await;
            return Ok(());
        }

        if let Err(reason) = self.invalidatability_gate(content) {
            self.publish_rejection(
                bus,
                source_agent,
                memory_type.discriminant_str(),
                content,
                "invalidatability",
                &reason,
            )
            .await;
            self.publish_policy_violation(
                bus,
                source_agent,
                "invalidatability_violation",
                &reason,
                Some(event_id),
            )
            .await;
            return Ok(());
        }

        let contradiction_result = self
            .contradiction_detection(&memory_type, &scope, content, authority)
            .await?;

        if matches!(contradiction_result, ContradictionResult::LowerAuthorityNew) {
            self.publish_rejection(
                bus,
                source_agent,
                memory_type.discriminant_str(),
                content,
                "contradiction",
                "lower authority than existing memory",
            )
            .await;
            return Ok(());
        }

        if matches!(contradiction_result, ContradictionResult::NoContradiction)
            && let Err(reason) = self.duplicate_gate(content).await?
        {
            self.publish_rejection(
                bus,
                source_agent,
                memory_type.discriminant_str(),
                content,
                "duplicate",
                &reason,
            )
            .await;
            return Ok(());
        }

        let mut builder = Memory::builder()
            .memory_type(memory_type)
            .content(content)
            .scope(scope)
            .authority(authority)
            .confidence(Confidence::new(confidence).unwrap_or_default())
            .evidence_refs(evidence_refs.iter().map(|er| er.event_id).collect())
            .source_agent(source_agent.clone());

        if let ContradictionResult::HigherAuthorityNew { existing_id }
        | ContradictionResult::EqualAuthorityNew { existing_id } = contradiction_result
        {
            builder = builder.supersedes(existing_id);
        }

        let memory = builder.build()?;

        let embedding = AttentionEngine::compute_simple_embedding(&memory.content);
        let mut memory_with_embedding = memory.clone();
        memory_with_embedding.embedding = Some(embedding.clone());

        self.store
            .insert_with_embedding(&memory_with_embedding, self.qdrant.as_ref())
            .await?;

        match contradiction_result {
            ContradictionResult::HigherAuthorityNew { existing_id }
            | ContradictionResult::EqualAuthorityNew { existing_id } => {
                self.store.supersede(existing_id, memory.id)?;
                if let Err(e) = self.qdrant.delete(existing_id).await {
                    tracing::warn!("Failed to delete superseded memory vector: {}", e);
                }
            }
            _ => {}
        }

        let accepted = SemanticEvent::MemoryAccepted {
            event_id: EventId::new(),
            source_agent: source_agent.clone(),
            timestamp_ns: Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64,
            memory_id: memory.id.0.into(),
            accepted_authority: Self::authority_to_role(&authority),
        };
        if let Err(e) = bus.publish(accepted) {
            tracing::error!("Failed to publish MemoryAccepted event: {}", e);
        }

        Ok(())
    }

    fn durability_gate(&self, content: &str) -> std::result::Result<(), String> {
        Self::durability_gate_static(content)
    }

    fn durability_gate_static(content: &str) -> std::result::Result<(), String> {
        let trivial_patterns = ["ok", "done", "yes", "no", "thanks", "thank you", "sure"];
        let lower = content.to_lowercase().trim().to_string();

        if lower.len() < 10 {
            return Err("content too short to be durable".to_string());
        }

        for pattern in &trivial_patterns {
            if lower == *pattern {
                return Err("content appears transient/trivial".to_string());
            }
        }

        Ok(())
    }

    fn grounding_gate(
        &self,
        source_agent: &RoleId,
        evidence_refs: &[EvidenceRef],
        authority: &Authority,
        allow_ungrounded_downgrade: bool,
    ) -> std::result::Result<(), String> {
        if source_agent.0 == "user" {
            return Ok(());
        }

        if Self::has_supporting_evidence(evidence_refs) || allow_ungrounded_downgrade {
            return Ok(());
        }

        if matches!(
            authority,
            Authority::CompilerOutput | Authority::RepositoryState
        ) {
            return Ok(());
        }

        Err("proposal has no evidence references and is not from user or compiler".to_string())
    }

    fn has_supporting_evidence(evidence_refs: &[EvidenceRef]) -> bool {
        evidence_refs
            .iter()
            .any(|evidence_ref| evidence_ref.description != "source event")
    }

    fn scope_gate(
        &self,
        memory_type: &MemoryType,
        scope: &MemoryScope,
    ) -> std::result::Result<(), String> {
        Self::scope_gate_static(memory_type, scope)
    }

    fn invalidatability_gate(&self, content: &str) -> std::result::Result<(), String> {
        Self::invalidatability_gate_static(content)
    }

    fn invalidatability_gate_static(content: &str) -> std::result::Result<(), String> {
        let lower = content.to_lowercase();
        let invalidatable = [
            "until",
            "unless",
            "as of",
            "because",
            "observed",
            "measured",
            "according to",
            "can be superseded",
            "subject to change",
        ];

        if invalidatable.iter().any(|phrase| lower.contains(phrase)) {
            return Ok(());
        }

        if lower.contains("always") || lower.contains("never") || lower.contains("impossible") {
            return Err("memory is phrased as unfalsifiable or permanent".to_string());
        }

        Ok(())
    }

    fn scope_gate_static(
        memory_type: &MemoryType,
        scope: &MemoryScope,
    ) -> std::result::Result<(), String> {
        match (memory_type, scope) {
            (MemoryType::SOP, MemoryScope::Ephemeral) => {
                Err("SOP requires durable scope, not Ephemeral".to_string())
            }
            (MemoryType::Lesson, MemoryScope::Ephemeral) => {
                Err("Lesson requires durable scope, not Ephemeral".to_string())
            }
            _ => Ok(()),
        }
    }

    async fn duplicate_gate(&self, content: &str) -> Result<std::result::Result<(), String>> {
        let embedding = AttentionEngine::compute_simple_embedding(content);
        let similar = self
            .store
            .search_similar(embedding, 1, self.qdrant.as_ref())
            .await?;

        if similar.is_empty() {
            return Ok(Ok(()));
        }

        let (id, score) = &similar[0];
        if *score > 0.92 {
            return Ok(Err(format!(
                "duplicate detected with memory {} at similarity score {}",
                id, score
            )));
        }

        Ok(Ok(()))
    }

    async fn contradiction_detection(
        &self,
        memory_type: &MemoryType,
        scope: &MemoryScope,
        content: &str,
        authority: Authority,
    ) -> Result<ContradictionResult> {
        let existing = self.store.query_by_type(*memory_type)?;
        let same_scope: Vec<_> = existing
            .into_iter()
            .filter(|m| &m.scope == scope && m.superseded_by.is_none())
            .collect();

        if same_scope.is_empty() {
            return Ok(ContradictionResult::NoContradiction);
        }

        for existing_memory in &same_scope {
            if contents_contradict(content, &existing_memory.content) {
                if authority > existing_memory.authority {
                    return Ok(ContradictionResult::HigherAuthorityNew {
                        existing_id: existing_memory.id,
                    });
                } else if authority < existing_memory.authority {
                    return Ok(ContradictionResult::LowerAuthorityNew);
                } else {
                    return Ok(ContradictionResult::EqualAuthorityNew {
                        existing_id: existing_memory.id,
                    });
                }
            }
        }

        Ok(ContradictionResult::NoContradiction)
    }

    async fn process_superseded(
        &self,
        _bus: &EventBus,
        old_memory_id: EventId,
        new_memory_id: EventId,
    ) -> Result<()> {
        let old_id = MemoryId(old_memory_id.0);
        let new_id = MemoryId(new_memory_id.0);
        if self
            .store
            .get_by_id(old_id)?
            .is_some_and(|memory| memory.superseded_by == Some(new_id))
        {
            return Ok(());
        }

        self.store.supersede(old_id, new_id)?;
        if let Err(e) = self.qdrant.delete(old_id).await {
            tracing::warn!("Failed to delete superseded memory vector: {}", e);
        }
        Ok(())
    }

    async fn process_audit_violation(
        &self,
        bus: &EventBus,
        violation_type: &str,
        description: &str,
        related_event_id: Option<EventId>,
    ) -> Result<()> {
        let Some(related_event_id) = related_event_id else {
            return Ok(());
        };
        let memory_id = MemoryId(related_event_id.0);
        let Some(memory) = self.store.get_by_id(memory_id)? else {
            return Ok(());
        };
        if memory.superseded_by.is_some() {
            return Ok(());
        }

        if memory.authority >= Authority::UserInstruction {
            let request = SemanticEvent::new_human_feedback_requested(
                RoleId::new("librarian-001"),
                format!("Audit finding requires review for memory {}", memory.id),
                format!("{violation_type}: {description}"),
            );
            if let Err(e) = bus.publish(request) {
                tracing::error!("Failed to publish HumanFeedbackRequested event: {}", e);
            }
            return Ok(());
        }

        let marker = Memory::builder()
            .memory_type(memory.memory_type)
            .content(format!(
                "[audit-flagged] {} ({}: {})",
                memory.content, violation_type, description
            ))
            .scope(memory.scope)
            .authority(Authority::ReviewFindings)
            .confidence(Confidence::new(1.0).unwrap_or_default())
            .decay_policy(crate::types::DecayPolicy::SupersededOnly)
            .evidence_refs(vec![related_event_id])
            .source_agent(RoleId::new("librarian-001"))
            .build()?;

        self.store.insert(&marker)?;
        self.store.supersede(memory.id, marker.id)?;
        if let Err(e) = self.qdrant.delete(memory.id).await {
            tracing::warn!("Failed to delete audit-flagged memory vector: {}", e);
        }

        let superseded = SemanticEvent::new_memory_superseded(
            RoleId::new("librarian-001"),
            memory.id.0.into(),
            marker.id.0.into(),
            format!("audit violation: {violation_type}"),
        );
        if let Err(e) = bus.publish(superseded) {
            tracing::error!(
                "Failed to publish MemorySuperseded for audit violation: {}",
                e
            );
        }

        Ok(())
    }

    async fn run_decay_scan(&self, bus: &EventBus) -> Result<()> {
        let decayed = self.store.query_decayed()?;

        for memory in decayed {
            let marker = Memory::builder()
                .memory_type(memory.memory_type)
                .content(format!("[decayed] {}", memory.content))
                .scope(memory.scope)
                .authority(Authority::CompilerOutput)
                .confidence(Confidence::new(1.0).unwrap_or_default())
                .decay_policy(crate::types::DecayPolicy::SupersededOnly)
                .source_agent(RoleId::new("librarian"))
                .build()?;

            self.store.insert(&marker)?;
            self.store.supersede(memory.id, marker.id)?;
            if let Err(e) = self.qdrant.delete(memory.id).await {
                tracing::warn!("Failed to delete decayed memory vector: {}", e);
            }

            let superseded_event = SemanticEvent::MemorySuperseded {
                event_id: EventId::new(),
                source_agent: RoleId::new("librarian"),
                timestamp_ns: Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64,
                old_memory_id: memory.id.0.into(),
                new_memory_id: marker.id.0.into(),
                reason: "decayed".to_string(),
            };

            if let Err(e) = bus.publish(superseded_event) {
                tracing::error!("Failed to publish MemorySuperseded for decay: {}", e);
            }
        }

        Ok(())
    }

    async fn publish_rejection(
        &self,
        bus: &EventBus,
        source_agent: &RoleId,
        memory_type: &str,
        content: &str,
        gate: &str,
        reason: &str,
    ) {
        let rejection = SemanticEvent::MemoryRejected {
            event_id: EventId::new(),
            source_agent: source_agent.clone(),
            timestamp_ns: Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64,
            proposed_memory_type: memory_type.to_string(),
            proposed_content: content.to_string(),
            rejection_gate: gate.to_string(),
            rejection_reason: reason.to_string(),
        };
        if let Err(e) = bus.publish(rejection) {
            tracing::error!("Failed to publish MemoryRejected event: {}", e);
        }
    }

    async fn publish_policy_violation(
        &self,
        bus: &EventBus,
        source_agent: &RoleId,
        violation_type: &str,
        description: &str,
        related_event_id: Option<EventId>,
    ) {
        let violation = SemanticEvent::PolicyViolationDetected {
            event_id: EventId::new(),
            source_agent: source_agent.clone(),
            timestamp_ns: Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64,
            violation_type: violation_type.to_string(),
            description: description.to_string(),
            related_event_id,
        };
        if let Err(e) = bus.publish(violation) {
            tracing::error!("Failed to publish PolicyViolationDetected event: {}", e);
        }
    }

    fn role_to_authority(role: &RoleId) -> Authority {
        match role.0.as_str() {
            "compiler" => Authority::CompilerOutput,
            "user" => Authority::UserInstruction,
            "intent-lead-001" => Authority::UserInstruction,
            "repository" => Authority::RepositoryState,
            "architect" => Authority::AcceptedADR,
            "ops-manager-001" => Authority::AcceptedADR,
            "reviewer" => Authority::ReviewFindings,
            "scholar-001" => Authority::ReviewFindings,
            "llm" => Authority::LLMInference,
            _ => Authority::SpeculativeReasoning,
        }
    }

    fn authority_to_role(authority: &Authority) -> RoleId {
        match authority {
            Authority::CompilerOutput => RoleId::new("compiler"),
            Authority::UserInstruction => RoleId::new("user"),
            Authority::RepositoryState => RoleId::new("repository"),
            Authority::AcceptedADR => RoleId::new("architect"),
            Authority::ReviewFindings => RoleId::new("reviewer"),
            Authority::LLMInference | Authority::SpeculativeReasoning => RoleId::new("llm"),
        }
    }
}

fn conflicting_http_status(new_content: &str, existing_content: &str) -> bool {
    let new_status = first_status_code(new_content);
    let existing_status = first_status_code(existing_content);

    matches!((new_status, existing_status), (Some(a), Some(b)) if a != b)
}

fn contents_contradict(new_content: &str, existing_content: &str) -> bool {
    if conflicting_http_status(new_content, existing_content) {
        return true;
    }

    let new_embedding = AttentionEngine::compute_simple_embedding(new_content);
    let existing_embedding = AttentionEngine::compute_simple_embedding(existing_content);
    let similarity = cosine_similarity(&new_embedding, &existing_embedding);
    if similarity < 0.55 {
        return false;
    }

    let opposing_pairs = [
        ("enabled", "disabled"),
        ("true", "false"),
        ("passes", "fails"),
        ("passed", "failed"),
        ("success", "failure"),
        ("succeeds", "fails"),
        ("present", "absent"),
        ("required", "not required"),
        ("allowed", "forbidden"),
    ];

    let new_lower = new_content.to_lowercase();
    let existing_lower = existing_content.to_lowercase();
    opposing_pairs.iter().any(|(left, right)| {
        (new_lower.contains(left) && existing_lower.contains(right))
            || (new_lower.contains(right) && existing_lower.contains(left))
    })
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

fn first_status_code(content: &str) -> Option<u16> {
    content
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| part.len() == 3)
        .find_map(|part| {
            let code = part.parse::<u16>().ok()?;
            if (100..=599).contains(&code) {
                Some(code)
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use qdrant_client::qdrant::Value;
    use std::collections::HashMap;

    use crate::qdrant::VectorMemoryBackend;

    #[derive(Default)]
    struct FakeVectorBackend {
        results: Mutex<Vec<(MemoryId, f32)>>,
        fail_upsert: bool,
        deleted: Mutex<Vec<MemoryId>>,
    }

    #[async_trait::async_trait]
    impl VectorMemoryBackend for FakeVectorBackend {
        async fn upsert(
            &self,
            _id: MemoryId,
            _embedding: Vec<f32>,
            _payload: HashMap<String, Value>,
        ) -> Result<()> {
            if self.fail_upsert {
                return Err(crate::error::Error::Qdrant("upsert failed".to_string()));
            }
            Ok(())
        }

        async fn search(
            &self,
            _query_embedding: Vec<f32>,
            _limit: u64,
        ) -> Result<Vec<(MemoryId, f32)>> {
            Ok(self.results.lock().clone())
        }

        async fn delete(&self, id: MemoryId) -> Result<()> {
            self.deleted.lock().push(id);
            Ok(())
        }
    }

    #[test]
    fn durability_gate_rejects_short_content() {
        assert!(Librarian::durability_gate_static("ok").is_err());
        assert!(Librarian::durability_gate_static("short").is_err());
        assert!(Librarian::durability_gate_static("This is a meaningful piece of content").is_ok());
    }

    #[test]
    fn scope_gate_rejects_sop_ephemeral() {
        assert!(Librarian::scope_gate_static(&MemoryType::SOP, &MemoryScope::Ephemeral).is_err());
        assert!(Librarian::scope_gate_static(&MemoryType::Fact, &MemoryScope::Project).is_ok());
    }

    #[test]
    fn invalidatability_gate_rejects_unfalsifiable_content() {
        assert!(Librarian::invalidatability_gate_static("This will always be impossible").is_err());
        assert!(
            Librarian::invalidatability_gate_static("This is true as of the latest test").is_ok()
        );
    }

    #[test]
    fn role_to_authority_mapping() {
        assert!(matches!(
            Librarian::role_to_authority(&RoleId::new("compiler")),
            Authority::CompilerOutput
        ));
        assert!(matches!(
            Librarian::role_to_authority(&RoleId::new("user")),
            Authority::UserInstruction
        ));
        assert!(matches!(
            Librarian::role_to_authority(&RoleId::new("llm")),
            Authority::LLMInference
        ));
    }

    #[test]
    fn cosine_similarity_identical_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn cosine_similarity_orthogonal_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim < 0.001);
    }

    #[test]
    fn detects_status_code_contradiction() {
        assert!(contents_contradict(
            "The API endpoint returns status 500 as of the latest test",
            "The API endpoint returns status 200 as of the latest test",
        ));
        assert!(!contents_contradict(
            "The API endpoint returns status 200 as of the latest test",
            "The API endpoint returns status 200 on success as of the latest test",
        ));
    }

    #[tokio::test]
    async fn ungrounded_llm_fact_is_downgraded_and_flagged() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = Arc::new(MemoryStore::open(tmp.path()).unwrap());
        let qdrant = Arc::new(FakeVectorBackend::default());
        let librarian = Librarian::new(store.clone(), qdrant, Duration::from_secs(3600));
        let bus = EventBus::new(16);
        let mut violations = bus.subscribe(&[EventType::PolicyViolationDetected]);

        librarian
            .process_proposal(
                &bus,
                "Fact",
                "The endpoint returns 404 as of the current investigation",
                "Project",
                &RoleId::new("llm"),
                &RoleId::new("llm"),
                &[],
                0.8,
                EventId::new(),
                0,
            )
            .await
            .unwrap();

        let stored = store.query_by_type(MemoryType::Fact).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].authority, Authority::SpeculativeReasoning);

        let violation = tokio::time::timeout(Duration::from_secs(1), violations.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            violation.as_ref(),
            SemanticEvent::PolicyViolationDetected { violation_type, .. }
                if violation_type == "authority_downgrade"
        ));
    }

    #[tokio::test]
    async fn failed_acceptance_does_not_supersede_existing_memory() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = Arc::new(MemoryStore::open(tmp.path()).unwrap());
        let old_memory = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("The API endpoint returns status 200 as of the latest test")
            .scope(MemoryScope::Project)
            .authority(Authority::LLMInference)
            .confidence(Confidence::new(0.7).unwrap())
            .source_agent(RoleId::new("llm"))
            .build()
            .unwrap();
        store.insert(&old_memory).unwrap();

        let qdrant = Arc::new(FakeVectorBackend {
            fail_upsert: true,
            ..FakeVectorBackend::default()
        });
        let librarian = Librarian::new(store.clone(), qdrant, Duration::from_secs(3600));
        let bus = EventBus::new(16);

        let result = librarian
            .process_proposal(
                &bus,
                "Fact",
                "The API endpoint returns status 500 as of the latest test",
                "Project",
                &RoleId::new("compiler"),
                &RoleId::new("compiler"),
                &[],
                0.95,
                EventId::new(),
                0,
            )
            .await;

        assert!(result.is_err());
        let old = store.get_by_id(old_memory.id).unwrap().unwrap();
        assert!(old.superseded_by.is_none());
        assert_eq!(store.query_by_type(MemoryType::Fact).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn contradiction_is_resolved_before_duplicate_gate() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = Arc::new(MemoryStore::open(tmp.path()).unwrap());
        let old_memory = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("The API endpoint returns status 200 as of the latest test")
            .scope(MemoryScope::Project)
            .authority(Authority::LLMInference)
            .confidence(Confidence::new(0.7).unwrap())
            .source_agent(RoleId::new("llm"))
            .build()
            .unwrap();
        store.insert(&old_memory).unwrap();

        let qdrant = Arc::new(FakeVectorBackend::default());
        qdrant.results.lock().push((old_memory.id, 0.99));
        let librarian = Librarian::new(store.clone(), qdrant, Duration::from_secs(3600));
        let bus = EventBus::new(16);

        librarian
            .process_proposal(
                &bus,
                "Fact",
                "The API endpoint returns status 500 as of the latest test",
                "Project",
                &RoleId::new("compiler"),
                &RoleId::new("compiler"),
                &[],
                0.95,
                EventId::new(),
                0,
            )
            .await
            .unwrap();

        let old = store.get_by_id(old_memory.id).unwrap().unwrap();
        assert!(old.superseded_by.is_some());
        let chain = store.get_supersession_chain(old_memory.id).unwrap();
        assert_eq!(chain.len(), 2);
    }

    #[tokio::test]
    async fn decay_marker_is_not_decayed_again() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = Arc::new(MemoryStore::open(tmp.path()).unwrap());
        let qdrant = Arc::new(FakeVectorBackend::default());
        let librarian = Librarian::new(store.clone(), qdrant.clone(), Duration::from_secs(3600));
        let bus = EventBus::new(16);

        let stale = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("This stale fact was observed during a test")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .decay_policy(crate::types::DecayPolicy::StaleAfterDays(0))
            .source_agent(RoleId::new("user"))
            .build()
            .unwrap();
        store.insert(&stale).unwrap();

        librarian.run_decay_scan(&bus).await.unwrap();
        assert!(store.query_decayed().unwrap().is_empty());
        assert_eq!(qdrant.deleted.lock().as_slice(), &[stale.id]);

        librarian.run_decay_scan(&bus).await.unwrap();
        assert_eq!(store.query_by_type(MemoryType::Fact).unwrap().len(), 1);
    }
}
