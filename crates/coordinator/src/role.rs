//! Core role types, lifecycle states, coordination primitives, and error types.

use std::{any::Any, collections::HashMap, fmt, sync::Arc};

use async_trait::async_trait;

use mmat_event_stream::{
    event::{EscalationSeverity, EventType, RoleId, SemanticEvent, StoredArtefactRef},
    event_bus::{EventBus, EventReceiver},
};
use mmat_memory::artefact_store::ArtefactStore;
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

    /// Returns the capability readiness status for this role.
    /// Default returns [`RoleReadiness::default`] (fallback, no capability info).
    fn role_readiness(&self) -> RoleReadiness {
        RoleReadiness::default()
    }
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

/// The capability status of a role indicating whether required providers,
/// tools, and infrastructure are available.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CapabilityStatus {
    /// All required providers and tools are configured and available.
    Configured,
    /// Some providers or tools are missing but the role can still function with limitations.
    Degraded,
    /// No external providers are available; the role uses deterministic fallback behaviour.
    Fallback,
    /// The role cannot function at all without the missing configuration.
    Unavailable,
}

impl std::fmt::Display for CapabilityStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configured => write!(f, "configured"),
            Self::Degraded => write!(f, "degraded"),
            Self::Fallback => write!(f, "fallback"),
            Self::Unavailable => write!(f, "unavailable"),
        }
    }
}

/// Readiness information for a role indicating which capabilities are present
/// and which are missing or degraded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleReadiness {
    /// Overall capability status derived from provider and tool checks.
    pub capability: CapabilityStatus,
    /// Whether an LLM client is configured.
    pub has_llm_client: bool,
    /// Whether the role's tool registry has at least one registered tool.
    pub has_tools: bool,
    /// Count of registered tools for this role.
    pub tool_count: u32,
    /// Whether the role is running with fallback worktree support.
    pub fallback_worktree: bool,
    /// Whether the role requires an LLM client to be useful.
    pub requires_llm: bool,
    /// Whether the artefact store is configured for persistence.
    pub has_artefact_store: bool,
    /// Human-readable description of the readiness state.
    pub summary: String,
}

impl Default for RoleReadiness {
    fn default() -> Self {
        Self {
            capability: CapabilityStatus::Fallback,
            has_llm_client: false,
            has_tools: false,
            tool_count: 0,
            fallback_worktree: false,
            requires_llm: false,
            has_artefact_store: false,
            summary: "No capability information available".to_string(),
        }
    }
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
    pub artefact_store: Option<Arc<ArtefactStore>>,
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

    /// Stores an artefact payload, returning a [`StoredArtefactRef`].
    ///
    /// Requires a configured artefact store.
    pub async fn store_artefact(
        &self,
        artefact_type: &str,
        payload: &str,
    ) -> std::result::Result<StoredArtefactRef, RoleError> {
        match &self.artefact_store {
            Some(store) => store
                .store(artefact_type, payload)
                .await
                .map_err(|e| RoleError::Internal(e.to_string())),
            None => Err(RoleError::Internal(
                "artefact store not configured".to_string(),
            )),
        }
    }

    /// Stores an artefact and publishes the corresponding [`ArtefactProduced`](SemanticEvent::ArtefactProduced) event.
    ///
    /// Postgres-backed stores persist the artefact and event row in one transaction before broadcasting.
    pub async fn store_and_publish_artefact(
        &self,
        artefact_type: &str,
        payload: &str,
        source_agent: RoleId,
        producer_role: RoleId,
    ) -> std::result::Result<StoredArtefactRef, RoleError> {
        match &self.artefact_store {
            Some(store) => store
                .store_and_publish_event(
                    artefact_type,
                    payload,
                    source_agent.0.as_str(),
                    producer_role.0.as_str(),
                    &self.bus,
                )
                .await
                .map_err(|e| RoleError::Internal(e.to_string())),
            None => {
                let stored = self.store_artefact(artefact_type, payload).await?;
                let event = SemanticEvent::new_artefact_produced_ref(
                    source_agent,
                    stored.artefact_id.clone(),
                    artefact_type,
                    stored.content_hash.clone(),
                    stored.storage_uri.clone(),
                    producer_role,
                    vec![],
                );
                self.bus
                    .publish(event)
                    .map_err(|e| RoleError::Internal(e.to_string()))?;
                Ok(stored)
            }
        }
    }

    /// Retrieves an artefact payload by its storage URI.
    ///
    /// Supports `db://` and legacy inline `type|payload` URIs.
    pub async fn get_artefact_payload(
        &self,
        storage_uri: &str,
    ) -> std::result::Result<Option<String>, RoleError> {
        match &self.artefact_store {
            Some(store) => store
                .get_payload(storage_uri)
                .await
                .map_err(|e| RoleError::Internal(e.to_string())),
            None => Err(RoleError::Internal(
                "artefact store not configured".to_string(),
            )),
        }
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
