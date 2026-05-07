use std::time::Duration;

use event_stream::event::{EventId, SemanticEvent};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::retrieval::RetrievalProfile;
use crate::role::AuthorityScope;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContractId(pub Uuid);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionCriteria {
    AllChecksPassed,
    ArtefactProduced,
    HumanApproved,
    Timeout(Duration),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract<I, O> {
    pub contract_id: ContractId,
    pub input_schema: String,
    pub output_schema: String,
    pub authority_scope: AuthorityScope,
    pub completion_criteria: CompletionCriteria,
    pub max_retries: u32,
    pub retrieval_override: Option<RetrievalProfile>,
    _phantom: std::marker::PhantomData<(I, O)>,
}

#[derive(Clone, Debug)]
pub struct TaskContext {
    pub contract_id: ContractId,
    pub source_task_event: EventId,
    pub events: Vec<SemanticEvent>,
}

impl ContractId {
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

    pub fn with_authority_scope(mut self, scope: AuthorityScope) -> Self {
        self.authority_scope = scope;
        self
    }

    pub fn with_completion_criteria(mut self, criteria: CompletionCriteria) -> Self {
        self.completion_criteria = criteria;
        self
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_retrieval_override(mut self, profile: RetrievalProfile) -> Self {
        self.retrieval_override = Some(profile);
        self
    }

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
    pub fn new(contract_id: ContractId, source_task_event: EventId) -> Self {
        Self {
            contract_id,
            source_task_event,
            events: Vec::new(),
        }
    }

    pub fn push_event(&mut self, event: SemanticEvent) {
        self.events.push(event);
    }
}
