use std::fmt;

use tokio::sync::{mpsc, oneshot};

use crate::project::ProjectId;

pub type EventReceiver = mpsc::UnboundedReceiver<FrontendEvent>;
pub type EventSender = mpsc::UnboundedSender<FrontendEvent>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSummaryEvent {
    pub project_id: ProjectId,
    pub run_id: String,
    pub project_root: String,
    pub run_root: String,
    pub prompt: String,
    pub status: String,
    pub current_stage: String,
    pub next_step: Option<String>,
}

#[derive(Debug)]
pub enum FrontendEvent {
    ProjectScoped {
        project_id: ProjectId,
        event: Box<FrontendEvent>,
    },
    StepStarted {
        task_label: String,
    },
    StepAttemptStarted {
        task_label: String,
        attempt: usize,
    },
    StepAttemptValidated {
        task_label: String,
        attempt: usize,
        accepted: bool,
        finding_count: usize,
    },
    StepRepairStarted {
        task_label: String,
        attempt: usize,
    },
    StepCompleted {
        task_label: String,
        attempts: usize,
    },
    StepRejected {
        task_label: String,
        attempts: usize,
        reason: String,
    },
    StepFailed {
        task_label: String,
        stage: String,
    },
    ComponentStarted {
        component: String,
        name: String,
    },
    ComponentCompleted {
        component: String,
        name: String,
    },
    ComponentFailed {
        component: String,
        name: String,
    },
    Log {
        level: tracing::Level,
        target: String,
        message: String,
    },
    AssistantReasoningDelta {
        delta: String,
    },
    AssistantMessageDelta {
        delta: String,
    },
    AssistantResponseCompleted {
        message: Option<String>,
    },
    ToolCallStarted {
        name: String,
        arguments: String,
    },
    HumanPrompt {
        question: String,
        choices: Vec<String>,
        reply: oneshot::Sender<String>,
    },
    RunSummary(RunSummaryEvent),
    DomainTreeUpdated,
    DomainNodePhaseChanged {
        node_id: String,
        phase: String,
    },
    BackflowStarted {
        node_id: String,
        severity: String,
        reason: String,
    },
    BackflowCascade {
        source_node_id: String,
        affected_node_ids: Vec<String>,
    },
    BackflowResolved {
        node_id: String,
    },
    BackflowHalting {
        source_node_id: String,
        reason: String,
    },
    DeliveryGraphUpdated,
    DeliveryBatchStarted {
        batch_index: usize,
    },
    DeliveryBatchCompleted {
        batch_index: usize,
    },
    Quit,
}

impl fmt::Display for FrontendEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProjectScoped { project_id, event } => {
                write!(f, "[project {project_id}] {event}")
            }
            Self::StepStarted { task_label } => write!(f, "step started: {task_label}"),
            Self::StepAttemptStarted {
                task_label,
                attempt,
            } => write!(f, "attempt {attempt} started: {task_label}"),
            Self::StepAttemptValidated {
                task_label,
                attempt,
                accepted,
                finding_count,
            } => write!(
                f,
                "attempt {attempt} validated: {task_label} (accepted={accepted}, findings={finding_count})"
            ),
            Self::StepRepairStarted {
                task_label,
                attempt,
            } => write!(f, "repair started: {task_label} (attempt {attempt})"),
            Self::StepCompleted {
                task_label,
                attempts,
            } => write!(f, "step completed: {task_label} ({attempts} attempts)"),
            Self::StepRejected {
                task_label,
                attempts,
                reason,
            } => write!(
                f,
                "step rejected: {task_label} ({attempts} attempts, {reason})"
            ),
            Self::StepFailed { task_label, stage } => {
                write!(f, "step failed: {task_label} ({stage})")
            }
            Self::ComponentStarted { component, name } => write!(f, "{component} started: {name}"),
            Self::ComponentCompleted { component, name } => {
                write!(f, "{component} completed: {name}")
            }
            Self::ComponentFailed { component, name } => write!(f, "{component} failed: {name}"),
            Self::Log { level, message, .. } => write!(f, "[{level}] {message}"),
            Self::AssistantReasoningDelta { .. } => write!(f, "assistant reasoning delta"),
            Self::AssistantMessageDelta { .. } => write!(f, "assistant message delta"),
            Self::AssistantResponseCompleted { .. } => write!(f, "assistant response completed"),
            Self::ToolCallStarted { name, .. } => write!(f, "tool call started: {name}"),
            Self::HumanPrompt { question, .. } => write!(f, "? {question}"),
            Self::RunSummary(summary) => write!(
                f,
                "run summary: {} ({})",
                summary.status, summary.current_stage
            ),
            Self::DomainTreeUpdated => write!(f, "domain tree updated"),
            Self::DomainNodePhaseChanged { node_id, phase } => {
                write!(f, "domain node {node_id} phase changed to {phase}")
            }
            Self::BackflowStarted {
                node_id,
                severity,
                reason,
            } => write!(f, "backflow started on {node_id} ({severity}): {reason}"),
            Self::BackflowCascade {
                source_node_id,
                affected_node_ids,
            } => write!(
                f,
                "backflow cascade from {source_node_id} affects {affected_node_ids:?}"
            ),
            Self::BackflowResolved { node_id } => write!(f, "backflow resolved on {node_id}"),
            Self::BackflowHalting {
                source_node_id,
                reason,
            } => write!(f, "backflow halting on {source_node_id}: {reason}"),
            Self::DeliveryGraphUpdated => write!(f, "delivery graph updated"),
            Self::DeliveryBatchStarted { batch_index } => {
                write!(f, "delivery batch {batch_index} started")
            }
            Self::DeliveryBatchCompleted { batch_index } => {
                write!(f, "delivery batch {batch_index} completed")
            }
            Self::Quit => write!(f, "quit"),
        }
    }
}
