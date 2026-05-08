//! Contract types defining task agreements and completion criteria between roles.

use std::time::Duration;

use mmat_event_stream::event::{EventId, SemanticEvent};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::retrieval::RetrievalProfile;
use crate::role::AuthorityScope;

/// Unique identifier for a contract, backed by a UUID v4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContractId(pub Uuid);

/// Criteria that determine when a contract is considered complete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionCriteria {
    /// All checks (reviews, tests, etc.) have passed.
    AllChecksPassed,
    /// An artefact has been produced.
    ArtefactProduced,
    /// A human has approved the work.
    HumanApproved,
    /// The contract times out after a given duration.
    Timeout(Duration),
}

/// A task contract between a delegator and a worker role.
///
/// Generic parameters `I` and `O` represent the input and output types.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract<I, O> {
    /// Unique identifier for this contract.
    pub contract_id: ContractId,
    /// Schema describing the expected input.
    pub input_schema: String,
    /// Schema describing the expected output.
    pub output_schema: String,
    /// The authority scope granted to the worker.
    pub authority_scope: AuthorityScope,
    /// Criteria for declaring the contract complete.
    pub completion_criteria: CompletionCriteria,
    /// Maximum number of retry attempts on failure.
    pub max_retries: u32,
    /// Optional override for the memory retrieval profile.
    pub retrieval_override: Option<RetrievalProfile>,
    _phantom: std::marker::PhantomData<(I, O)>,
}

/// Contextual state for an executing task, tracking associated events.
#[derive(Clone, Debug)]
pub struct TaskContext {
    /// The contract this task is operating under.
    pub contract_id: ContractId,
    /// The event that triggered this task.
    pub source_task_event: EventId,
    /// Events accumulated during task execution.
    pub events: Vec<SemanticEvent>,
}

impl ContractId {
    /// Creates a new contract ID with a random UUID v4.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ContractId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ContractId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<I, O> Contract<I, O> {
    /// Creates a new contract with default parameters.
    ///
    /// Defaults to `AuthorityScope::Implementation`, `AllChecksPassed` completion,
    /// and a maximum of 3 retries.
    pub fn new(input_schema: impl Into<String>, output_schema: impl Into<String>) -> Self {
        Self {
            contract_id: ContractId::new(),
            input_schema: input_schema.into(),
            output_schema: output_schema.into(),
            authority_scope: AuthorityScope::Implementation,
            completion_criteria: CompletionCriteria::AllChecksPassed,
            max_retries: 3,
            retrieval_override: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Sets the authority scope for the contract.
    pub fn with_authority_scope(mut self, scope: AuthorityScope) -> Self {
        self.authority_scope = scope;
        self
    }

    /// Sets the completion criteria for the contract.
    pub fn with_completion_criteria(mut self, criteria: CompletionCriteria) -> Self {
        self.completion_criteria = criteria;
        self
    }

    /// Sets the maximum number of retry attempts.
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Overrides the default retrieval profile for this contract.
    pub fn with_retrieval_override(mut self, profile: RetrievalProfile) -> Self {
        self.retrieval_override = Some(profile);
        self
    }

    /// Validates that the contract's schemas are non-empty.
    pub fn validate(&self) -> Result<()> {
        if self.input_schema.is_empty() {
            return Err(Error::InvalidRoleSpec(
                "input_schema must not be empty".into(),
            ));
        }
        if self.output_schema.is_empty() {
            return Err(Error::InvalidRoleSpec(
                "output_schema must not be empty".into(),
            ));
        }
        Ok(())
    }

    /// Checks whether the contract's completion criteria are satisfied
    /// by the given set of events.
    pub fn is_satisfied(&self, events: &[SemanticEvent]) -> bool {
        match &self.completion_criteria {
            CompletionCriteria::AllChecksPassed => events
                .iter()
                .any(|e| matches!(e, SemanticEvent::ReviewCompleted { accepted: true, .. })),
            CompletionCriteria::ArtefactProduced => events
                .iter()
                .any(|e| matches!(e, SemanticEvent::ArtefactProduced { .. })),
            CompletionCriteria::HumanApproved => events
                .iter()
                .any(|e| matches!(e, SemanticEvent::HumanFeedbackReceived { .. })),
            CompletionCriteria::Timeout(_duration) => {
                // Timeout is checked externally by the scheduler against wall-clock time
                false
            }
        }
    }
}

impl TaskContext {
    /// Creates a new task context for the given contract and trigger event.
    pub fn new(contract_id: ContractId, source_task_event: EventId) -> Self {
        Self {
            contract_id,
            source_task_event,
            events: Vec::new(),
        }
    }

    /// Appends an event to the task's event history.
    pub fn push_event(&mut self, event: SemanticEvent) {
        self.events.push(event);
    }
}
