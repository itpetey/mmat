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

/// A unique identifier for a memory, kept distinct from [`EventId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryId(pub Uuid);

/// Scope and causal metadata attached to every semantic event.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventContext {
    /// Organisation boundary used to prevent cross-organisation memory pollution.
    pub organisation_id: String,
    /// Workspace, department, or discipline boundary within the organisation.
    pub workspace_id: String,
    /// Project boundary for durable project memory.
    pub project_id: String,
    /// Execution run boundary for operational memory.
    pub run_id: String,
    /// Optional task boundary for events emitted during a task.
    pub task_id: Option<String>,
    /// Immediate causal event, if known.
    pub causation_id: Option<EventId>,
    /// Correlation identifier that groups related events across roles.
    pub correlation_id: Option<EventId>,
}

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

/// Where a produced artefact is materialised.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtefactStorageKind {
    /// Payload is stored in the artefact blob store.
    #[default]
    Blob,
    /// Output is materialised in a project repository or worktree.
    Code,
}

/// Repository/worktree metadata for generated code outputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryOutputRef {
    /// Path to the project repository root used to create the worktree.
    pub repository_path: String,
    /// Path to the worktree where generated code was written.
    pub worktree_path: String,
    /// Branch associated with the worktree.
    pub worktree_branch: String,
    /// Repository-relative paths changed by the implementation.
    pub paths: Vec<String>,
    /// Human-readable summary of the generated diff or patch.
    pub diff_summary: String,
    /// Summary of validation checks, when available.
    pub validation_summary: Option<String>,
    /// Repository revision, commit, or tree state when available.
    pub revision: Option<String>,
}

/// Reference to an artefact blob stored outside the event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredArtefactRef {
    /// Stable artefact identity used in events and task outputs.
    pub artefact_id: String,
    /// Hash of the stored content.
    pub content_hash: String,
    /// URI pointing to the stored blob.
    pub storage_uri: String,
}

impl Default for EventContext {
    fn default() -> Self {
        Self {
            organisation_id: "default-organisation".to_string(),
            workspace_id: "default-workspace".to_string(),
            project_id: "default-project".to_string(),
            run_id: "default-run".to_string(),
            task_id: None,
            causation_id: None,
            correlation_id: None,
        }
    }
}

impl EventContext {
    /// Creates a context for a concrete organisation/workspace/project/run boundary.
    pub fn new(
        organisation_id: impl Into<String>,
        workspace_id: impl Into<String>,
        project_id: impl Into<String>,
        run_id: impl Into<String>,
    ) -> Self {
        Self {
            organisation_id: organisation_id.into(),
            workspace_id: workspace_id.into(),
            project_id: project_id.into(),
            run_id: run_id.into(),
            task_id: None,
            causation_id: None,
            correlation_id: None,
        }
    }

    /// Attaches a task boundary to the context.
    pub fn with_task_id(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    /// Attaches causal and correlation identifiers to the context.
    pub fn with_causality(
        mut self,
        causation_id: Option<EventId>,
        correlation_id: Option<EventId>,
    ) -> Self {
        self.causation_id = causation_id;
        self.correlation_id = correlation_id;
        self
    }
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
        #[serde(default)]
        context: EventContext,
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
        #[serde(default)]
        context: EventContext,
        claim_text: String,
        evidence_refs: Vec<EvidenceRef>,
        confidence_score: f32,
    },
    DecisionRecorded {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        decision_text: String,
        rationale_refs: Vec<EvidenceRef>,
    },
    MemoryProposed {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
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
        #[serde(default)]
        context: EventContext,
        memory_id: MemoryId,
        proposal_event_id: EventId,
        accepted_authority: RoleId,
    },
    MemoryRejected {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        proposed_memory_type: String,
        proposed_content: String,
        rejection_gate: String,
        rejection_reason: String,
    },
    MemorySuperseded {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        old_memory_id: MemoryId,
        new_memory_id: MemoryId,
        reason: String,
    },
    EvidenceChainBroken {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        claim_id: EventId,
        broken_ref: EventId,
        claim_text: String,
    },
    ProcessSkipped {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        step: String,
        claim_id: EventId,
    },
    PolicyViolationDetected {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        violation_type: String,
        description: String,
        related_event_id: Option<EventId>,
    },
    TaskAssigned {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        task_id: String,
        worker_id: RoleId,
        contract_ref: TaskContract,
        dependencies: Vec<String>,
    },
    TaskStarted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        task_id: String,
        worker_id: RoleId,
    },
    TaskCompleted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        task_id: String,
        contract_id: String,
        output_artefact: ArtefactRef,
    },
    TaskFailed {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        task_id: String,
        error_description: String,
    },
    ReviewRequested {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        task_id: String,
        reviewer_id: RoleId,
    },
    ReviewCompleted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        task_id: String,
        findings: Vec<ReviewFinding>,
        accepted: bool,
    },
    EscalationRequested {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        from_role: RoleId,
        to_role: RoleId,
        reason: String,
        severity: EscalationSeverity,
    },
    HumanFeedbackRequested {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        question: String,
        request_context: String,
    },
    HumanFeedbackReceived {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        answer: String,
    },
    ArtefactProduced {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        artefact_id: String,
        artefact_type: String,
        content_hash: String,
        storage_uri: String,
        producer_role: RoleId,
        evidence_refs: Vec<EvidenceRef>,
        #[serde(default)]
        storage_kind: ArtefactStorageKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repository_output: Option<RepositoryOutputRef>,
    },
    BudgetWarning {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        contract_id: String,
        message: String,
        usage_percent: u8,
    },
    EscalationAccepted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        escalation_event_id: EventId,
        target_role: RoleId,
        chain_depth: u32,
    },
    RoleStateChanged {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        role_id: RoleId,
        old_state: String,
        new_state: String,
    },
    OrganisationStarted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
    },
    OrganisationStopped {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        reason: String,
    },
    Heartbeat {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        active_roles: u32,
        completed_roles: u32,
        failed_roles: u32,
    },
    LaneCreated {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        lane_id: String,
        name: String,
        #[serde(default)]
        kind: String,
        #[serde(default)]
        colour: String,
        #[serde(default)]
        purpose: String,
        #[serde(default)]
        parent_lane_id: Option<String>,
        #[serde(default)]
        related_lane_ids: Vec<String>,
        #[serde(default)]
        source_message_id: Option<String>,
    },
    LaneArchived {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        lane_id: String,
    },
    LanePaused {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        lane_id: String,
    },
    ActionRequestCreated {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        request_id: String,
        request_kind: String,
        prompt: String,
        #[serde(default)]
        choices: Vec<String>,
        #[serde(default)]
        lane_id: Option<String>,
    },
    ActionRequestResolved {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        request_id: String,
        choice: String,
    },
    ActionRequestCancelled {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        request_id: String,
        reason: String,
    },
    ProjectCreated {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        project_id: String,
        host_work_dir: String,
    },
    ProjectListed {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        project_id: String,
        path: String,
    },
    ProjectRenamed {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        project_id: String,
        old_name: String,
        new_name: String,
    },
    ProjectDeleted {
        event_id: EventId,
        source_agent: RoleId,
        timestamp_ns: u64,
        #[serde(default)]
        context: EventContext,
        project_id: String,
        name: String,
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
    /// A conversation lane was created.
    LaneCreated,
    /// A conversation lane was archived.
    LaneArchived,
    /// A conversation lane was paused.
    LanePaused,
    /// An action request was created for human input.
    ActionRequestCreated,
    /// An action request was resolved by human input.
    ActionRequestResolved,
    /// An action request was cancelled.
    ActionRequestCancelled,
    /// A project was created.
    ProjectCreated,
    /// A project was discovered on the filesystem.
    ProjectListed,
    /// A project was renamed.
    ProjectRenamed,
    /// A project was deleted.
    ProjectDeleted,
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

impl MemoryId {
    /// Creates a new random [`MemoryId`] based on a UUID v4.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for MemoryId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MemoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for MemoryId {
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
        memory_id: MemoryId,
        proposal_event_id: EventId,
        accepted_authority: RoleId,
    ) -> Self {
        Self::MemoryAccepted {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            memory_id,
            proposal_event_id,
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
            context: EventContext::default(),
            proposed_memory_type: proposed_memory_type.into(),
            proposed_content: proposed_content.into(),
            rejection_gate: rejection_gate.into(),
            rejection_reason: rejection_reason.into(),
        }
    }

    /// Constructs a new [`MemorySuperseded`](SemanticEvent::MemorySuperseded) event.
    pub fn new_memory_superseded(
        source_agent: RoleId,
        old_memory_id: MemoryId,
        new_memory_id: MemoryId,
        reason: impl Into<String>,
    ) -> Self {
        Self::MemorySuperseded {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
        request_context: impl Into<String>,
    ) -> Self {
        Self::HumanFeedbackRequested {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            question: question.into(),
            request_context: request_context.into(),
        }
    }

    /// Constructs a new [`HumanFeedbackReceived`](SemanticEvent::HumanFeedbackReceived) event.
    pub fn new_human_feedback_received(source_agent: RoleId, answer: impl Into<String>) -> Self {
        Self::HumanFeedbackReceived {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            answer: answer.into(),
        }
    }

    /// Constructs a new [`ArtefactProduced`](SemanticEvent::ArtefactProduced) event.
    pub fn new_artefact_produced(
        source_agent: RoleId,
        artefact_type: impl Into<String>,
        storage_uri: impl Into<String>,
        producer_role: RoleId,
    ) -> Self {
        let storage_uri = storage_uri.into();
        Self::ArtefactProduced {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            artefact_id: format!("artefact-{}", Uuid::new_v4()),
            artefact_type: artefact_type.into(),
            content_hash: stable_content_hash(&storage_uri),
            storage_uri,
            producer_role,
            evidence_refs: Vec::new(),
            storage_kind: ArtefactStorageKind::Blob,
            repository_output: None,
        }
    }

    /// Constructs an [`ArtefactProduced`](SemanticEvent::ArtefactProduced) event
    /// with explicit blob identity, hash, storage URI, and evidence references.
    pub fn new_artefact_produced_ref(
        source_agent: RoleId,
        artefact_id: impl Into<String>,
        artefact_type: impl Into<String>,
        content_hash: impl Into<String>,
        storage_uri: impl Into<String>,
        producer_role: RoleId,
        evidence_refs: Vec<EvidenceRef>,
    ) -> Self {
        Self::ArtefactProduced {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            artefact_id: artefact_id.into(),
            artefact_type: artefact_type.into(),
            content_hash: content_hash.into(),
            storage_uri: storage_uri.into(),
            producer_role,
            evidence_refs,
            storage_kind: ArtefactStorageKind::Blob,
            repository_output: None,
        }
    }

    /// Constructs an [`ArtefactProduced`](SemanticEvent::ArtefactProduced) event
    /// for code materialised in a repository or worktree.
    pub fn new_code_output_ref(
        source_agent: RoleId,
        artefact_type: impl Into<String>,
        stored: StoredArtefactRef,
        producer_role: RoleId,
        evidence_refs: Vec<EvidenceRef>,
        repository_output: RepositoryOutputRef,
    ) -> Self {
        Self::ArtefactProduced {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            artefact_id: stored.artefact_id,
            artefact_type: artefact_type.into(),
            content_hash: stored.content_hash,
            storage_uri: stored.storage_uri,
            producer_role,
            evidence_refs,
            storage_kind: ArtefactStorageKind::Code,
            repository_output: Some(repository_output),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
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
            context: EventContext::default(),
        }
    }

    /// Constructs a new [`OrganisationStopped`](SemanticEvent::OrganisationStopped) event.
    pub fn new_organisation_stopped(source_agent: RoleId, reason: impl Into<String>) -> Self {
        Self::OrganisationStopped {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
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
            context: EventContext::default(),
            active_roles,
            completed_roles,
            failed_roles,
        }
    }

    /// Constructs a new [`LaneCreated`](SemanticEvent::LaneCreated) event.
    #[allow(clippy::too_many_arguments)]
    pub fn new_lane_created(
        source_agent: RoleId,
        lane_id: impl Into<String>,
        name: impl Into<String>,
        kind: impl Into<String>,
        colour: impl Into<String>,
        purpose: impl Into<String>,
        parent_lane_id: Option<String>,
        related_lane_ids: Vec<String>,
        source_message_id: Option<String>,
    ) -> Self {
        Self::LaneCreated {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            lane_id: lane_id.into(),
            name: name.into(),
            kind: kind.into(),
            colour: colour.into(),
            purpose: purpose.into(),
            parent_lane_id,
            related_lane_ids,
            source_message_id,
        }
    }

    /// Constructs a new [`LaneArchived`](SemanticEvent::LaneArchived) event.
    pub fn new_lane_archived(source_agent: RoleId, lane_id: impl Into<String>) -> Self {
        Self::LaneArchived {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            lane_id: lane_id.into(),
        }
    }

    /// Constructs a new [`LanePaused`](SemanticEvent::LanePaused) event.
    pub fn new_lane_paused(source_agent: RoleId, lane_id: impl Into<String>) -> Self {
        Self::LanePaused {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            lane_id: lane_id.into(),
        }
    }

    /// Constructs a new [`ActionRequestCreated`](SemanticEvent::ActionRequestCreated) event.
    pub fn new_action_request_created(
        source_agent: RoleId,
        request_id: impl Into<String>,
        request_kind: impl Into<String>,
        prompt: impl Into<String>,
        choices: Vec<String>,
        lane_id: Option<String>,
    ) -> Self {
        Self::ActionRequestCreated {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            request_id: request_id.into(),
            request_kind: request_kind.into(),
            prompt: prompt.into(),
            choices,
            lane_id,
        }
    }

    /// Constructs a new [`ActionRequestResolved`](SemanticEvent::ActionRequestResolved) event.
    pub fn new_action_request_resolved(
        source_agent: RoleId,
        request_id: impl Into<String>,
        choice: impl Into<String>,
    ) -> Self {
        Self::ActionRequestResolved {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            request_id: request_id.into(),
            choice: choice.into(),
        }
    }

    /// Constructs a new [`ActionRequestCancelled`](SemanticEvent::ActionRequestCancelled) event.
    pub fn new_action_request_cancelled(
        source_agent: RoleId,
        request_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::ActionRequestCancelled {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            request_id: request_id.into(),
            reason: reason.into(),
        }
    }

    /// Constructs a new [`ProjectCreated`](SemanticEvent::ProjectCreated) event.
    pub fn new_project_created(
        source_agent: RoleId,
        project_id: impl Into<String>,
        host_work_dir: impl Into<String>,
    ) -> Self {
        Self::ProjectCreated {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            project_id: project_id.into(),
            host_work_dir: host_work_dir.into(),
        }
    }

    /// Constructs a new [`ProjectListed`](SemanticEvent::ProjectListed) event.
    pub fn new_project_listed(
        source_agent: RoleId,
        project_id: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self::ProjectListed {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            project_id: project_id.into(),
            path: path.into(),
        }
    }

    /// Constructs a new [`ProjectRenamed`](SemanticEvent::ProjectRenamed) event.
    pub fn new_project_renamed(
        source_agent: RoleId,
        project_id: impl Into<String>,
        old_name: impl Into<String>,
        new_name: impl Into<String>,
    ) -> Self {
        Self::ProjectRenamed {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            project_id: project_id.into(),
            old_name: old_name.into(),
            new_name: new_name.into(),
        }
    }

    /// Constructs a new [`ProjectDeleted`](SemanticEvent::ProjectDeleted) event.
    pub fn new_project_deleted(
        source_agent: RoleId,
        project_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self::ProjectDeleted {
            event_id: EventId::new(),
            source_agent,
            timestamp_ns: now_ns(),
            context: EventContext::default(),
            project_id: project_id.into(),
            name: name.into(),
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
            Self::LaneCreated { event_id, .. } => *event_id,
            Self::LaneArchived { event_id, .. } => *event_id,
            Self::LanePaused { event_id, .. } => *event_id,
            Self::ActionRequestCreated { event_id, .. } => *event_id,
            Self::ActionRequestResolved { event_id, .. } => *event_id,
            Self::ActionRequestCancelled { event_id, .. } => *event_id,
            Self::ProjectCreated { event_id, .. } => *event_id,
            Self::ProjectListed { event_id, .. } => *event_id,
            Self::ProjectRenamed { event_id, .. } => *event_id,
            Self::ProjectDeleted { event_id, .. } => *event_id,
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
            Self::LaneCreated { .. } => "LaneCreated",
            Self::LaneArchived { .. } => "LaneArchived",
            Self::LanePaused { .. } => "LanePaused",
            Self::ActionRequestCreated { .. } => "ActionRequestCreated",
            Self::ActionRequestResolved { .. } => "ActionRequestResolved",
            Self::ActionRequestCancelled { .. } => "ActionRequestCancelled",
            Self::ProjectCreated { .. } => "ProjectCreated",
            Self::ProjectListed { .. } => "ProjectListed",
            Self::ProjectRenamed { .. } => "ProjectRenamed",
            Self::ProjectDeleted { .. } => "ProjectDeleted",
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
            Self::LaneCreated { .. } => EventType::LaneCreated,
            Self::LaneArchived { .. } => EventType::LaneArchived,
            Self::LanePaused { .. } => EventType::LanePaused,
            Self::ActionRequestCreated { .. } => EventType::ActionRequestCreated,
            Self::ActionRequestResolved { .. } => EventType::ActionRequestResolved,
            Self::ActionRequestCancelled { .. } => EventType::ActionRequestCancelled,
            Self::ProjectCreated { .. } => EventType::ProjectCreated,
            Self::ProjectListed { .. } => EventType::ProjectListed,
            Self::ProjectRenamed { .. } => EventType::ProjectRenamed,
            Self::ProjectDeleted { .. } => EventType::ProjectDeleted,
        }
    }

    /// Returns the scoped context attached to this event.
    pub fn context(&self) -> &EventContext {
        match self {
            Self::ToolExecuted { context, .. }
            | Self::ClaimMade { context, .. }
            | Self::DecisionRecorded { context, .. }
            | Self::MemoryProposed { context, .. }
            | Self::MemoryAccepted { context, .. }
            | Self::MemoryRejected { context, .. }
            | Self::MemorySuperseded { context, .. }
            | Self::EvidenceChainBroken { context, .. }
            | Self::ProcessSkipped { context, .. }
            | Self::PolicyViolationDetected { context, .. }
            | Self::TaskAssigned { context, .. }
            | Self::TaskStarted { context, .. }
            | Self::TaskCompleted { context, .. }
            | Self::TaskFailed { context, .. }
            | Self::ReviewRequested { context, .. }
            | Self::ReviewCompleted { context, .. }
            | Self::EscalationRequested { context, .. }
            | Self::HumanFeedbackRequested { context, .. }
            | Self::HumanFeedbackReceived { context, .. }
            | Self::ArtefactProduced { context, .. }
            | Self::BudgetWarning { context, .. }
            | Self::EscalationAccepted { context, .. }
            | Self::RoleStateChanged { context, .. }
            | Self::OrganisationStarted { context, .. }
            | Self::OrganisationStopped { context, .. }
            | Self::Heartbeat { context, .. }
            | Self::LaneCreated { context, .. }
            | Self::LaneArchived { context, .. }
            | Self::LanePaused { context, .. }
            | Self::ActionRequestCreated { context, .. }
            | Self::ActionRequestResolved { context, .. }
            | Self::ActionRequestCancelled { context, .. }
            | Self::ProjectCreated { context, .. }
            | Self::ProjectListed { context, .. }
            | Self::ProjectRenamed { context, .. }
            | Self::ProjectDeleted { context, .. } => context,
        }
    }

    /// Returns a copy of this event with the supplied context.
    pub fn with_context(self, context: EventContext) -> Self {
        match self {
            Self::ToolExecuted {
                event_id,
                source_agent,
                timestamp_ns,
                tool_name,
                arguments,
                exit_code,
                stdout,
                stderr,
                token_usage,
                ..
            } => Self::ToolExecuted {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                tool_name,
                arguments,
                exit_code,
                stdout,
                stderr,
                token_usage,
            },
            Self::ClaimMade {
                event_id,
                source_agent,
                timestamp_ns,
                claim_text,
                evidence_refs,
                confidence_score,
                ..
            } => Self::ClaimMade {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                claim_text,
                evidence_refs,
                confidence_score,
            },
            Self::DecisionRecorded {
                event_id,
                source_agent,
                timestamp_ns,
                decision_text,
                rationale_refs,
                ..
            } => Self::DecisionRecorded {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                decision_text,
                rationale_refs,
            },
            Self::MemoryProposed {
                event_id,
                source_agent,
                timestamp_ns,
                memory_type,
                content,
                scope,
                proposed_authority,
                evidence_refs,
                confidence,
                ..
            } => Self::MemoryProposed {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                memory_type,
                content,
                scope,
                proposed_authority,
                evidence_refs,
                confidence,
            },
            Self::MemoryAccepted {
                event_id,
                source_agent,
                timestamp_ns,
                memory_id,
                proposal_event_id,
                accepted_authority,
                ..
            } => Self::MemoryAccepted {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                memory_id,
                proposal_event_id,
                accepted_authority,
            },
            Self::MemoryRejected {
                event_id,
                source_agent,
                timestamp_ns,
                proposed_memory_type,
                proposed_content,
                rejection_gate,
                rejection_reason,
                ..
            } => Self::MemoryRejected {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                proposed_memory_type,
                proposed_content,
                rejection_gate,
                rejection_reason,
            },
            Self::MemorySuperseded {
                event_id,
                source_agent,
                timestamp_ns,
                old_memory_id,
                new_memory_id,
                reason,
                ..
            } => Self::MemorySuperseded {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                old_memory_id,
                new_memory_id,
                reason,
            },
            Self::EvidenceChainBroken {
                event_id,
                source_agent,
                timestamp_ns,
                claim_id,
                broken_ref,
                claim_text,
                ..
            } => Self::EvidenceChainBroken {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                claim_id,
                broken_ref,
                claim_text,
            },
            Self::ProcessSkipped {
                event_id,
                source_agent,
                timestamp_ns,
                step,
                claim_id,
                ..
            } => Self::ProcessSkipped {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                step,
                claim_id,
            },
            Self::PolicyViolationDetected {
                event_id,
                source_agent,
                timestamp_ns,
                violation_type,
                description,
                related_event_id,
                ..
            } => Self::PolicyViolationDetected {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                violation_type,
                description,
                related_event_id,
            },
            Self::TaskAssigned {
                event_id,
                source_agent,
                timestamp_ns,
                task_id,
                worker_id,
                contract_ref,
                dependencies,
                ..
            } => Self::TaskAssigned {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                task_id,
                worker_id,
                contract_ref,
                dependencies,
            },
            Self::TaskStarted {
                event_id,
                source_agent,
                timestamp_ns,
                task_id,
                worker_id,
                ..
            } => Self::TaskStarted {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                task_id,
                worker_id,
            },
            Self::TaskCompleted {
                event_id,
                source_agent,
                timestamp_ns,
                task_id,
                contract_id,
                output_artefact,
                ..
            } => Self::TaskCompleted {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                task_id,
                contract_id,
                output_artefact,
            },
            Self::TaskFailed {
                event_id,
                source_agent,
                timestamp_ns,
                task_id,
                error_description,
                ..
            } => Self::TaskFailed {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                task_id,
                error_description,
            },
            Self::ReviewRequested {
                event_id,
                source_agent,
                timestamp_ns,
                task_id,
                reviewer_id,
                ..
            } => Self::ReviewRequested {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                task_id,
                reviewer_id,
            },
            Self::ReviewCompleted {
                event_id,
                source_agent,
                timestamp_ns,
                task_id,
                findings,
                accepted,
                ..
            } => Self::ReviewCompleted {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                task_id,
                findings,
                accepted,
            },
            Self::EscalationRequested {
                event_id,
                source_agent,
                timestamp_ns,
                from_role,
                to_role,
                reason,
                severity,
                ..
            } => Self::EscalationRequested {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                from_role,
                to_role,
                reason,
                severity,
            },
            Self::HumanFeedbackRequested {
                event_id,
                source_agent,
                timestamp_ns,
                question,
                request_context,
                ..
            } => Self::HumanFeedbackRequested {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                question,
                request_context,
            },
            Self::HumanFeedbackReceived {
                event_id,
                source_agent,
                timestamp_ns,
                answer,
                ..
            } => Self::HumanFeedbackReceived {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                answer,
            },
            Self::ArtefactProduced {
                event_id,
                source_agent,
                timestamp_ns,
                artefact_id,
                artefact_type,
                storage_kind,
                storage_uri,
                content_hash,
                evidence_refs,
                repository_output,
                producer_role,
                ..
            } => Self::ArtefactProduced {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                artefact_id,
                artefact_type,
                storage_kind,
                storage_uri,
                content_hash,
                evidence_refs,
                repository_output,
                producer_role,
            },
            Self::BudgetWarning {
                event_id,
                source_agent,
                timestamp_ns,
                contract_id,
                message,
                usage_percent,
                ..
            } => Self::BudgetWarning {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                contract_id,
                message,
                usage_percent,
            },
            Self::EscalationAccepted {
                event_id,
                source_agent,
                timestamp_ns,
                escalation_event_id,
                target_role,
                chain_depth,
                ..
            } => Self::EscalationAccepted {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                escalation_event_id,
                target_role,
                chain_depth,
            },
            Self::RoleStateChanged {
                event_id,
                source_agent,
                timestamp_ns,
                role_id,
                old_state,
                new_state,
                ..
            } => Self::RoleStateChanged {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                role_id,
                old_state,
                new_state,
            },
            Self::OrganisationStarted {
                event_id,
                source_agent,
                timestamp_ns,
                ..
            } => Self::OrganisationStarted {
                event_id,
                source_agent,
                timestamp_ns,
                context,
            },
            Self::OrganisationStopped {
                event_id,
                source_agent,
                timestamp_ns,
                reason,
                ..
            } => Self::OrganisationStopped {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                reason,
            },
            Self::Heartbeat {
                event_id,
                source_agent,
                timestamp_ns,
                active_roles,
                completed_roles,
                failed_roles,
                ..
            } => Self::Heartbeat {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                active_roles,
                completed_roles,
                failed_roles,
            },
            Self::LaneCreated {
                event_id,
                source_agent,
                timestamp_ns,
                lane_id,
                name,
                kind,
                colour,
                purpose,
                parent_lane_id,
                related_lane_ids,
                source_message_id,
                ..
            } => Self::LaneCreated {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                lane_id,
                name,
                kind,
                colour,
                purpose,
                parent_lane_id,
                related_lane_ids,
                source_message_id,
            },
            Self::LaneArchived {
                event_id,
                source_agent,
                timestamp_ns,
                lane_id,
                ..
            } => Self::LaneArchived {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                lane_id,
            },
            Self::LanePaused {
                event_id,
                source_agent,
                timestamp_ns,
                lane_id,
                ..
            } => Self::LanePaused {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                lane_id,
            },
            Self::ActionRequestCreated {
                event_id,
                source_agent,
                timestamp_ns,
                request_id,
                request_kind,
                prompt,
                choices,
                lane_id,
                ..
            } => Self::ActionRequestCreated {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                request_id,
                request_kind,
                prompt,
                choices,
                lane_id,
            },
            Self::ActionRequestResolved {
                event_id,
                source_agent,
                timestamp_ns,
                request_id,
                choice,
                ..
            } => Self::ActionRequestResolved {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                request_id,
                choice,
            },
            Self::ActionRequestCancelled {
                event_id,
                source_agent,
                timestamp_ns,
                request_id,
                reason,
                ..
            } => Self::ActionRequestCancelled {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                request_id,
                reason,
            },
            Self::ProjectCreated {
                event_id,
                source_agent,
                timestamp_ns,
                project_id,
                host_work_dir,
                ..
            } => Self::ProjectCreated {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                project_id,
                host_work_dir,
            },
            Self::ProjectListed {
                event_id,
                source_agent,
                timestamp_ns,
                project_id,
                path,
                ..
            } => Self::ProjectListed {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                project_id,
                path,
            },
            Self::ProjectRenamed {
                event_id,
                source_agent,
                timestamp_ns,
                project_id,
                old_name,
                new_name,
                ..
            } => Self::ProjectRenamed {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                project_id,
                old_name,
                new_name,
            },
            Self::ProjectDeleted {
                event_id,
                source_agent,
                timestamp_ns,
                project_id,
                name,
                ..
            } => Self::ProjectDeleted {
                event_id,
                source_agent,
                timestamp_ns,
                context,
                project_id,
                name,
            },
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
            Self::LaneCreated => "LaneCreated",
            Self::LaneArchived => "LaneArchived",
            Self::LanePaused => "LanePaused",
            Self::ActionRequestCreated => "ActionRequestCreated",
            Self::ActionRequestResolved => "ActionRequestResolved",
            Self::ActionRequestCancelled => "ActionRequestCancelled",
            Self::ProjectCreated => "ProjectCreated",
            Self::ProjectListed => "ProjectListed",
            Self::ProjectRenamed => "ProjectRenamed",
            Self::ProjectDeleted => "ProjectDeleted",
        }
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Returns the current system time as nanoseconds since the UNIX epoch.
pub fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

/// Returns a stable non-cryptographic hash string for lightweight content identity.
pub fn stable_content_hash(content: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
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
            EventType::LaneCreated.name(),
            EventType::LaneArchived.name(),
            EventType::LanePaused.name(),
            EventType::ActionRequestCreated.name(),
            EventType::ActionRequestResolved.name(),
            EventType::ActionRequestCancelled.name(),
            EventType::ProjectCreated.name(),
            EventType::ProjectListed.name(),
            EventType::ProjectRenamed.name(),
            EventType::ProjectDeleted.name(),
        ];
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(names.len(), unique.len());
    }

    #[test]
    fn project_created_round_trip_serialisation() {
        let event =
            SemanticEvent::new_project_created(RoleId::new("human"), "my-app", "/workspace");
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("ProjectCreated"));
        let back: SemanticEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event.event_id(), back.event_id());
        assert_eq!(event.event_type(), EventType::ProjectCreated);
        match &back {
            SemanticEvent::ProjectCreated {
                project_id,
                host_work_dir,
                ..
            } => {
                assert_eq!(project_id, "my-app");
                assert_eq!(host_work_dir, "/workspace");
            }
            other => panic!("expected ProjectCreated, got {}", other.variant_name()),
        }
    }
}
