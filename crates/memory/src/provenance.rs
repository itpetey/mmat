use std::collections::HashMap;
use std::sync::Arc;

use event_stream::event::{EventId, EventType, SemanticEvent};
use event_stream::event_bus::EventBus;
use event_stream::event_store::EventStore;
use parking_lot::RwLock;

use crate::error::Result;
use crate::types::Memory;

pub struct ProvenanceEngine {
    evidence_index: RwLock<HashMap<EventId, Vec<EventId>>>,
}

impl ProvenanceEngine {
    pub fn new() -> Self {
        Self {
            evidence_index: RwLock::new(HashMap::new()),
        }
    }

    pub async fn run(&self, bus: Arc<EventBus>, event_store: Arc<EventStore>) -> Result<()> {
        self.rebuild_index_from_store(&event_store)?;

        let mut rx = bus.subscribe(&[
            EventType::ClaimMade,
            EventType::DecisionRecorded,
            EventType::MemoryAccepted,
            EventType::ToolExecuted,
        ]);

        loop {
            let event = match rx.recv().await {
                Ok(event) => event,
                Err(_) => break,
            };

            self.index_event(&event);

            let broken = self.check_broken_evidence_for_event(&event, &event_store)?;
            if !broken.is_empty() {
                self.publish_broken_evidence(&bus, &event, &broken).await;
            }
        }

        Ok(())
    }

    fn rebuild_index_from_store(&self, event_store: &EventStore) -> Result<()> {
        let relevant_types = [
            "ClaimMade",
            "DecisionRecorded",
            "MemoryAccepted",
            "ToolExecuted",
        ];

        for variant in relevant_types {
            let events = event_store.query_by_variant(variant, None, None)?;

            for event in events {
                self.index_event(&event);
            }
        }

        Ok(())
    }

    fn index_event(&self, event: &SemanticEvent) {
        let event_id = event.event_id();

        let evidence_refs: Vec<EventId> = match event {
            SemanticEvent::ClaimMade { evidence_refs, .. } => {
                evidence_refs.iter().map(|er| er.event_id).collect()
            }
            SemanticEvent::DecisionRecorded { rationale_refs, .. } => {
                rationale_refs.iter().map(|er| er.event_id).collect()
            }
            _ => return,
        };

        if !evidence_refs.is_empty() {
            let mut index = self.evidence_index.write();
            index.insert(event_id, evidence_refs);
        }
    }

    pub fn trace_evidence(
        &self,
        event_id: EventId,
        event_store: &EventStore,
    ) -> Result<Vec<SemanticEvent>> {
        let mut chain = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![event_id];

        while let Some(current_id) = stack.pop() {
            if visited.contains(&current_id) {
                continue;
            }
            visited.insert(current_id);

            let row = event_store.row_for_event_id(current_id)?;
            if let Some(_rowid) = row {
                let events = event_store.replay(0, None)?;
                if let Some(event) = events.into_iter().find(|e| e.event_id() == current_id) {
                    chain.push(event.clone());

                    let refs: Vec<EventId> = match &event {
                        SemanticEvent::ClaimMade { evidence_refs, .. } => {
                            evidence_refs.iter().map(|er| er.event_id).collect()
                        }
                        SemanticEvent::DecisionRecorded { rationale_refs, .. } => {
                            rationale_refs.iter().map(|er| er.event_id).collect()
                        }
                        _ => Vec::new(),
                    };

                    for ref_id in refs {
                        if !visited.contains(&ref_id) {
                            stack.push(ref_id);
                        }
                    }
                }
            }
        }

        chain.reverse();
        Ok(chain)
    }

    pub fn trace_memory(
        &self,
        memory: &Memory,
        event_store: &EventStore,
    ) -> Result<Vec<SemanticEvent>> {
        let mut all_events = Vec::new();

        for evidence_ref in &memory.evidence_refs {
            let events = self.trace_evidence(*evidence_ref, event_store)?;
            all_events.extend(events);
        }

        all_events.sort_by_key(|e| e.event_id().0);
        all_events.dedup_by_key(|e| e.event_id().0);

        Ok(all_events)
    }

    pub fn assess_confidence(&self, event_id: EventId, event_store: &EventStore) -> Result<f64> {
        let events = event_store.replay(0, None)?;
        let event = events.into_iter().find(|e| e.event_id() == event_id);

        let Some(event) = event else {
            return Ok(0.0);
        };

        let evidence_refs: Vec<EventId> = match &event {
            SemanticEvent::ClaimMade { evidence_refs, .. } => {
                evidence_refs.iter().map(|er| er.event_id).collect()
            }
            _ => Vec::new(),
        };

        if evidence_refs.is_empty() {
            return Ok(0.2);
        }

        let mut has_direct_tool = false;
        let mut has_indirect_claim = false;

        let all_events = event_store.replay(0, None)?;

        for ref_id in &evidence_refs {
            if let Some(ref_ev) = all_events.iter().find(|e| e.event_id() == *ref_id) {
                match ref_ev {
                    SemanticEvent::ToolExecuted { exit_code: 0, .. } => {
                        has_direct_tool = true;
                    }
                    SemanticEvent::ToolExecuted { .. } => {}
                    SemanticEvent::ClaimMade { .. } => {
                        has_indirect_claim = true;
                    }
                    _ => {}
                }
            }
        }

        if has_direct_tool {
            Ok(0.85)
        } else if has_indirect_claim {
            Ok(0.5)
        } else {
            Ok(0.2)
        }
    }

    fn check_broken_evidence_for_event(
        &self,
        event: &SemanticEvent,
        event_store: &EventStore,
    ) -> Result<Vec<EventId>> {
        let evidence_refs: Vec<EventId> = match event {
            SemanticEvent::ClaimMade { evidence_refs, .. } => {
                evidence_refs.iter().map(|er| er.event_id).collect()
            }
            _ => return Ok(Vec::new()),
        };

        let all_events = event_store.replay(0, None)?;
        let existing_ids: std::collections::HashSet<EventId> =
            all_events.iter().map(|e| e.event_id()).collect();

        let mut broken = Vec::new();
        for ref_id in &evidence_refs {
            if !existing_ids.contains(ref_id) {
                broken.push(*ref_id);
            }
        }

        Ok(broken)
    }

    pub fn check_broken_evidence(
        &self,
        event_id: EventId,
        event_store: &EventStore,
    ) -> Result<Vec<EventId>> {
        let events = event_store.replay(0, None)?;
        let event = events.into_iter().find(|e| e.event_id() == event_id);

        let Some(event) = event else {
            return Ok(Vec::new());
        };

        self.check_broken_evidence_for_event(&event, event_store)
    }

    async fn publish_broken_evidence(
        &self,
        bus: &EventBus,
        event: &SemanticEvent,
        broken_refs: &[EventId],
    ) {
        let source_agent = match event {
            SemanticEvent::ClaimMade { source_agent, .. }
            | SemanticEvent::DecisionRecorded { source_agent, .. } => source_agent.clone(),
            _ => event_stream::event::RoleId::new("unknown"),
        };

        let description = format!(
            "Claim references {} non-existent event(s): {:?}",
            broken_refs.len(),
            broken_refs
        );

        let violation = SemanticEvent::PolicyViolationDetected {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: event_stream::event::now_ns(),
            violation_type: "broken_evidence".to_string(),
            description,
            related_event_id: Some(event.event_id()),
        };
        if let Err(e) = bus.publish(violation) {
            tracing::error!("Failed to publish PolicyViolationDetected: {}", e);
        }
    }

    #[cfg(test)]
    pub fn evidence_index(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, HashMap<EventId, Vec<EventId>>> {
        self.evidence_index.read()
    }
}

impl Default for ProvenanceEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use event_stream::event::{EvidenceRef, RoleId};

    #[test]
    fn provenance_engine_new() {
        let engine = ProvenanceEngine::new();
        assert!(engine.evidence_index().is_empty());
    }

    #[test]
    fn index_event_from_claim_made() {
        let engine = ProvenanceEngine::new();
        let tool_event_id = EventId::new();
        let evidence_ref = EvidenceRef {
            event_id: tool_event_id,
            description: "tool output".to_string(),
        };
        let claim = SemanticEvent::new_claim_made(
            RoleId::new("llm"),
            "test claim",
            vec![evidence_ref],
            0.8,
        );

        engine.index_event(&claim);

        let index = engine.evidence_index();
        assert!(index.contains_key(&claim.event_id()));
        assert_eq!(index[&claim.event_id()].len(), 1);
    }

    #[test]
    fn assess_confidence_no_evidence() {
        let engine = ProvenanceEngine::new();
        let event =
            SemanticEvent::new_claim_made(RoleId::new("llm"), "ungrounded claim", vec![], 0.5);

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = EventStore::open(tmp.path()).unwrap();
        store.insert(&event).unwrap();

        let confidence = engine.assess_confidence(event.event_id(), &store).unwrap();
        assert!(confidence <= 0.3);
    }

    #[test]
    fn assess_confidence_with_tool_evidence() {
        let engine = ProvenanceEngine::new();
        let tool_event =
            SemanticEvent::new_tool_executed(RoleId::new("worker"), "test", "{}", 0, "success", "");
        let tool_event_id = tool_event.event_id();

        let evidence_ref = EvidenceRef {
            event_id: tool_event_id,
            description: "tool output".to_string(),
        };
        let claim = SemanticEvent::new_claim_made(
            RoleId::new("llm"),
            "test claim",
            vec![evidence_ref],
            0.8,
        );

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = EventStore::open(tmp.path()).unwrap();
        store.insert(&tool_event).unwrap();
        store.insert(&claim).unwrap();

        let confidence = engine.assess_confidence(claim.event_id(), &store).unwrap();
        assert!(confidence >= 0.8);
    }
}
