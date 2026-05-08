//! Core role types, lifecycle states, coordination primitives, and error types.

use std::{any::Any, collections::HashMap, fmt, sync::Arc};

use async_trait::async_trait;
use mmat_event_stream::{
    event::{EscalationSeverity, EventType, RoleId},
    event_bus::{EventBus, EventReceiver},
};
use mmat_memory::store::MemoryStore;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Trait implemented by all role types.
///
/// Defines identity, specification, event subscriptions, and the main run loop.
#[async_trait]
pub trait Role: Send + Sync + 'static {
    /// Returns the unique ID of this role.
    fn id(&self) -> RoleId;
    /// Returns the specification that defines this role's capabilities and constraints.
    fn spec(&self) -> RoleSpec;
    /// Returns the set of event types this role subscribes to.
    fn subscriptions(&self) -> &'static [EventType];

    /// Runs the role's main event loop with the given context.
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

/// The type of role within an organisation, determining its responsibilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoleType {
    /// Defines intent and prioritises work.
    IntentLead,
    /// Researches and learns from external sources.
    Scholar,
    /// Manages operational procedures and incident response.
    OpsManager,
    /// Designs system architecture and makes cross-cutting decisions.
    Architect,
    /// Plans work and assigns tasks.
    ProjectManager,
    /// Executes implementation tasks.
    Worker,
    /// Reviews completed work for quality and compliance.
    Reviewer,
    /// Audits processes for policy adherence.
    Auditor,
    /// Curates and organises shared knowledge.
    Librarian,
}

/// Severity level for events, escalations, and alerts.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Minimal impact, routine matter.
    Low,
    /// Moderate impact, requires attention.
    Medium,
    /// Significant impact, needs prompt action.
    High,
    /// Severe impact, requires immediate escalation.
    Critical,
}

/// Defines the scope of authority a role has to publish events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityScope {
    /// Only may publish intent-related events.
    IntentOnly,
    /// May publish architecture and design decisions.
    Architecture,
    /// May publish planning and task assignment events.
    Planning,
    /// May publish implementation and tool execution events.
    Implementation,
    /// May publish review-related events.
    Review,
    /// May publish audit and policy violation events.
    Audit,
    /// May publish any event type.
    FullAccess,
}

/// Resource budget constraining a role's execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// Maximum wall-clock time in seconds.
    pub time_limit_seconds: u64,
    /// Maximum number of tokens (e.g. LLM tokens) allowed.
    pub token_limit: u64,
    /// Maximum number of retry attempts on failure.
    pub max_retries: u32,
}

/// Specification defining a role's identity, capabilities, constraints, and routing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSpec {
    /// Unique identifier for the role.
    pub id: RoleId,
    /// The type of role.
    pub role_type: RoleType,
    /// The authority scope granted to this role.
    pub authority_scope: AuthorityScope,
    /// Default resource budget for this role.
    pub default_budget: Budget,
    /// Mapping of severity levels to escalation target role IDs.
    pub escalation_paths: HashMap<Severity, RoleId>,
    /// The event type that triggers this role.
    pub input_contract: EventType,
    /// The event types this role is permitted to publish.
    pub output_contract: Vec<EventType>,
}

/// The lifecycle state of a role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoleLifecycleState {
    /// The role is waiting for work.
    Idle,
    /// The role is actively executing a task.
    Running,
    /// The role has completed its task successfully.
    Completed,
    /// The role has failed its task.
    Failed,
    /// The role has escalated to a higher authority.
    Escalated,
}

/// Messages sent from roles to the coordinator over an mpsc channel.
#[derive(Clone, Debug)]
pub enum CoordinatorMessage {
    /// A role reports its current lifecycle state.
    ReportStatus {
        role_id: RoleId,
        state: RoleLifecycleState,
    },
    /// A role requests escalation to a higher authority.
    RequestEscalation {
        from: RoleId,
        severity: Severity,
        reason: String,
    },
}

/// Handle that roles use to communicate with the coordinator.
#[derive(Clone, Debug)]
pub struct CoordinatorHandle {
    pub(crate) tx: mpsc::Sender<CoordinatorMessage>,
}

/// Registry of tools available to a role, keyed by name.
#[derive(Clone, Debug)]
pub struct ToolRegistry<T> {
    tools: HashMap<String, T>,
}

/// Execution context provided to a role when it starts running.
pub struct RoleContext {
    pub bus: EventBus,
    pub receiver: EventReceiver,
    pub memory_store: Arc<MemoryStore>,
    pub coordinator: CoordinatorHandle,
    pub tools: Box<dyn Any + Send + Sync>,
}

/// Errors that a role may return from its run loop.
#[derive(Clone, Debug, PartialEq)]
pub enum RoleError {
    /// A generic internal error.
    Internal(String),
    /// The role's time or token budget was exceeded.
    BudgetExceeded(String),
    /// The role violated its contract.
    ContractViolation(String),
    /// Escalation to a higher-authority role is required.
    EscalationRequired(String),
}

pub(crate) struct RoleHandle<T: Role>(Arc<T>);

/// Formats the severity as a human-readable string.
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

/// Converts an [`EscalationSeverity`] into a [`Severity`].
impl From<EscalationSeverity> for Severity {
    fn from(s: EscalationSeverity) -> Self {
        match s {
            EscalationSeverity::Low => Self::Low,
            EscalationSeverity::Medium => Self::Medium,
            EscalationSeverity::High => Self::High,
            EscalationSeverity::Critical => Self::Critical,
        }
    }
}

impl AuthorityScope {
    /// Checks whether this authority scope permits publishing the given event type.
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

/// Returns a [`Budget`] with defaults: 300s time limit, 100k tokens, 3 retries.
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
    /// Checks whether a transition from the current state to `next` is valid.
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

/// Formats the lifecycle state as a human-readable string.
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
    /// Creates a new coordinator handle backed by the given sender.
    pub fn new(tx: mpsc::Sender<CoordinatorMessage>) -> Self {
        Self { tx }
    }

    /// Sends a status report to the coordinator.
    pub async fn report_status(
        &self,
        role_id: RoleId,
        state: RoleLifecycleState,
    ) -> std::result::Result<(), mpsc::error::SendError<CoordinatorMessage>> {
        self.tx
            .send(CoordinatorMessage::ReportStatus { role_id, state })
            .await
    }

    /// Sends an escalation request to the coordinator.
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
    /// Creates an empty tool registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a tool under the given name.
    pub fn register(&mut self, name: impl Into<String>, tool: T) {
        self.tools.insert(name.into(), tool);
    }

    /// Looks up a tool by its registered name.
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
    /// Downcasts the tools container to a [`ToolRegistry<T>`].
    pub fn tools<T: 'static>(&self) -> Option<&ToolRegistry<T>> {
        self.tools.downcast_ref::<ToolRegistry<T>>()
    }

    /// Returns a new context with the given tool registry attached.
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

/// Formats the role error with a descriptive message.
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

/// Marks [`RoleError`] as implementing the standard error trait.
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

impl From<Severity> for EscalationSeverity {
    fn from(s: Severity) -> Self {
        match s {
            Severity::Low => Self::Low,
            Severity::Medium => Self::Medium,
            Severity::High => Self::High,
            Severity::Critical => Self::Critical,
        }
    }
}
