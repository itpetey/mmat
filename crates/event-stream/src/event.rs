//! Semantic event types, identifiers, helper structs, and the free `now_ns` function
//! used throughout the event stream system.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A unique identifier for an event, backed by a UUID v4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

/// A unique identifier for a role (agent) within the system.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoleId(pub String);

/// A reference to an event used as supporting evidence for a claim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRef {
    /// The unique identifier of the referenced event.
    pub event_id: EventId,
    /// A human-readable description of how this evidence supports the claim.
    pub description: String,
}

/// A contract declaring the work to be performed by a worker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskContract {
    /// The unique identifier for this contract.
    pub contract_id: String,
    /// A human-readable description of the work to be performed.
    pub description: String,
}

/// A finding recorded during review of a task's output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewFinding {
    /// A human-readable description of the finding.
    pub finding: String,
    /// The severity level of the finding (e.g. "minor", "major", "critical").
    pub severity: String,
}

/// The severity level of an escalation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EscalationSeverity {
    /// Low priority escalation.
    Low,
    /// Medium priority escalation.
    Medium,
    /// High priority escalation.
    High,
    /// Critical priority escalation requiring immediate attention.
    Critical,
}

/// A reference to a produced artefact (e.g. a file, commit, or pull request).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtefactRef {
    /// The type of artefact (e.g. "file", "commit", "PR").
    pub artefact_type: String,
    /// A reference string identifying the artefact (e.g. path, URL, hash).
    pub reference: String,
}

/// A semantic event representing a meaningful occurrence within the system.
///
/// Every variant carries a unique [`EventId`], the [`RoleId`] of the source agent
/// that produced it, and a `timestamp_ns` in nanoseconds since the UNIX epoch.
/// The [`serde` tag attribute](serde) serialises the variant name as `"variant"`.
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
        token_usage: u64,
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
        evidence_refs: Vec<EvidenceRef>,
        confidence: f64,
    },
    MemoryAccepted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        memory_id: EventId,
        accepted_authority: RoleId,
    },
    MemoryRejected {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        proposed_memory_type: String,
        proposed_content: String,
        rejection_gate: String,
        rejection_reason: String,
    },
    MemorySuperseded {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        old_memory_id: EventId,
        new_memory_id: EventId,
        reason: String,
    },
    EvidenceChainBroken {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        claim_id: EventId,
        broken_ref: EventId,
        claim_text: String,
    },
    ProcessSkipped {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        step: String,
        claim_id: EventId,
    },
    PolicyViolationDetected {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        violation_type: String,
        description: String,
        related_event_id: Option<EventId>,
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
        contract_id: String,
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
    BudgetWarning {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        contract_id: String,
        message: String,
        usage_percent: u8,
    },
    EscalationAccepted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        escalation_event_id: EventId,
        target_role: RoleId,
        chain_depth: u32,
    },
    RoleStateChanged {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        role_id: RoleId,
        old_state: String,
        new_state: String,
    },
    OrganisationStarted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
    },
    OrganisationStopped {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        reason: String,
    },
    Heartbeat {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        active_roles: u32,
        completed_roles: u32,
        failed_roles: u32,
    },
}

/// The set of known semantic event types, used for filtering.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventType {
    /// A tool was executed by an agent.
    ToolExecuted,
    /// A claim was made with supporting evidence.
    ClaimMade,
    /// A decision was recorded with rationale references.
    DecisionRecorded,
    /// A new memory entry was proposed for acceptance.
    MemoryProposed,
    /// A proposed memory was accepted by the appropriate authority.
    MemoryAccepted,
    /// A proposed memory was rejected.
    MemoryRejected,
    /// An existing memory was superseded by a newer one.
    MemorySuperseded,
    /// An evidence chain was detected as broken.
    EvidenceChainBroken,
    /// A process step was skipped.
    ProcessSkipped,
    /// A policy violation was detected.
    PolicyViolationDetected,
    /// A task was assigned to a worker.
    TaskAssigned,
    /// A worker began executing an assigned task.
    TaskStarted,
    /// A task was completed successfully.
    TaskCompleted,
    /// A task failed during execution.
    TaskFailed,
    /// A review was requested for a task's output.
    ReviewRequested,
    /// A review was completed with findings.
    ReviewCompleted,
    /// An escalation was requested to a higher authority.
    EscalationRequested,
    /// Human feedback was requested.
    HumanFeedbackRequested,
    /// Human feedback was received.
    HumanFeedbackReceived,
    /// An artefact was produced.
    ArtefactProduced,
    /// A budget usage warning was issued.
    BudgetWarning,
    /// An escalation was accepted by the target role.
    EscalationAccepted,
    /// A role's state changed.
    RoleStateChanged,
    /// The organisation was started.
    OrganisationStarted,
    /// The organisation was stopped.
    OrganisationStopped,
    /// A heartbeat signal was emitted.
    Heartbeat,
}

impl EventId {
    /// Creates a new random [`EventId`] based on a UUID v4.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EventId {
    /// Returns a new random [`EventId`].
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EventId {
    /// Formats the inner UUID.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for EventId {
    /// Wraps a [`Uuid`] in an [`EventId`].
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl RoleId {
    /// Creates a new [`RoleId`] from any type that can be converted into a [`String`].
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for RoleId {
    /// Formats the inner role identifier string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for RoleId {
    /// Converts a [`String`] into a [`RoleId`].
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for RoleId {
    /// Converts a string slice into a [`RoleId`].
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

impl SemanticEvent {
    /// Constructs a new [`ToolExecuted`](SemanticEvent::ToolExecuted) event.
    ///
    /// Generates a fresh [`EventId`] and stamps the current nanosecond timestamp.
    pub fn new_tool_executed(
        source_agent: RoleId,
        tool_name: impl Into<String>,
        arguments: impl Into<String>,
        exit_code: i32,
        stdout: impl Into<String>,
        stderr: impl Into<String>,
        token_usage: u64,
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
            token_usage,
        }
    }

    /// Constructs a new [`ClaimMade`](SemanticEvent::ClaimMade) event.
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

    /// Constructs a new [`DecisionRecorded`](SemanticEvent::DecisionRecorded) event.
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

    /// Constructs a new [`MemoryProposed`](SemanticEvent::MemoryProposed) event.
    pub fn new_memory_proposed(
        source_agent: RoleId,
        memory_type: impl Into<String>,
        content: impl Into<String>,
        scope: impl Into<String>,
        proposed_authority: RoleId,
        evidence_refs: Vec<EvidenceRef>,
        confidence: f64,
    ) -> Self {
        Self::MemoryProposed {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            memory_type: memory_type.into(),
            content: content.into(),
            scope: scope.into(),
            proposed_authority,
            evidence_refs,
            confidence,
        }
    }

    /// Constructs a new [`MemoryAccepted`](SemanticEvent::MemoryAccepted) event.
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

    /// Constructs a new [`MemoryRejected`](SemanticEvent::MemoryRejected) event.
    pub fn new_memory_rejected(
        source_agent: RoleId,
        proposed_memory_type: impl Into<String>,
        proposed_content: impl Into<String>,
        rejection_gate: impl Into<String>,
        rejection_reason: impl Into<String>,
    ) -> Self {
        Self::MemoryRejected {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            proposed_memory_type: proposed_memory_type.into(),
            proposed_content: proposed_content.into(),
            rejection_gate: rejection_gate.into(),
            rejection_reason: rejection_reason.into(),
        }
    }

    /// Constructs a new [`MemorySuperseded`](SemanticEvent::MemorySuperseded) event.
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

    /// Constructs a new [`EvidenceChainBroken`](SemanticEvent::EvidenceChainBroken) event.
    pub fn new_evidence_chain_broken(
        source_agent: RoleId,
        claim_id: EventId,
        broken_ref: EventId,
        claim_text: impl Into<String>,
    ) -> Self {
        Self::EvidenceChainBroken {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            claim_id,
            broken_ref,
            claim_text: claim_text.into(),
        }
    }

    /// Constructs a new [`ProcessSkipped`](SemanticEvent::ProcessSkipped) event.
    pub fn new_process_skipped(
        source_agent: RoleId,
        step: impl Into<String>,
        claim_id: EventId,
    ) -> Self {
        Self::ProcessSkipped {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            step: step.into(),
            claim_id,
        }
    }

    /// Constructs a new [`PolicyViolationDetected`](SemanticEvent::PolicyViolationDetected) event.
    pub fn new_policy_violation_detected(
        source_agent: RoleId,
        violation_type: impl Into<String>,
        description: impl Into<String>,
        related_event_id: Option<EventId>,
    ) -> Self {
        Self::PolicyViolationDetected {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            violation_type: violation_type.into(),
            description: description.into(),
            related_event_id,
        }
    }

    /// Constructs a new [`TaskAssigned`](SemanticEvent::TaskAssigned) event.
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

    /// Constructs a new [`TaskStarted`](SemanticEvent::TaskStarted) event.
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

    /// Constructs a new [`TaskCompleted`](SemanticEvent::TaskCompleted) event.
    pub fn new_task_completed(
        source_agent: RoleId,
        task_id: impl Into<String>,
        contract_id: impl Into<String>,
        output_artefact: ArtefactRef,
    ) -> Self {
        Self::TaskCompleted {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            task_id: task_id.into(),
            contract_id: contract_id.into(),
            output_artefact,
        }
    }

    /// Constructs a new [`TaskFailed`](SemanticEvent::TaskFailed) event.
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

    /// Constructs a new [`ReviewRequested`](SemanticEvent::ReviewRequested) event.
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

    /// Constructs a new [`ReviewCompleted`](SemanticEvent::ReviewCompleted) event.
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

    /// Constructs a new [`EscalationRequested`](SemanticEvent::EscalationRequested) event.
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

    /// Constructs a new [`HumanFeedbackRequested`](SemanticEvent::HumanFeedbackRequested) event.
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

    /// Constructs a new [`HumanFeedbackReceived`](SemanticEvent::HumanFeedbackReceived) event.
    pub fn new_human_feedback_received(source_agent: RoleId, answer: impl Into<String>) -> Self {
        Self::HumanFeedbackReceived {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            answer: answer.into(),
        }
    }

    /// Constructs a new [`ArtefactProduced`](SemanticEvent::ArtefactProduced) event.
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

    /// Constructs a new [`BudgetWarning`](SemanticEvent::BudgetWarning) event.
    pub fn new_budget_warning(
        source_agent: RoleId,
        contract_id: impl Into<String>,
        message: impl Into<String>,
        usage_percent: u8,
    ) -> Self {
        Self::BudgetWarning {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            contract_id: contract_id.into(),
            message: message.into(),
            usage_percent,
        }
    }

    /// Constructs a new [`EscalationAccepted`](SemanticEvent::EscalationAccepted) event.
    pub fn new_escalation_accepted(
        source_agent: RoleId,
        escalation_event_id: EventId,
        target_role: RoleId,
        chain_depth: u32,
    ) -> Self {
        Self::EscalationAccepted {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            escalation_event_id,
            target_role,
            chain_depth,
        }
    }

    /// Constructs a new [`RoleStateChanged`](SemanticEvent::RoleStateChanged) event.
    pub fn new_role_state_changed(
        source_agent: RoleId,
        role_id: RoleId,
        old_state: impl Into<String>,
        new_state: impl Into<String>,
    ) -> Self {
        Self::RoleStateChanged {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            role_id,
            old_state: old_state.into(),
            new_state: new_state.into(),
        }
    }

    /// Constructs a new [`OrganisationStarted`](SemanticEvent::OrganisationStarted) event.
    pub fn new_organisation_started(source_agent: RoleId) -> Self {
        Self::OrganisationStarted {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
        }
    }

    /// Constructs a new [`OrganisationStopped`](SemanticEvent::OrganisationStopped) event.
    pub fn new_organisation_stopped(source_agent: RoleId, reason: impl Into<String>) -> Self {
        Self::OrganisationStopped {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            reason: reason.into(),
        }
    }

    /// Constructs a new [`Heartbeat`](SemanticEvent::Heartbeat) event.
    pub fn new_heartbeat(
        source_agent: RoleId,
        active_roles: u32,
        completed_roles: u32,
        failed_roles: u32,
    ) -> Self {
        Self::Heartbeat {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            active_roles,
            completed_roles,
            failed_roles,
        }
    }

    /// Returns the [`EventId`] of this event.
    pub fn event_id(&self) -> EventId {
        match self {
            Self::ToolExecuted { event_id, .. } => *event_id,
            Self::ClaimMade { event_id, .. } => *event_id,
            Self::DecisionRecorded { event_id, .. } => *event_id,
            Self::MemoryProposed { event_id, .. } => *event_id,
            Self::MemoryAccepted { event_id, .. } => *event_id,
            Self::MemoryRejected { event_id, .. } => *event_id,
            Self::MemorySuperseded { event_id, .. } => *event_id,
            Self::EvidenceChainBroken { event_id, .. } => *event_id,
            Self::ProcessSkipped { event_id, .. } => *event_id,
            Self::PolicyViolationDetected { event_id, .. } => *event_id,
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
            Self::BudgetWarning { event_id, .. } => *event_id,
            Self::EscalationAccepted { event_id, .. } => *event_id,
            Self::RoleStateChanged { event_id, .. } => *event_id,
            Self::OrganisationStarted { event_id, .. } => *event_id,
            Self::OrganisationStopped { event_id, .. } => *event_id,
            Self::Heartbeat { event_id, .. } => *event_id,
        }
    }

    /// Returns the variant name as a `&'static str` (e.g. `"ToolExecuted"`).
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::ToolExecuted { .. } => "ToolExecuted",
            Self::ClaimMade { .. } => "ClaimMade",
            Self::DecisionRecorded { .. } => "DecisionRecorded",
            Self::MemoryProposed { .. } => "MemoryProposed",
            Self::MemoryAccepted { .. } => "MemoryAccepted",
            Self::MemoryRejected { .. } => "MemoryRejected",
            Self::MemorySuperseded { .. } => "MemorySuperseded",
            Self::EvidenceChainBroken { .. } => "EvidenceChainBroken",
            Self::ProcessSkipped { .. } => "ProcessSkipped",
            Self::PolicyViolationDetected { .. } => "PolicyViolationDetected",
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
            Self::BudgetWarning { .. } => "BudgetWarning",
            Self::EscalationAccepted { .. } => "EscalationAccepted",
            Self::RoleStateChanged { .. } => "RoleStateChanged",
            Self::OrganisationStarted { .. } => "OrganisationStarted",
            Self::OrganisationStopped { .. } => "OrganisationStopped",
            Self::Heartbeat { .. } => "Heartbeat",
        }
    }

    /// Returns the [`EventType`] discriminator for this event.
    pub fn event_type(&self) -> EventType {
        match self {
            Self::ToolExecuted { .. } => EventType::ToolExecuted,
            Self::ClaimMade { .. } => EventType::ClaimMade,
            Self::DecisionRecorded { .. } => EventType::DecisionRecorded,
            Self::MemoryProposed { .. } => EventType::MemoryProposed,
            Self::MemoryAccepted { .. } => EventType::MemoryAccepted,
            Self::MemoryRejected { .. } => EventType::MemoryRejected,
            Self::MemorySuperseded { .. } => EventType::MemorySuperseded,
            Self::EvidenceChainBroken { .. } => EventType::EvidenceChainBroken,
            Self::ProcessSkipped { .. } => EventType::ProcessSkipped,
            Self::PolicyViolationDetected { .. } => EventType::PolicyViolationDetected,
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
            Self::BudgetWarning { .. } => EventType::BudgetWarning,
            Self::EscalationAccepted { .. } => EventType::EscalationAccepted,
            Self::RoleStateChanged { .. } => EventType::RoleStateChanged,
            Self::OrganisationStarted { .. } => EventType::OrganisationStarted,
            Self::OrganisationStopped { .. } => EventType::OrganisationStopped,
            Self::Heartbeat { .. } => EventType::Heartbeat,
        }
    }
}

impl EventType {
    /// Returns the variant name as a `&'static str` (e.g. `"ToolExecuted"`).
    pub fn name(&self) -> &'static str {
        match self {
            Self::ToolExecuted => "ToolExecuted",
            Self::ClaimMade => "ClaimMade",
            Self::DecisionRecorded => "DecisionRecorded",
            Self::MemoryProposed => "MemoryProposed",
            Self::MemoryAccepted => "MemoryAccepted",
            Self::MemoryRejected => "MemoryRejected",
            Self::MemorySuperseded => "MemorySuperseded",
            Self::EvidenceChainBroken => "EvidenceChainBroken",
            Self::ProcessSkipped => "ProcessSkipped",
            Self::PolicyViolationDetected => "PolicyViolationDetected",
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
            Self::BudgetWarning => "BudgetWarning",
            Self::EscalationAccepted => "EscalationAccepted",
            Self::RoleStateChanged => "RoleStateChanged",
            Self::OrganisationStarted => "OrganisationStarted",
            Self::OrganisationStopped => "OrganisationStopped",
            Self::Heartbeat => "Heartbeat",
        }
    }
}

/// Returns the current system time as nanoseconds since the UNIX epoch.
pub fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
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
            0,
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
            EventType::MemoryRejected.name(),
            EventType::MemorySuperseded.name(),
            EventType::EvidenceChainBroken.name(),
            EventType::ProcessSkipped.name(),
            EventType::PolicyViolationDetected.name(),
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
            EventType::BudgetWarning.name(),
            EventType::EscalationAccepted.name(),
            EventType::RoleStateChanged.name(),
            EventType::OrganisationStarted.name(),
            EventType::OrganisationStopped.name(),
            EventType::Heartbeat.name(),
        ];
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(names.len(), unique.len());
    }
}
