use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use event_stream::event::{EventType, RoleId};
use event_stream::event_bus::{EventBus, EventReceiver};
use memory::store::MemoryStore;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[async_trait]
pub trait Role: Send + Sync + 'static {
    fn id(&self) -> RoleId;
    fn spec(&self) -> RoleSpec;
    fn subscriptions(&self) -> &'static [EventType];

    async fn run(self: Arc<Self>, ctx: RoleContext) -> std::result::Result<(), RoleError>;
}

#[async_trait]
pub(crate) trait SpawnableRole: Send + Sync + 'static {
    fn id(&self) -> RoleId;
    #[allow(dead_code)]
    fn spec(&self) -> RoleSpec;
    fn subscriptions(&self) -> &'static [EventType];
    async fn run(&self, ctx: RoleContext) -> std::result::Result<(), RoleError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoleType {
    IntentLead,
    Scholar,
    OpsManager,
    Architect,
    ProjectManager,
    Worker,
    Reviewer,
    Auditor,
    Librarian,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityScope {
    IntentOnly,
    Architecture,
    Planning,
    Implementation,
    Review,
    Audit,
    FullAccess,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub time_limit_seconds: u64,
    pub token_limit: u64,
    pub max_retries: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSpec {
    pub id: RoleId,
    pub role_type: RoleType,
    pub authority_scope: AuthorityScope,
    pub default_budget: Budget,
    pub escalation_paths: HashMap<Severity, RoleId>,
    pub input_contract: EventType,
    pub output_contract: Vec<EventType>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoleLifecycleState {
    Idle,
    Running,
    Completed,
    Failed,
    Escalated,
}

#[derive(Clone, Debug)]
pub enum CoordinatorMessage {
    ReportStatus {
        role_id: RoleId,
        state: RoleLifecycleState,
    },
    RequestEscalation {
        from: RoleId,
        severity: Severity,
        reason: String,
    },
}

#[derive(Clone, Debug)]
pub struct CoordinatorHandle {
    pub(crate) tx: mpsc::Sender<CoordinatorMessage>,
}

#[derive(Clone, Debug)]
pub struct ToolRegistry<T> {
    tools: HashMap<String, T>,
}

pub struct RoleContext {
    pub bus: EventBus,
    pub receiver: EventReceiver,
    pub memory_store: Arc<MemoryStore>,
    pub coordinator: CoordinatorHandle,
    pub tools: Box<dyn Any + Send + Sync>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RoleError {
    Internal(String),
    BudgetExceeded(String),
    ContractViolation(String),
    EscalationRequired(String),
}

pub(crate) struct RoleHandle<T: Role>(Arc<T>);

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "Low"),
            Self::Medium => write!(f, "Medium"),
            Self::High => write!(f, "High"),
            Self::Critical => write!(f, "Critical"),
        }
    }
}

impl From<event_stream::event::EscalationSeverity> for Severity {
    fn from(s: event_stream::event::EscalationSeverity) -> Self {
        match s {
            event_stream::event::EscalationSeverity::Low => Self::Low,
            event_stream::event::EscalationSeverity::Medium => Self::Medium,
            event_stream::event::EscalationSeverity::High => Self::High,
            event_stream::event::EscalationSeverity::Critical => Self::Critical,
        }
    }
}

impl AuthorityScope {
    pub fn can_publish(&self, event_type: &EventType) -> bool {
        if matches!(
            event_type,
            EventType::TaskStarted
                | EventType::TaskCompleted
                | EventType::TaskFailed
                | EventType::EscalationRequested
                | EventType::ClaimMade
        ) {
            return true;
        }

        match self {
            Self::FullAccess => true,
            Self::IntentOnly => matches!(
                event_type,
                EventType::HumanFeedbackRequested
                    | EventType::HumanFeedbackReceived
                    | EventType::ArtefactProduced
                    | EventType::MemoryProposed
                    | EventType::TaskAssigned
            ),
            Self::Architecture => matches!(
                event_type,
                EventType::DecisionRecorded
                    | EventType::ArtefactProduced
                    | EventType::MemoryProposed
            ),
            Self::Planning => matches!(
                event_type,
                EventType::TaskAssigned | EventType::ArtefactProduced | EventType::MemoryProposed
            ),
            Self::Implementation => matches!(
                event_type,
                EventType::ToolExecuted | EventType::ArtefactProduced | EventType::MemoryProposed
            ),
            Self::Review => matches!(
                event_type,
                EventType::ReviewRequested | EventType::ReviewCompleted
            ),
            Self::Audit => matches!(
                event_type,
                EventType::PolicyViolationDetected
                    | EventType::EvidenceChainBroken
                    | EventType::ProcessSkipped
                    | EventType::ArtefactProduced
            ),
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            time_limit_seconds: 300,
            token_limit: 100_000,
            max_retries: 3,
        }
    }
}

impl RoleLifecycleState {
    pub fn can_transition_to(&self, next: &Self) -> bool {
        match (self, next) {
            (Self::Idle, Self::Running) => true,
            (Self::Running, Self::Completed) => true,
            (Self::Running, Self::Failed) => true,
            (Self::Running, Self::Escalated) => true,
            (Self::Failed, Self::Running) => true,
            (Self::Failed, Self::Escalated) => true,
            (Self::Escalated, Self::Running) => true,
            (Self::Completed, Self::Idle) => true,
            (Self::Completed, Self::Running) => true,
            (a, b) if a == b => true,
            _ => false,
        }
    }
}

impl fmt::Display for RoleLifecycleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Running => write!(f, "Running"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed => write!(f, "Failed"),
            Self::Escalated => write!(f, "Escalated"),
        }
    }
}

impl CoordinatorHandle {
    pub fn new(tx: mpsc::Sender<CoordinatorMessage>) -> Self {
        Self { tx }
    }

    pub async fn report_status(
        &self,
        role_id: RoleId,
        state: RoleLifecycleState,
    ) -> std::result::Result<(), mpsc::error::SendError<CoordinatorMessage>> {
        self.tx
            .send(CoordinatorMessage::ReportStatus { role_id, state })
            .await
    }

    pub async fn request_escalation(
        &self,
        from: RoleId,
        severity: Severity,
        reason: impl Into<String>,
    ) -> std::result::Result<(), mpsc::error::SendError<CoordinatorMessage>> {
        self.tx
            .send(CoordinatorMessage::RequestEscalation {
                from,
                severity,
                reason: reason.into(),
            })
            .await
    }
}

impl<T> ToolRegistry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, tool: T) {
        self.tools.insert(name.into(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&T> {
        self.tools.get(name)
    }
}

impl<T> Default for ToolRegistry<T> {
    fn default() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }
}

impl RoleContext {
    pub fn tools<T: 'static>(&self) -> Option<&ToolRegistry<T>> {
        self.tools.downcast_ref::<ToolRegistry<T>>()
    }

    pub fn with_tools<T: 'static + Send + Sync>(mut self, tools: ToolRegistry<T>) -> Self {
        self.tools = Box::new(tools);
        self
    }
}

impl std::fmt::Debug for RoleContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoleContext")
            .field("bus", &self.bus)
            .field("coordinator", &self.coordinator)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for RoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal(msg) => write!(f, "Internal error: {msg}"),
            Self::BudgetExceeded(msg) => write!(f, "Budget exceeded: {msg}"),
            Self::ContractViolation(msg) => write!(f, "Contract violation: {msg}"),
            Self::EscalationRequired(msg) => write!(f, "Escalation required: {msg}"),
        }
    }
}

impl std::error::Error for RoleError {}

impl<T: Role> RoleHandle<T> {
    pub fn new(role: T) -> Self {
        Self(Arc::new(role))
    }
}

#[async_trait]
impl<T: Role> SpawnableRole for RoleHandle<T> {
    fn id(&self) -> RoleId {
        Role::id(self.0.as_ref())
    }

    fn spec(&self) -> RoleSpec {
        Role::spec(self.0.as_ref())
    }

    fn subscriptions(&self) -> &'static [EventType] {
        Role::subscriptions(self.0.as_ref())
    }

    async fn run(&self, ctx: RoleContext) -> std::result::Result<(), RoleError> {
        Role::run(Arc::clone(&self.0), ctx).await
    }
}

impl From<Severity> for event_stream::event::EscalationSeverity {
    fn from(s: Severity) -> Self {
        match s {
            Severity::Low => Self::Low,
            Severity::Medium => Self::Medium,
            Severity::High => Self::High,
            Severity::Critical => Self::Critical,
        }
    }
}
