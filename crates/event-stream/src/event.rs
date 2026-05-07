use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

impl EventId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for EventId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoleId(pub String);

impl RoleId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for RoleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for RoleId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for RoleId {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub event_id: EventId,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskContract {
    pub contract_id: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewFinding {
    pub finding: String,
    pub severity: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EscalationSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtefactRef {
    pub artefact_type: String,
    pub reference: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "variant")]
pub enum SemanticEvent {
    ToolExecuted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        tool_name: String,
        arguments: String,
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
    ClaimMade {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        claim_text: String,
        evidence_refs: Vec<EvidenceRef>,
        confidence_score: f32,
    },
    DecisionRecorded {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        decision_text: String,
        rationale_refs: Vec<EvidenceRef>,
    },
    MemoryProposed {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        memory_type: String,
        content: String,
        scope: String,
        proposed_authority: RoleId,
    },
    MemoryAccepted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        memory_id: EventId,
        accepted_authority: RoleId,
    },
    MemorySuperseded {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        old_memory_id: EventId,
        new_memory_id: EventId,
        reason: String,
    },
    TaskAssigned {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        task_id: String,
        worker_id: RoleId,
        contract_ref: TaskContract,
        dependencies: Vec<String>,
    },
    TaskStarted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        task_id: String,
        worker_id: RoleId,
    },
    TaskCompleted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        task_id: String,
        output_artefact: ArtefactRef,
    },
    TaskFailed {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        task_id: String,
        error_description: String,
    },
    ReviewRequested {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        task_id: String,
        reviewer_id: RoleId,
    },
    ReviewCompleted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        task_id: String,
        findings: Vec<ReviewFinding>,
        accepted: bool,
    },
    EscalationRequested {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        from_role: RoleId,
        to_role: RoleId,
        reason: String,
        severity: EscalationSeverity,
    },
    HumanFeedbackRequested {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        question: String,
        context: String,
    },
    HumanFeedbackReceived {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        answer: String,
    },
    ArtefactProduced {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        artefact_type: String,
        reference: String,
        producer_role: RoleId,
    },
}

impl SemanticEvent {
    pub fn new_tool_executed(
        source_agent: RoleId,
        tool_name: impl Into<String>,
        arguments: impl Into<String>,
        exit_code: i32,
        stdout: impl Into<String>,
        stderr: impl Into<String>,
    ) -> Self {
        Self::ToolExecuted {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            tool_name: tool_name.into(),
            arguments: arguments.into(),
            exit_code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    pub fn new_claim_made(
        source_agent: RoleId,
        claim_text: impl Into<String>,
        evidence_refs: Vec<EvidenceRef>,
        confidence_score: f32,
    ) -> Self {
        Self::ClaimMade {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            claim_text: claim_text.into(),
            evidence_refs,
            confidence_score,
        }
    }

    pub fn new_decision_recorded(
        source_agent: RoleId,
        decision_text: impl Into<String>,
        rationale_refs: Vec<EvidenceRef>,
    ) -> Self {
        Self::DecisionRecorded {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            decision_text: decision_text.into(),
            rationale_refs,
        }
    }

    pub fn new_memory_proposed(
        source_agent: RoleId,
        memory_type: impl Into<String>,
        content: impl Into<String>,
        scope: impl Into<String>,
        proposed_authority: RoleId,
    ) -> Self {
        Self::MemoryProposed {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            memory_type: memory_type.into(),
            content: content.into(),
            scope: scope.into(),
            proposed_authority,
        }
    }

    pub fn new_memory_accepted(
        source_agent: RoleId,
        memory_id: EventId,
        accepted_authority: RoleId,
    ) -> Self {
        Self::MemoryAccepted {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            memory_id,
            accepted_authority,
        }
    }

    pub fn new_memory_superseded(
        source_agent: RoleId,
        old_memory_id: EventId,
        new_memory_id: EventId,
        reason: impl Into<String>,
    ) -> Self {
        Self::MemorySuperseded {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            old_memory_id,
            new_memory_id,
            reason: reason.into(),
        }
    }

    pub fn new_task_assigned(
        source_agent: RoleId,
        task_id: impl Into<String>,
        worker_id: RoleId,
        contract_ref: TaskContract,
        dependencies: Vec<String>,
    ) -> Self {
        Self::TaskAssigned {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            task_id: task_id.into(),
            worker_id,
            contract_ref,
            dependencies,
        }
    }

    pub fn new_task_started(
        source_agent: RoleId,
        task_id: impl Into<String>,
        worker_id: RoleId,
    ) -> Self {
        Self::TaskStarted {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            task_id: task_id.into(),
            worker_id,
        }
    }

    pub fn new_task_completed(
        source_agent: RoleId,
        task_id: impl Into<String>,
        output_artefact: ArtefactRef,
    ) -> Self {
        Self::TaskCompleted {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            task_id: task_id.into(),
            output_artefact,
        }
    }

    pub fn new_task_failed(
        source_agent: RoleId,
        task_id: impl Into<String>,
        error_description: impl Into<String>,
    ) -> Self {
        Self::TaskFailed {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            task_id: task_id.into(),
            error_description: error_description.into(),
        }
    }

    pub fn new_review_requested(
        source_agent: RoleId,
        task_id: impl Into<String>,
        reviewer_id: RoleId,
    ) -> Self {
        Self::ReviewRequested {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            task_id: task_id.into(),
            reviewer_id,
        }
    }

    pub fn new_review_completed(
        source_agent: RoleId,
        task_id: impl Into<String>,
        findings: Vec<ReviewFinding>,
        accepted: bool,
    ) -> Self {
        Self::ReviewCompleted {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            task_id: task_id.into(),
            findings,
            accepted,
        }
    }

    pub fn new_escalation_requested(
        source_agent: RoleId,
        from_role: RoleId,
        to_role: RoleId,
        reason: impl Into<String>,
        severity: EscalationSeverity,
    ) -> Self {
        Self::EscalationRequested {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            from_role,
            to_role,
            reason: reason.into(),
            severity,
        }
    }

    pub fn new_human_feedback_requested(
        source_agent: RoleId,
        question: impl Into<String>,
        context: impl Into<String>,
    ) -> Self {
        Self::HumanFeedbackRequested {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            question: question.into(),
            context: context.into(),
        }
    }

    pub fn new_human_feedback_received(source_agent: RoleId, answer: impl Into<String>) -> Self {
        Self::HumanFeedbackReceived {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            answer: answer.into(),
        }
    }

    pub fn new_artefact_produced(
        source_agent: RoleId,
        artefact_type: impl Into<String>,
        reference: impl Into<String>,
        producer_role: RoleId,
    ) -> Self {
        Self::ArtefactProduced {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            artefact_type: artefact_type.into(),
            reference: reference.into(),
            producer_role,
        }
    }

    pub fn event_id(&self) -> EventId {
        match self {
            Self::ToolExecuted { event_id, .. } => *event_id,
            Self::ClaimMade { event_id, .. } => *event_id,
            Self::DecisionRecorded { event_id, .. } => *event_id,
            Self::MemoryProposed { event_id, .. } => *event_id,
            Self::MemoryAccepted { event_id, .. } => *event_id,
            Self::MemorySuperseded { event_id, .. } => *event_id,
            Self::TaskAssigned { event_id, .. } => *event_id,
            Self::TaskStarted { event_id, .. } => *event_id,
            Self::TaskCompleted { event_id, .. } => *event_id,
            Self::TaskFailed { event_id, .. } => *event_id,
            Self::ReviewRequested { event_id, .. } => *event_id,
            Self::ReviewCompleted { event_id, .. } => *event_id,
            Self::EscalationRequested { event_id, .. } => *event_id,
            Self::HumanFeedbackRequested { event_id, .. } => *event_id,
            Self::HumanFeedbackReceived { event_id, .. } => *event_id,
            Self::ArtefactProduced { event_id, .. } => *event_id,
        }
    }

    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::ToolExecuted { .. } => "ToolExecuted",
            Self::ClaimMade { .. } => "ClaimMade",
            Self::DecisionRecorded { .. } => "DecisionRecorded",
            Self::MemoryProposed { .. } => "MemoryProposed",
            Self::MemoryAccepted { .. } => "MemoryAccepted",
            Self::MemorySuperseded { .. } => "MemorySuperseded",
            Self::TaskAssigned { .. } => "TaskAssigned",
            Self::TaskStarted { .. } => "TaskStarted",
            Self::TaskCompleted { .. } => "TaskCompleted",
            Self::TaskFailed { .. } => "TaskFailed",
            Self::ReviewRequested { .. } => "ReviewRequested",
            Self::ReviewCompleted { .. } => "ReviewCompleted",
            Self::EscalationRequested { .. } => "EscalationRequested",
            Self::HumanFeedbackRequested { .. } => "HumanFeedbackRequested",
            Self::HumanFeedbackReceived { .. } => "HumanFeedbackReceived",
            Self::ArtefactProduced { .. } => "ArtefactProduced",
        }
    }

    pub fn event_type(&self) -> EventType {
        match self {
            Self::ToolExecuted { .. } => EventType::ToolExecuted,
            Self::ClaimMade { .. } => EventType::ClaimMade,
            Self::DecisionRecorded { .. } => EventType::DecisionRecorded,
            Self::MemoryProposed { .. } => EventType::MemoryProposed,
            Self::MemoryAccepted { .. } => EventType::MemoryAccepted,
            Self::MemorySuperseded { .. } => EventType::MemorySuperseded,
            Self::TaskAssigned { .. } => EventType::TaskAssigned,
            Self::TaskStarted { .. } => EventType::TaskStarted,
            Self::TaskCompleted { .. } => EventType::TaskCompleted,
            Self::TaskFailed { .. } => EventType::TaskFailed,
            Self::ReviewRequested { .. } => EventType::ReviewRequested,
            Self::ReviewCompleted { .. } => EventType::ReviewCompleted,
            Self::EscalationRequested { .. } => EventType::EscalationRequested,
            Self::HumanFeedbackRequested { .. } => EventType::HumanFeedbackRequested,
            Self::HumanFeedbackReceived { .. } => EventType::HumanFeedbackReceived,
            Self::ArtefactProduced { .. } => EventType::ArtefactProduced,
        }
    }
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EventType {
    ToolExecuted,
    ClaimMade,
    DecisionRecorded,
    MemoryProposed,
    MemoryAccepted,
    MemorySuperseded,
    TaskAssigned,
    TaskStarted,
    TaskCompleted,
    TaskFailed,
    ReviewRequested,
    ReviewCompleted,
    EscalationRequested,
    HumanFeedbackRequested,
    HumanFeedbackReceived,
    ArtefactProduced,
}

impl EventType {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ToolExecuted => "ToolExecuted",
            Self::ClaimMade => "ClaimMade",
            Self::DecisionRecorded => "DecisionRecorded",
            Self::MemoryProposed => "MemoryProposed",
            Self::MemoryAccepted => "MemoryAccepted",
            Self::MemorySuperseded => "MemorySuperseded",
            Self::TaskAssigned => "TaskAssigned",
            Self::TaskStarted => "TaskStarted",
            Self::TaskCompleted => "TaskCompleted",
            Self::TaskFailed => "TaskFailed",
            Self::ReviewRequested => "ReviewRequested",
            Self::ReviewCompleted => "ReviewCompleted",
            Self::EscalationRequested => "EscalationRequested",
            Self::HumanFeedbackRequested => "HumanFeedbackRequested",
            Self::HumanFeedbackReceived => "HumanFeedbackReceived",
            Self::ArtefactProduced => "ArtefactProduced",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_id_round_trip() {
        let id = EventId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: EventId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn semantic_event_serialises() {
        let event = SemanticEvent::new_tool_executed(
            RoleId::new("worker"),
            "test_tool",
            "{}",
            0,
            "out",
            "err",
        );
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("ToolExecuted"));
        let back: SemanticEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event.event_id(), back.event_id());
    }

    #[test]
    fn all_variants_have_unique_names() {
        let names = vec![
            EventType::ToolExecuted.name(),
            EventType::ClaimMade.name(),
            EventType::DecisionRecorded.name(),
            EventType::MemoryProposed.name(),
            EventType::MemoryAccepted.name(),
            EventType::MemorySuperseded.name(),
            EventType::TaskAssigned.name(),
            EventType::TaskStarted.name(),
            EventType::TaskCompleted.name(),
            EventType::TaskFailed.name(),
            EventType::ReviewRequested.name(),
            EventType::ReviewCompleted.name(),
            EventType::EscalationRequested.name(),
            EventType::HumanFeedbackRequested.name(),
            EventType::HumanFeedbackReceived.name(),
            EventType::ArtefactProduced.name(),
        ];
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(names.len(), unique.len());
    }
}
