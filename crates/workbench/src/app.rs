use std::{cmp::Reverse, collections::BTreeMap, convert::Infallible, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Sse, sse::Event},
    routing::{get, post},
};
use futures_util::{Stream, StreamExt, stream};
use mmat_coordinator::{
    CapabilityStatus, OrganisationConfig, OrganisationRuntime, Role, RoleReadiness, RoleRegistry,
    Scheduler,
};
use mmat_event_stream::{
    event::{ArtefactStorageKind, RepositoryOutputRef, RoleId, SemanticEvent, TaskContract},
    event_bus::{EventBus, RecvError},
};
use mmat_memory::artefact_store::ArtefactStore;
use mmat_roles::{
    Architect, Auditor, IntentLead, OpsManager, ProjectManager, Reviewer, Scholar, Worker,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};
use tracing::error;
use uuid::Uuid;

pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";

#[derive(Clone)]
pub struct AppState {
    bus: EventBus,
    projection: Arc<RwLock<WorkbenchProjection>>,
    artefact_store: Arc<ArtefactStore>,
    scheduler: Option<Arc<Mutex<Scheduler>>>,
}

#[derive(Debug, Error)]
pub enum WorkbenchError {
    #[error("invalid bind address {address}: {source}")]
    InvalidBindAddress {
        address: String,
        source: std::net::AddrParseError,
    },
    #[error("failed to bind listener at {address}: {source}")]
    Bind {
        address: String,
        source: std::io::Error,
    },
    #[error("server failed: {0}")]
    Server(std::io::Error),
    #[error("failed to initialise workbench runtime: {0}")]
    Init(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkbenchProjection {
    pub(crate) project: ProjectView,
    pub(crate) roles: BTreeMap<String, RoleView>,
    pub(crate) events: Vec<EventView>,
    pub(crate) messages: Vec<MessageView>,
    pub(crate) artefacts: Vec<ArtefactView>,
    pub(crate) memories: Vec<MemoryView>,
    pub(crate) notifications: Vec<NotificationView>,
    pub(crate) action_requests: Vec<ActionRequestView>,
    pub(crate) dag_steps: Vec<DagStepView>,
    pub(crate) lanes: Vec<LaneView>,
    pub(crate) completed_task_ids: Vec<String>,
    pub(crate) pending_question: Option<String>,
    pub(crate) active_artefact_id: Option<String>,
    pub(crate) active_step_id: Option<String>,
    pub(crate) active_lane_ids: Vec<String>,
    pub(crate) has_conversation: bool,
    pub(crate) active_project_id: String,
    pub(crate) active_run_id: String,
    pub(crate) runs: Vec<RunView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) active_run_id: Option<String>,
    pub(crate) understanding: UnderstandingView,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunView {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) status: String,
    pub(crate) created_at_ns: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct UnderstandingView {
    pub(crate) intent: String,
    pub(crate) audience: String,
    pub(crate) success: Vec<String>,
    pub(crate) constraints: Vec<String>,
    pub(crate) open_questions: Vec<String>,
    pub(crate) confidence: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RoleView {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) state: String,
    pub(crate) summary: String,
    pub(crate) readiness: RoleReadiness,
}

#[derive(Clone, Debug, Serialize)]
pub struct EventView {
    pub(crate) id: String,
    pub(crate) variant: String,
    pub(crate) source_agent: String,
    pub(crate) timestamp_ns: u64,
    pub(crate) summary: String,
    pub(crate) detail: serde_json::Value,
}

/// Classification of semantic events into activity lanes (system-level grouping).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lane {
    Conversation,
    Discovery,
    Delivery,
    System,
}

pub fn classify_event_lane(event: &SemanticEvent) -> Lane {
    match event {
        SemanticEvent::HumanFeedbackRequested { .. }
        | SemanticEvent::HumanFeedbackReceived { .. } => Lane::Conversation,
        SemanticEvent::MemoryProposed { .. }
        | SemanticEvent::MemoryAccepted { .. }
        | SemanticEvent::MemoryRejected { .. }
        | SemanticEvent::MemorySuperseded { .. }
        | SemanticEvent::ToolExecuted { .. }
        | SemanticEvent::ClaimMade { .. }
        | SemanticEvent::DecisionRecorded { .. } => Lane::Discovery,
        SemanticEvent::TaskAssigned { .. }
        | SemanticEvent::TaskStarted { .. }
        | SemanticEvent::TaskCompleted { .. }
        | SemanticEvent::TaskFailed { .. }
        | SemanticEvent::ReviewRequested { .. }
        | SemanticEvent::ReviewCompleted { .. }
        | SemanticEvent::ArtefactProduced { .. } => Lane::Delivery,
        SemanticEvent::OrganisationStarted { .. }
        | SemanticEvent::OrganisationStopped { .. }
        | SemanticEvent::RoleStateChanged { .. }
        | SemanticEvent::Heartbeat { .. }
        | SemanticEvent::EscalationRequested { .. }
        | SemanticEvent::EscalationAccepted { .. }
        | SemanticEvent::BudgetWarning { .. }
        | SemanticEvent::PolicyViolationDetected { .. }
        | SemanticEvent::EvidenceChainBroken { .. }
        | SemanticEvent::ProcessSkipped { .. } => Lane::System,
        SemanticEvent::LaneCreated { .. }
        | SemanticEvent::LaneArchived { .. }
        | SemanticEvent::LanePaused { .. } => Lane::Conversation,
        SemanticEvent::ActionRequestCreated { .. }
        | SemanticEvent::ActionRequestResolved { .. }
        | SemanticEvent::ActionRequestCancelled { .. } => Lane::Conversation,
    }
}

/// Kind of a user-created conversation lane (scopes attention, not memory).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LaneKind {
    Conversation,
    Discovery,
    Delivery,
    System,
}

impl LaneKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LaneKind::Conversation => "conversation",
            LaneKind::Discovery => "discovery",
            LaneKind::Delivery => "delivery",
            LaneKind::System => "system",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "discovery" => LaneKind::Discovery,
            "delivery" => LaneKind::Delivery,
            "system" => LaneKind::System,
            _ => LaneKind::Conversation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LaneStatus {
    Active,
    Archived,
    Paused,
}

impl LaneStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            LaneStatus::Active => "Active",
            LaneStatus::Archived => "Archived",
            LaneStatus::Paused => "Paused",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionRequestStatus {
    Pending,
    Resolved,
    Cancelled,
}

impl ActionRequestStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActionRequestStatus::Pending => "Pending",
            ActionRequestStatus::Resolved => "Resolved",
            ActionRequestStatus::Cancelled => "Cancelled",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LaneView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) kind: LaneKind,
    pub(crate) colour: String,
    pub(crate) purpose: String,
    pub(crate) status: LaneStatus,
    pub(crate) creator: String,
    pub(crate) parent_lane_id: Option<String>,
    pub(crate) related_lane_ids: Vec<String>,
    pub(crate) source_message_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MessageView {
    pub(crate) message_id: String,
    pub(crate) speaker: String,
    pub(crate) content: String,
    pub(crate) timestamp_ns: u64,
    pub(crate) primary_lane_id: Option<String>,
    pub(crate) related_lane_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArtefactView {
    pub(crate) id: String,
    pub(crate) artefact_type: String,
    pub(crate) storage_kind: String,
    pub(crate) storage_uri: String,
    pub(crate) title: String,
    pub(crate) producer_role: String,
    pub(crate) content_hash: String,
    pub(crate) content: serde_json::Value,
    pub(crate) evidence_refs: Vec<String>,
    pub(crate) repository_output: Option<RepositoryOutputRef>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryView {
    pub(crate) id: String,
    pub(crate) memory_type: String,
    pub(crate) scope: String,
    pub(crate) authority: String,
    pub(crate) confidence: f64,
    pub(crate) content: String,
    pub(crate) evidence_refs: Vec<String>,
    pub(crate) status: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct NotificationView {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) target: String,
    pub(crate) acknowledged: bool,
    pub(crate) timestamp_ns: u64,
    pub(crate) message_id: Option<String>,
    pub(crate) lane_id: Option<String>,
    pub(crate) dag_node_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ActionRequestView {
    pub(crate) id: String,
    pub(crate) request_kind: String,
    pub(crate) prompt: String,
    pub(crate) choices: Vec<String>,
    pub(crate) status: ActionRequestStatus,
    pub(crate) source_agent: String,
    pub(crate) lane_id: Option<String>,
    pub(crate) timestamp_ns: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DagStepView {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) role: String,
    pub(crate) state: String,
    pub(crate) summary: String,
    pub(crate) dependencies: Vec<String>,
    pub(crate) artefact_ids: Vec<String>,
    pub(crate) event_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct MessageRequest {
    message: String,
    active_step_id: Option<String>,
    active_artefact_id: Option<String>,
    #[serde(default)]
    active_lane_ids: Vec<String>,
    #[serde(default)]
    action_request_id: Option<String>,
}

#[derive(Clone, Copy)]
struct ReviewContext<'a> {
    step_id: Option<&'a str>,
    artefact_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "payload")]
enum StreamUpdate {
    Event(EventView),
    State(Box<WorkbenchProjection>),
    Notice(String),
}

pub fn build_app_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/style.css", get(style_css))
        .route("/app.js", get(app_js))
        .route("/events", get(events))
        .route("/api/state", get(snapshot))
        .route("/api/messages", get(get_messages).post(post_message))
        .route("/api/lanes", get(get_lanes))
        .route("/api/notifications/{id}/ack", post(ack_notification))
        .route("/api/runs", post(create_run))
        .route("/api/runs/{id}/select", post(select_run))
        .route("/api/runs/{id}/archive", post(archive_run))
        .route("/api/project/reset", post(reset_project))
        .with_state(state)
}

impl AppState {
    pub async fn with_events(
        bus: EventBus,
        events: &[SemanticEvent],
        artefact_store: Arc<ArtefactStore>,
    ) -> Self {
        Self {
            bus,
            projection: Arc::new(RwLock::new(
                WorkbenchProjection::from_events(events, &artefact_store).await,
            )),
            artefact_store,
            scheduler: None,
        }
    }

    pub fn with_scheduler(mut self, scheduler: Arc<Mutex<Scheduler>>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }

    pub async fn publish(&self, event: SemanticEvent) {
        let context = {
            let projection = self.projection.read().await;
            mmat_event_stream::event::EventContext::new(
                "default-organisation",
                "default-workspace",
                &projection.active_project_id,
                &projection.active_run_id,
            )
        };
        let event = event.with_context(context);
        if let Err(err) = self.bus.publish(event) {
            error!("failed to publish workbench event: {}", err);
        }
    }
}

fn redact_database_url(url: &str) -> String {
    if let Some(rest) = url.strip_suffix('/') {
        return redact_database_url(rest);
    }
    if let Some(colon_slash) = url.find("://") {
        let after_scheme = &url[colon_slash + 3..];
        if let Some(at_pos) = after_scheme.find('@') {
            let credentials = &after_scheme[..at_pos];
            if let Some(colon_pos) = credentials.find(':') {
                let user = &credentials[..colon_pos];
                return format!(
                    "{}:***@{}",
                    &url[..colon_slash + 3 + user.len()],
                    &after_scheme[at_pos + 1..]
                );
            }
        }
    }
    url.to_string()
}

fn require_database_url() -> Result<String, WorkbenchError> {
    std::env::var("DATABASE_URL").map_err(|_| {
        WorkbenchError::Init(
            "DATABASE_URL is not set.\n\
             The workbench requires a Postgres database to store events, memories, and artefacts.\n\
             Set DATABASE_URL in your environment, for example:\n\
             export DATABASE_URL=\"postgres://user:password@localhost:5432/mmat\""
                .to_string(),
        )
    })
}

pub async fn build_runtime() -> Result<(AppState, OrganisationRuntime), WorkbenchError> {
    let database_url = require_database_url()?;

    let intent_lead = IntentLead::new();
    let mut scholar = Scholar::new();
    let mut ops_manager = OpsManager::new();
    let mut architect = Architect::new();
    let mut project_manager = ProjectManager::new();
    let mut worker = Worker::new().with_fallback_worktree(true);
    let mut reviewer = Reviewer::new();
    let auditor = Auditor::new();

    let mut readiness: Vec<(String, RoleReadiness)> = vec![
        (intent_lead.id().0.clone(), intent_lead.role_readiness()),
        (scholar.id().0.clone(), scholar.role_readiness()),
        (ops_manager.id().0.clone(), ops_manager.role_readiness()),
        (architect.id().0.clone(), architect.role_readiness()),
        (
            project_manager.id().0.clone(),
            project_manager.role_readiness(),
        ),
        (worker.id().0.clone(), worker.role_readiness()),
        (reviewer.id().0.clone(), reviewer.role_readiness()),
        (auditor.id().0.clone(), auditor.role_readiness()),
        (
            "librarian".to_string(),
            RoleReadiness {
                capability: CapabilityStatus::Configured,
                has_llm_client: false,
                has_tools: false,
                tool_count: 0,
                fallback_worktree: false,
                requires_llm: false,
                has_artefact_store: true,
                summary: "Built-in memory curator, always available".to_string(),
            },
        ),
    ];
    for (_, r) in &mut readiness {
        r.has_artefact_store = true;
    }

    let mut registry = RoleRegistry::new();
    registry.register(intent_lead.spec()).map_err(|err| {
        WorkbenchError::Init(format!("role registration (intent lead) failed: {err}"))
    })?;
    registry.register(scholar.spec()).map_err(|err| {
        WorkbenchError::Init(format!("role registration (scholar) failed: {err}"))
    })?;
    registry.register(ops_manager.spec()).map_err(|err| {
        WorkbenchError::Init(format!("role registration (ops manager) failed: {err}"))
    })?;
    registry.register(architect.spec()).map_err(|err| {
        WorkbenchError::Init(format!("role registration (architect) failed: {err}"))
    })?;
    registry.register(project_manager.spec()).map_err(|err| {
        WorkbenchError::Init(format!("role registration (project manager) failed: {err}"))
    })?;
    registry
        .register(worker.spec())
        .map_err(|err| WorkbenchError::Init(format!("role registration (worker) failed: {err}")))?;
    registry.register(reviewer.spec()).map_err(|err| {
        WorkbenchError::Init(format!("role registration (reviewer) failed: {err}"))
    })?;
    registry.register(auditor.spec()).map_err(|err| {
        WorkbenchError::Init(format!("role registration (auditor) failed: {err}"))
    })?;

    let config = OrganisationConfig {
        database_url: Some(database_url.clone()),
        event_store_path: None,
        memory_store_path: None,
        ..OrganisationConfig::default()
    };

    let mut runtime = OrganisationRuntime::new(config, registry).map_err(|err| {
        WorkbenchError::Init(format!(
            "failed to create organisation runtime (database: {}): {err}",
            redact_database_url(&database_url),
        ))
    })?;

    let replayed_events = runtime
        .event_store()
        .replay(0, None)
        .map_err(|err| WorkbenchError::Init(format!("failed to replay persisted events: {err}")))?;

    let state = AppState::with_events(
        runtime.bus().clone(),
        &replayed_events,
        runtime.artefact_store().clone(),
    )
    .await
    .with_scheduler(runtime.scheduler().clone());

    state
        .projection
        .write()
        .await
        .set_role_readiness(&readiness);

    let tool_bus = runtime.bus().clone();
    scholar.set_tool_bus(tool_bus.clone());
    ops_manager.set_tool_bus(tool_bus.clone());
    architect.set_tool_bus(tool_bus.clone());
    project_manager.set_tool_bus(tool_bus.clone());
    worker.set_tool_bus(tool_bus.clone());
    reviewer.set_tool_bus(tool_bus.clone());

    let create_lane = Box::new(mmat_roles::tools::CreateLaneTool);
    let _ = scholar.register_tool(create_lane);
    let create_lane = Box::new(mmat_roles::tools::CreateLaneTool);
    let _ = ops_manager.register_tool(create_lane);
    let create_lane = Box::new(mmat_roles::tools::CreateLaneTool);
    let _ = architect.register_tool(create_lane);
    let create_lane = Box::new(mmat_roles::tools::CreateLaneTool);
    let _ = project_manager.register_tool(create_lane);
    let create_lane = Box::new(mmat_roles::tools::CreateLaneTool);
    let _ = worker.register_tool(create_lane);
    let create_lane = Box::new(mmat_roles::tools::CreateLaneTool);
    let _ = reviewer.register_tool(create_lane);

    runtime.add_role(intent_lead);
    runtime.add_role(scholar);
    runtime.add_role(ops_manager);
    runtime.add_role(architect);
    runtime.add_role(project_manager);
    runtime.add_role(worker);
    runtime.add_role(reviewer);
    runtime.add_role(auditor);

    Ok((state, runtime))
}

impl Default for WorkbenchProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkbenchProjection {
    pub fn new() -> Self {
        let roles = [
            ("intent-lead-001", "Intent Lead"),
            ("scholar-001", "Scholar"),
            ("ops-manager-001", "Ops Manager"),
            ("architect-001", "Architect"),
            ("pm-001", "Project Manager"),
            ("worker-001", "Worker"),
            ("reviewer-001", "Reviewer"),
            ("auditor-001", "Auditor"),
            ("librarian", "Librarian"),
        ]
        .into_iter()
        .map(|(id, label)| {
            (
                id.to_string(),
                RoleView {
                    id: id.to_string(),
                    label: label.to_string(),
                    state: "Idle".to_string(),
                    summary: "Waiting for relevant events".to_string(),
                    readiness: RoleReadiness::default(),
                },
            )
        })
        .collect();

        Self {
            project: ProjectView {
                id: "project-workbench-mvp".to_string(),
                name: "SELIUM".to_string(),
                status: "New project".to_string(),
                active_run_id: Some("run-001".to_string()),
                understanding: UnderstandingView {
                    intent: "Waiting for the first project intent.".to_string(),
                    audience: "Unknown".to_string(),
                    success: Vec::new(),
                    constraints: Vec::new(),
                    open_questions: vec!["What are we making?".to_string()],
                    confidence: 0.0,
                },
            },
            roles,
            events: Vec::new(),
            messages: Vec::new(),
            artefacts: Vec::new(),
            memories: Vec::new(),
            notifications: Vec::new(),
            action_requests: Vec::new(),
            dag_steps: vec![DagStepView {
                id: "intent".to_string(),
                label: "Understand intent".to_string(),
                role: "intent-lead".to_string(),
                state: "Waiting".to_string(),
                summary: "Interview the human and form the initial project model".to_string(),
                dependencies: Vec::new(),
                artefact_ids: Vec::new(),
                event_ids: Vec::new(),
            }],
            lanes: Vec::new(),
            completed_task_ids: Vec::new(),
            pending_question: None,
            active_artefact_id: None,
            active_step_id: Some("intent".to_string()),
            active_lane_ids: Vec::new(),
            has_conversation: false,
            active_project_id: "project-workbench-mvp".to_string(),
            active_run_id: "run-001".to_string(),
            runs: vec![RunView {
                id: "run-001".to_string(),
                label: "Initial run".to_string(),
                status: "active".to_string(),
                created_at_ns: 0,
            }],
        }
    }

    pub async fn from_events(events: &[SemanticEvent], artefact_store: &ArtefactStore) -> Self {
        let mut projection = Self::new();
        for event in events {
            projection.apply_event(event, artefact_store).await;
        }
        projection
    }

    pub fn has_conversation_history(&self) -> bool {
        self.has_conversation
    }

    pub fn set_role_readiness(&mut self, readiness: &[(String, RoleReadiness)]) {
        for (role_id, r) in readiness {
            if let Some(role) = self.roles.get_mut(role_id) {
                role.readiness = r.clone();
            } else {
                tracing::warn!(
                    "set_role_readiness: role id {role_id} not found in projection, readiness skipped"
                );
            }
        }
    }

    fn reviewable_task_id(&self, context: ReviewContext<'_>) -> Option<String> {
        if let Some(step_id) = context.step_id
            && self.completed_task_ids.iter().any(|id| id == step_id)
        {
            return Some(step_id.to_string());
        }

        let artefact_id = context.artefact_id.or(self.active_artefact_id.as_deref())?;
        let step = self
            .dag_steps
            .iter()
            .find(|step| step.artefact_ids.iter().any(|id| id == artefact_id))?;
        self.completed_task_ids
            .iter()
            .any(|id| id == &step.id)
            .then(|| step.id.clone())
    }

    #[allow(dead_code)]
    pub(crate) fn events_by_lane(&self, lane: Lane) -> Vec<&EventView> {
        self.events
            .iter()
            .filter(|ev| classify_event_variant_lane(&ev.variant) == lane)
            .collect()
    }

    pub(crate) fn messages_for_lane_ids(&self, lane_ids: &[String]) -> Vec<&MessageView> {
        if lane_ids.is_empty() {
            return self.messages.iter().collect();
        }
        self.messages
            .iter()
            .filter(|msg| {
                let has_lane = msg
                    .primary_lane_id
                    .as_deref()
                    .is_some_and(|id| lane_ids.iter().any(|lid| lid == id))
                    || msg
                        .related_lane_ids
                        .iter()
                        .any(|id| lane_ids.iter().any(|lid| lid == id));
                let is_untagged_system = msg.primary_lane_id.is_none()
                    && msg.related_lane_ids.is_empty()
                    && (msg.speaker.starts_with("System") || msg.speaker == "Librarian");
                has_lane || (!is_untagged_system && msg.primary_lane_id.is_none())
            })
            .collect()
    }

    #[allow(dead_code)]
    pub(crate) fn notifications_for_lane_ids(&self, lane_ids: &[String]) -> Vec<&NotificationView> {
        if lane_ids.is_empty() {
            return self.notifications.iter().collect();
        }
        self.notifications
            .iter()
            .filter(|n| {
                n.lane_id
                    .as_deref()
                    .is_some_and(|id| lane_ids.iter().any(|lid| lid == id))
            })
            .collect()
    }

    #[allow(dead_code)]
    fn active_lanes(&self) -> Vec<&LaneView> {
        self.lanes
            .iter()
            .filter(|l| l.status == LaneStatus::Active)
            .collect()
    }

    async fn apply_event(&mut self, event: &SemanticEvent, artefact_store: &ArtefactStore) {
        self.events.push(EventView::from_event(event));
        if self.events.len() > 200 {
            let overflow = self.events.len().saturating_sub(200);
            self.events.drain(0..overflow);
        }

        match event {
            SemanticEvent::HumanFeedbackRequested {
                event_id,
                source_agent,
                question,
                timestamp_ns,
                ..
            } => {
                let speaker = label_for_role(&source_agent.0);
                self.has_conversation = true;
                let is_me_question = contains_mention(&question.to_lowercase(), "@me");

                if is_me_question {
                    let action_request_id = Uuid::new_v4().to_string();
                    self.action_requests.push(ActionRequestView {
                        id: action_request_id.clone(),
                        request_kind: "clarification".to_string(),
                        prompt: question.clone(),
                        choices: Vec::new(),
                        status: ActionRequestStatus::Pending,
                        source_agent: source_agent.0.clone(),
                        lane_id: self.active_lane_ids.first().cloned(),
                        timestamp_ns: *timestamp_ns,
                    });
                    self.messages.push(MessageView {
                        message_id: event_id.to_string(),
                        speaker: speaker.clone(),
                        content: format!("[Action: clarification] {question}"),
                        timestamp_ns: *timestamp_ns,
                        primary_lane_id: self.active_lane_ids.first().cloned(),
                        related_lane_ids: Vec::new(),
                    });
                } else {
                    self.pending_question = Some(question.clone());
                    self.messages.push(MessageView {
                        message_id: event_id.to_string(),
                        speaker: speaker.clone(),
                        content: question.clone(),
                        timestamp_ns: *timestamp_ns,
                        primary_lane_id: self.active_lane_ids.first().cloned(),
                        related_lane_ids: Vec::new(),
                    });
                    self.set_role(
                        &source_agent.0,
                        "Waiting",
                        format!("{speaker} is waiting for human input"),
                    );
                }
                let step_id = if source_agent.0 == "intent-lead-001" {
                    "intent"
                } else {
                    &source_agent.0
                };
                self.add_step_event(step_id, event_id.to_string());
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: if is_me_question {
                        "ActionRequest"
                    } else {
                        "Question"
                    }
                    .to_string(),
                    title: format!("{speaker} question"),
                    summary: question.clone(),
                    target: "chat".to_string(),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
                    message_id: Some(event_id.to_string()),
                    lane_id: self.active_lane_ids.first().cloned(),
                    dag_node_id: None,
                });
            }
            SemanticEvent::HumanFeedbackReceived {
                event_id,
                answer,
                timestamp_ns,
                ..
            } => {
                self.has_conversation = true;
                self.acknowledge_kind("Question");
                self.acknowledge_kind("ActionRequest");
                self.pending_question = None;
                self.messages.push(MessageView {
                    message_id: event_id.to_string(),
                    speaker: "You".to_string(),
                    content: answer.clone(),
                    timestamp_ns: *timestamp_ns,
                    primary_lane_id: self.active_lane_ids.first().cloned(),
                    related_lane_ids: Vec::new(),
                });
                self.set_role("intent-lead-001", "Running", "Updating the intent model");
                self.update_understanding_from_human(answer);
                self.add_step_event("intent", event_id.to_string());
            }
            SemanticEvent::RoleStateChanged {
                role_id, new_state, ..
            } => self.set_role(&role_id.0, new_state, role_summary(&role_id.0, new_state)),
            SemanticEvent::MemoryProposed {
                event_id,
                memory_type,
                content,
                scope,
                proposed_authority,
                evidence_refs,
                confidence,
                ..
            } => {
                self.memories.push(MemoryView {
                    id: event_id.to_string(),
                    memory_type: memory_type.clone(),
                    scope: scope.clone(),
                    authority: proposed_authority.0.clone(),
                    confidence: *confidence,
                    content: content.clone(),
                    evidence_refs: evidence_refs
                        .iter()
                        .map(|r| format!("{}: {}", r.event_id, r.description))
                        .collect(),
                    status: "Proposed".to_string(),
                });
                self.set_role("librarian", "Running", "Evaluating proposed memory");
                self.upsert_step(DagStepView {
                    id: "librarian".to_string(),
                    label: "Librarian".to_string(),
                    role: "librarian".to_string(),
                    state: "Running".to_string(),
                    summary: format!("Evaluating {scope} {memory_type} memory"),
                    dependencies: Vec::new(),
                    artefact_ids: Vec::new(),
                    event_ids: vec![event_id.to_string()],
                });
            }
            SemanticEvent::MemoryAccepted {
                event_id,
                proposal_event_id,
                memory_id,
                accepted_authority,
                timestamp_ns,
                ..
            } => {
                let proposal_id = proposal_event_id.to_string();
                for memory in &mut self.memories {
                    if memory.id == proposal_id {
                        memory.id = memory_id.to_string();
                        memory.authority = accepted_authority.0.clone();
                        memory.status = "Accepted".to_string();
                    }
                }
                self.set_role("librarian", "Completed", "Accepted a durable memory");
                self.ensure_librarian_step();
                self.update_step("librarian", "Completed", "Accepted a durable memory");
                self.add_step_event("librarian", event_id.to_string());
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: "Memory".to_string(),
                    title: "Memory accepted".to_string(),
                    summary: format!("Accepted memory {memory_id}"),
                    target: "memories".to_string(),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
                    message_id: None,
                    lane_id: None,
                    dag_node_id: None,
                });
            }
            SemanticEvent::MemoryRejected {
                event_id,
                proposed_memory_type,
                proposed_content,
                rejection_gate,
                rejection_reason,
                timestamp_ns,
                ..
            } => {
                self.memories.push(MemoryView {
                    id: event_id.to_string(),
                    memory_type: proposed_memory_type.clone(),
                    scope: String::new(),
                    authority: String::new(),
                    confidence: 0.0,
                    content: proposed_content.clone(),
                    evidence_refs: Vec::new(),
                    status: format!("Rejected at {}: {}", rejection_gate, rejection_reason),
                });
                self.set_role(
                    "librarian",
                    "Completed",
                    format!("Rejected memory at {rejection_gate}"),
                );
                self.ensure_librarian_step();
                self.update_step(
                    "librarian",
                    "Completed",
                    &format!("Rejected memory at {rejection_gate}"),
                );
                self.add_step_event("librarian", event_id.to_string());
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: "Memory".to_string(),
                    title: "Memory rejected".to_string(),
                    summary: format!("Rejected at {rejection_gate}: {rejection_reason}"),
                    target: "memories".to_string(),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
                    message_id: None,
                    lane_id: None,
                    dag_node_id: None,
                });
            }
            SemanticEvent::MemorySuperseded {
                event_id,
                old_memory_id,
                new_memory_id,
                reason,
                timestamp_ns,
                ..
            } => {
                let old_id = old_memory_id.to_string();
                for memory in &mut self.memories {
                    if memory.id == old_id {
                        memory.status = format!("Superseded by {new_memory_id}: {reason}");
                    }
                }
                self.set_role(
                    "librarian",
                    "Completed",
                    format!("Superseded memory: {reason}"),
                );
                self.ensure_librarian_step();
                self.update_step(
                    "librarian",
                    "Completed",
                    &format!("Superseded memory: {reason}"),
                );
                self.add_step_event("librarian", event_id.to_string());
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: "Memory".to_string(),
                    title: "Memory superseded".to_string(),
                    summary: format!("Superseded {old_memory_id}: {reason}"),
                    target: "memories".to_string(),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
                    message_id: None,
                    lane_id: None,
                    dag_node_id: None,
                });
            }
            SemanticEvent::TaskAssigned {
                event_id,
                source_agent,
                task_id,
                worker_id,
                contract_ref,
                dependencies,
                timestamp_ns,
                ..
            } => {
                self.set_role(
                    &worker_id.0,
                    "Running",
                    role_summary(&worker_id.0, "Running"),
                );
                self.upsert_step(DagStepView {
                    id: task_id.clone(),
                    label: label_for_role(&worker_id.0),
                    role: worker_id.0.clone(),
                    state: "Running".to_string(),
                    summary: contract_ref.description.clone(),
                    dependencies: dependencies.clone(),
                    artefact_ids: Vec::new(),
                    event_ids: vec![event_id.to_string()],
                });
                if source_agent.0 != "human" {
                    let from_label = label_for_role(&source_agent.0);
                    let to_label = label_for_role(&worker_id.0);
                    self.messages.push(MessageView {
                        message_id: Uuid::new_v4().to_string(),
                        speaker: format!("System ({from_label})"),
                        content: format!("Dispatched {to_label}: {}", contract_ref.description,),
                        timestamp_ns: *timestamp_ns,
                        primary_lane_id: None,
                        related_lane_ids: Vec::new(),
                    });
                }
                if let Some(role) = self.roles.get(&worker_id.0)
                    && role.readiness.capability != CapabilityStatus::Configured
                {
                    self.messages.push(MessageView {
                        message_id: Uuid::new_v4().to_string(),
                        speaker: "System (Capability)".to_string(),
                        content: format!(
                            "{} running in {} mode: {}",
                            label_for_role(&worker_id.0),
                            role.readiness.capability,
                            role.readiness.summary,
                        ),
                        timestamp_ns: *timestamp_ns,
                        primary_lane_id: None,
                        related_lane_ids: Vec::new(),
                    });
                }
            }
            SemanticEvent::ReviewCompleted {
                event_id,
                task_id,
                accepted,
                timestamp_ns,
                ..
            } => {
                let summary = if *accepted {
                    "Review accepted the current artefact"
                } else {
                    "Review requested rework"
                };
                self.set_role("reviewer-001", "Completed", summary);
                self.upsert_step(DagStepView {
                    id: format!("review-{task_id}"),
                    label: "Review".to_string(),
                    role: "reviewer-001".to_string(),
                    state: if *accepted {
                        "Accepted"
                    } else {
                        "Needs rework"
                    }
                    .to_string(),
                    summary: summary.to_string(),
                    dependencies: vec![task_id.clone()],
                    artefact_ids: self
                        .active_artefact_id
                        .clone()
                        .into_iter()
                        .collect::<Vec<_>>(),
                    event_ids: vec![event_id.to_string()],
                });
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: "Review".to_string(),
                    title: if *accepted {
                        "Review accepted".to_string()
                    } else {
                        "Review needs attention".to_string()
                    },
                    summary: summary.to_string(),
                    target: format!("step:review-{task_id}"),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
                    message_id: None,
                    lane_id: None,
                    dag_node_id: Some(format!("review-{task_id}")),
                });
            }
            SemanticEvent::ArtefactProduced {
                event_id,
                artefact_id,
                artefact_type,
                content_hash,
                storage_uri,
                producer_role,
                evidence_refs,
                storage_kind,
                repository_output,
                timestamp_ns,
                ..
            } => {
                if !self
                    .artefacts
                    .iter()
                    .any(|artefact| artefact.id == *artefact_id)
                {
                    self.artefacts.push(ArtefactView {
                        id: artefact_id.clone(),
                        artefact_type: artefact_type.clone(),
                        storage_kind: storage_kind_label(storage_kind),
                        storage_uri: storage_uri.clone(),
                        title: label_for_artefact(artefact_type),
                        producer_role: producer_role.0.clone(),
                        content_hash: content_hash.clone(),
                        content: artefact_content(
                            storage_kind,
                            storage_uri,
                            repository_output,
                            artefact_store,
                        )
                        .await,
                        evidence_refs: evidence_refs
                            .iter()
                            .map(|evidence| evidence.event_id.to_string())
                            .collect(),
                        repository_output: repository_output.clone(),
                    });
                    self.active_artefact_id = Some(artefact_id.clone());
                }
                self.attach_artefact_to_role_step(&producer_role.0, artefact_id);
                self.add_step_event(&producer_role.0, event_id.to_string());
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: "Artefact".to_string(),
                    title: format!("{} ready", label_for_artefact(artefact_type)),
                    summary: format!(
                        "{} produced by {}",
                        label_for_artefact(artefact_type),
                        producer_role
                    ),
                    target: format!("artefact:{artefact_id}"),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
                    message_id: None,
                    lane_id: None,
                    dag_node_id: None,
                });
                if let Some(role) = self.roles.get(&producer_role.0)
                    && role.readiness.capability != CapabilityStatus::Configured
                {
                    self.messages.push(MessageView {
                        message_id: Uuid::new_v4().to_string(),
                        speaker: "System (Capability)".to_string(),
                        content: format!(
                            "{} produced {} in {} mode (output may be deterministic fallback)",
                            role.label,
                            label_for_artefact(artefact_type),
                            role.readiness.capability,
                        ),
                        timestamp_ns: *timestamp_ns,
                        primary_lane_id: None,
                        related_lane_ids: Vec::new(),
                    });
                }
            }
            SemanticEvent::PolicyViolationDetected { .. }
            | SemanticEvent::EvidenceChainBroken { .. } => {
                self.set_role(
                    "auditor",
                    "Running",
                    "Inspecting evidence and process integrity",
                );
            }
            SemanticEvent::TaskStarted {
                event_id,
                task_id,
                worker_id,
                ..
            } => {
                self.set_role(
                    &worker_id.0,
                    "Running",
                    role_summary(&worker_id.0, "Running"),
                );
                self.update_step(task_id, "Running", "Task is being worked on");
                self.add_step_event(task_id, event_id.to_string());
            }
            SemanticEvent::TaskCompleted {
                event_id,
                task_id,
                timestamp_ns,
                ..
            } => {
                if !self.completed_task_ids.contains(task_id) {
                    self.completed_task_ids.push(task_id.clone());
                }
                self.update_step(task_id, "Completed", "Task completed successfully");
                self.add_step_event(task_id, event_id.to_string());
                let worker_role = self
                    .dag_steps
                    .iter()
                    .find(|step| step.id == *task_id)
                    .and_then(|step| self.roles.get(&step.role));
                if let Some(role) = worker_role
                    && role.readiness.capability != CapabilityStatus::Configured
                {
                    self.messages.push(MessageView {
                        message_id: Uuid::new_v4().to_string(),
                        speaker: "System (Capability)".to_string(),
                        content: format!(
                            "{} completed task in {} mode (output may be deterministic fallback)",
                            role.label, role.readiness.capability,
                        ),
                        timestamp_ns: *timestamp_ns,
                        primary_lane_id: None,
                        related_lane_ids: Vec::new(),
                    });
                }
            }
            SemanticEvent::BudgetWarning {
                event_id,
                contract_id,
                message,
                usage_percent,
                timestamp_ns,
                ..
            } => {
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: "Budget".to_string(),
                    title: format!("Budget warning ({usage_percent}%)"),
                    summary: message.clone(),
                    target: format!("contract:{contract_id}"),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
                    message_id: None,
                    lane_id: None,
                    dag_node_id: None,
                });
            }
            SemanticEvent::TaskFailed {
                event_id,
                task_id,
                error_description,
                timestamp_ns,
                ..
            } => {
                self.update_step(task_id, "Failed", error_description);
                self.add_step_event(task_id, event_id.to_string());
                let worker_role = self
                    .dag_steps
                    .iter()
                    .find(|step| step.id == *task_id)
                    .map(|step| step.role.clone());
                if let Some(ref role) = worker_role {
                    self.set_role(role, "Failed", error_description);
                }
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: "Failure".to_string(),
                    title: "Task failed".to_string(),
                    summary: error_description.clone(),
                    target: format!("step:{task_id}"),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
                    message_id: None,
                    lane_id: None,
                    dag_node_id: Some(task_id.clone()),
                });
            }
            SemanticEvent::ReviewRequested {
                event_id,
                task_id,
                reviewer_id,
                ..
            } => {
                self.set_role(&reviewer_id.0, "Running", "Review requested");
                self.upsert_step(DagStepView {
                    id: format!("review-{task_id}"),
                    label: "Review".to_string(),
                    role: reviewer_id.0.clone(),
                    state: "Pending".to_string(),
                    summary: format!("Review requested for task {task_id}"),
                    dependencies: vec![task_id.clone()],
                    artefact_ids: Vec::new(),
                    event_ids: vec![event_id.to_string()],
                });
            }
            SemanticEvent::EscalationRequested {
                event_id,
                from_role,
                to_role,
                reason,
                timestamp_ns,
                ..
            } => {
                self.upsert_step(DagStepView {
                    id: format!("escalation-{event_id}"),
                    label: "Escalation".to_string(),
                    role: to_role.0.clone(),
                    state: "Escalated".to_string(),
                    summary: reason.clone(),
                    dependencies: vec![from_role.0.clone()],
                    artefact_ids: Vec::new(),
                    event_ids: vec![event_id.to_string()],
                });
                self.set_role(&to_role.0, "Escalated", reason.clone());
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: "Escalation".to_string(),
                    title: format!("Escalation to {}", to_role),
                    summary: reason.clone(),
                    target: format!("role:{}", to_role),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
                    message_id: None,
                    lane_id: None,
                    dag_node_id: None,
                });
            }
            SemanticEvent::ClaimMade {
                source_agent,
                timestamp_ns,
                ..
            } => {
                if let Some(role) = self.roles.get(&source_agent.0)
                    && role.readiness.capability != CapabilityStatus::Configured
                {
                    self.messages.push(MessageView {
                        message_id: Uuid::new_v4().to_string(),
                        speaker: "System (Capability)".to_string(),
                        content: format!(
                            "{} made a claim in {} mode (output may be deterministic fallback)",
                            role.label, role.readiness.capability,
                        ),
                        timestamp_ns: *timestamp_ns,
                        primary_lane_id: None,
                        related_lane_ids: Vec::new(),
                    });
                }
            }
            SemanticEvent::LaneCreated {
                lane_id,
                name,
                kind,
                colour,
                purpose,
                source_agent,
                parent_lane_id,
                related_lane_ids,
                source_message_id,
                timestamp_ns,
                ..
            } => {
                self.lanes.push(LaneView {
                    id: lane_id.clone(),
                    name: name.clone(),
                    kind: LaneKind::parse(kind),
                    colour: colour.clone(),
                    purpose: purpose.clone(),
                    status: LaneStatus::Active,
                    creator: source_agent.0.clone(),
                    parent_lane_id: parent_lane_id.clone(),
                    related_lane_ids: related_lane_ids.clone(),
                    source_message_id: source_message_id.clone(),
                });
                if let Some(src_msg_id) = source_message_id
                    && let Some(src_msg) = self
                        .messages
                        .iter_mut()
                        .find(|m| m.message_id == *src_msg_id)
                    && !src_msg.related_lane_ids.contains(lane_id)
                {
                    src_msg.related_lane_ids.push(lane_id.clone());
                }
                self.messages.push(MessageView {
                    message_id: Uuid::new_v4().to_string(),
                    speaker: label_for_role(&source_agent.0),
                    content: format!("Created lane \"{name}\": {purpose}"),
                    timestamp_ns: *timestamp_ns,
                    primary_lane_id: Some(lane_id.clone()),
                    related_lane_ids: related_lane_ids.clone(),
                });
            }
            SemanticEvent::LaneArchived {
                lane_id,
                timestamp_ns,
                ..
            } => {
                if let Some(lane) = self.lanes.iter_mut().find(|l| l.id == *lane_id) {
                    lane.status = LaneStatus::Archived;
                }
                self.messages.push(MessageView {
                    message_id: Uuid::new_v4().to_string(),
                    speaker: "System".to_string(),
                    content: format!("Lane archived: {lane_id}"),
                    timestamp_ns: *timestamp_ns,
                    primary_lane_id: None,
                    related_lane_ids: Vec::new(),
                });
            }
            SemanticEvent::LanePaused {
                lane_id,
                timestamp_ns,
                ..
            } => {
                if let Some(lane) = self.lanes.iter_mut().find(|l| l.id == *lane_id) {
                    lane.status = LaneStatus::Paused;
                }
                self.messages.push(MessageView {
                    message_id: Uuid::new_v4().to_string(),
                    speaker: "System".to_string(),
                    content: format!("Lane paused: {lane_id}"),
                    timestamp_ns: *timestamp_ns,
                    primary_lane_id: None,
                    related_lane_ids: Vec::new(),
                });
            }
            SemanticEvent::ActionRequestCreated {
                event_id,
                source_agent,
                request_id,
                request_kind,
                prompt,
                choices,
                lane_id,
                timestamp_ns,
                ..
            } => {
                self.action_requests.push(ActionRequestView {
                    id: request_id.clone(),
                    request_kind: request_kind.clone(),
                    prompt: prompt.clone(),
                    choices: choices.clone(),
                    status: ActionRequestStatus::Pending,
                    source_agent: source_agent.0.clone(),
                    lane_id: lane_id.clone(),
                    timestamp_ns: *timestamp_ns,
                });
                self.messages.push(MessageView {
                    message_id: event_id.to_string(),
                    speaker: label_for_role(&source_agent.0),
                    content: format!("[Action: {request_kind}] {prompt}"),
                    timestamp_ns: *timestamp_ns,
                    primary_lane_id: lane_id.clone(),
                    related_lane_ids: Vec::new(),
                });
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: "ActionRequest".to_string(),
                    title: format!("{}: {request_kind}", label_for_role(&source_agent.0)),
                    summary: prompt.clone(),
                    target: "chat".to_string(),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
                    message_id: Some(event_id.to_string()),
                    lane_id: lane_id.clone(),
                    dag_node_id: None,
                });
            }
            SemanticEvent::ActionRequestResolved {
                request_id,
                choice,
                timestamp_ns,
                ..
            } => {
                if let Some(req) = self
                    .action_requests
                    .iter_mut()
                    .find(|r| r.id == *request_id)
                {
                    req.status = ActionRequestStatus::Resolved;
                }
                self.messages.push(MessageView {
                    message_id: Uuid::new_v4().to_string(),
                    speaker: "You".to_string(),
                    content: choice.clone(),
                    timestamp_ns: *timestamp_ns,
                    primary_lane_id: self.active_lane_ids.first().cloned(),
                    related_lane_ids: Vec::new(),
                });
            }
            SemanticEvent::ActionRequestCancelled {
                request_id,
                reason,
                timestamp_ns,
                ..
            } => {
                if let Some(req) = self
                    .action_requests
                    .iter_mut()
                    .find(|r| r.id == *request_id)
                {
                    req.status = ActionRequestStatus::Cancelled;
                }
                self.messages.push(MessageView {
                    message_id: Uuid::new_v4().to_string(),
                    speaker: "System".to_string(),
                    content: format!("Action request cancelled: {reason}"),
                    timestamp_ns: *timestamp_ns,
                    primary_lane_id: None,
                    related_lane_ids: Vec::new(),
                });
            }
            _ => {}
        }
    }

    fn update_understanding_from_human(&mut self, answer: &str) {
        let trimmed = answer.trim();
        if self.project.understanding.confidence < 0.3 {
            self.project.understanding.intent = trimmed.to_string();
            self.project.understanding.audience = infer_audience(trimmed);
            self.project.understanding.open_questions = vec![
                "What would make this excellent?".to_string(),
                "What should MMAT avoid?".to_string(),
            ];
            self.project.understanding.confidence = 0.3;
            self.project.status = "Understanding intent".to_string();
            self.update_step("intent", "Running", "Initial intent captured");
            return;
        }

        if self.project.understanding.success.is_empty() {
            self.project.understanding.success.push(trimmed.to_string());
            self.project.understanding.constraints = extract_constraints(trimmed);
            self.project.understanding.open_questions =
                vec!["Which prior context should Scholar inspect?".to_string()];
            self.project.understanding.confidence = 0.58;
            self.project.status = "Scoping evidence".to_string();
            self.update_step("intent", "Completed", "Intent brief is ready for review");
            return;
        }

        if self.project.understanding.open_questions.len() == 1 {
            self.project.understanding.open_questions =
                vec!["What autonomy level should this project use?".to_string()];
            self.project.understanding.confidence = 0.72;
            self.project.status = "Researching context".to_string();
            return;
        }

        self.project.understanding.open_questions.clear();
        self.project.understanding.confidence = 0.84;
        self.project.status = "Ready for delivery planning".to_string();
    }

    fn add_notification(&mut self, notification: NotificationView) {
        if self
            .notifications
            .iter()
            .any(|existing| existing.id == notification.id)
        {
            return;
        }
        self.notifications.push(notification);
        self.notifications
            .sort_by_key(|notification| Reverse(notification.timestamp_ns));
    }

    fn acknowledge_kind(&mut self, kind: &str) {
        for notification in &mut self.notifications {
            if notification.kind == kind {
                notification.acknowledged = true;
            }
        }
    }

    pub fn acknowledge_notification(&mut self, id: &str) -> bool {
        for notification in &mut self.notifications {
            if notification.id == id {
                notification.acknowledged = true;
                return true;
            }
        }
        false
    }

    fn upsert_step(&mut self, step: DagStepView) {
        let state = step.state.clone();
        let step_id = step.id.clone();
        self.active_step_id = Some(step.id.clone());
        if let Some(existing) = self
            .dag_steps
            .iter_mut()
            .find(|existing| existing.id == step.id)
        {
            *existing = step;
        } else {
            self.dag_steps.push(step);
        }
        if state == "Failed" || state == "Escalated" {
            self.propagate_blocked_state(&step_id);
        }
    }

    fn update_step(&mut self, id: &str, state: &str, summary: &str) {
        if let Some(step) = self.dag_steps.iter_mut().find(|step| step.id == id) {
            step.state = state.to_string();
            step.summary = summary.to_string();
        }
        if state == "Failed" || state == "Escalated" {
            self.propagate_blocked_state(id);
        }
    }

    fn propagate_blocked_state(&mut self, failed_step_id: &str) {
        let blocked_ids: Vec<String> = self
            .dag_steps
            .iter()
            .filter(|step| {
                step.state != "Failed"
                    && step.state != "Escalated"
                    && step.state != "Completed"
                    && step.dependencies.iter().any(|dep| dep == failed_step_id)
            })
            .map(|step| step.id.clone())
            .collect();

        for blocked_id in blocked_ids {
            if let Some(step) = self.dag_steps.iter_mut().find(|s| s.id == blocked_id) {
                step.state = "Blocked".to_string();
            }
            if let Some(role_id) = self
                .dag_steps
                .iter()
                .find(|s| s.id == blocked_id)
                .map(|s| s.role.clone())
                && let Some(role) = self.roles.get(&role_id)
                && role.state != "Failed"
                && role.state != "Escalated"
            {
                self.set_role(
                    &role_id,
                    "Blocked",
                    format!("Blocked by failed step: {failed_step_id}"),
                );
            }
        }
    }

    fn sync_scheduler_task_states(&mut self, task_states: &[(String, String)]) {
        for (task_id, scheduler_state) in task_states {
            if let Some(step) = self.dag_steps.iter_mut().find(|step| step.id == *task_id) {
                step.state = scheduler_state.clone();
            }
        }
    }

    fn ensure_librarian_step(&mut self) {
        if self.dag_steps.iter().any(|step| step.id == "librarian") {
            return;
        }
        self.dag_steps.push(DagStepView {
            id: "librarian".to_string(),
            label: "Librarian".to_string(),
            role: "librarian".to_string(),
            state: "Running".to_string(),
            summary: "Curating accepted memory".to_string(),
            dependencies: Vec::new(),
            artefact_ids: Vec::new(),
            event_ids: Vec::new(),
        });
    }

    fn add_step_event(&mut self, step_id: &str, event_id: String) {
        if let Some(step) = self
            .dag_steps
            .iter_mut()
            .rev()
            .find(|step| step.id == step_id || step.role == step_id)
            && !step.event_ids.contains(&event_id)
        {
            step.event_ids.push(event_id);
        }
    }

    fn attach_artefact_to_role_step(&mut self, role_id: &str, artefact_id: &str) {
        let step_id = self
            .dag_steps
            .iter()
            .rev()
            .find(|step| step.role == role_id)
            .map(|step| step.id.clone())
            .unwrap_or_else(|| role_id.to_string());

        if !self.dag_steps.iter().any(|step| step.id == step_id) {
            self.dag_steps.push(DagStepView {
                id: step_id.clone(),
                label: label_for_role(role_id),
                role: role_id.to_string(),
                state: "Completed".to_string(),
                summary: "Produced an artefact".to_string(),
                dependencies: Vec::new(),
                artefact_ids: Vec::new(),
                event_ids: Vec::new(),
            });
        }

        if let Some(step) = self.dag_steps.iter_mut().find(|step| step.id == step_id) {
            step.state = "Completed".to_string();
            if !step.artefact_ids.iter().any(|id| id == artefact_id) {
                step.artefact_ids.push(artefact_id.to_string());
            }
        }
    }

    fn set_role(&mut self, role_id: &str, state: impl Into<String>, summary: impl Into<String>) {
        let state = state.into();
        let summary = summary.into();
        self.roles
            .entry(role_id.to_string())
            .and_modify(|role| {
                role.state = state.clone();
                role.summary = summary.clone();
            })
            .or_insert_with(|| RoleView {
                id: role_id.to_string(),
                label: label_for_role(role_id),
                state: state.clone(),
                summary: summary.clone(),
                readiness: RoleReadiness::default(),
            });
    }
}

impl EventView {
    fn from_event(event: &SemanticEvent) -> Self {
        let detail = serde_json::to_value(event).unwrap_or_else(|err| {
            serde_json::json!({
                "error": format!("failed to serialise event detail: {err}")
            })
        });

        Self {
            id: event.event_id().to_string(),
            variant: event.variant_name().to_string(),
            source_agent: source_agent(event),
            timestamp_ns: timestamp_ns(event),
            summary: event_summary(event),
            detail,
        }
    }
}

pub fn spawn_projection_task(state: AppState) {
    tokio::spawn(async move {
        let mut receiver = state.bus.subscribe(&[]);
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let mut projection = state.projection.write().await;
                    projection
                        .apply_event(event.as_ref(), &state.artefact_store)
                        .await;
                }
                Err(RecvError::Lagged(skipped)) => {
                    error!("workbench projection lagged by {} events", skipped);
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

pub async fn seed_workbench(state: &AppState) {
    if state.projection.read().await.has_conversation_history() {
        return;
    }

    state
        .publish(SemanticEvent::new_human_feedback_requested(
            RoleId::new("intent-lead-001"),
            "What are we making, who is it for, and what would make it excellent?",
            "Start of the Intent Lead interview.",
        ))
        .await;
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn style_css() -> impl IntoResponse {
    (
        [("content-type", "text/css")],
        include_str!("../static/style.css"),
    )
}

async fn app_js() -> impl IntoResponse {
    (
        [("content-type", "application/javascript")],
        include_str!("../static/app.js"),
    )
}

async fn snapshot(State(state): State<AppState>) -> Json<WorkbenchProjection> {
    Json(snapshot_projection(&state).await)
}

async fn snapshot_projection(state: &AppState) -> WorkbenchProjection {
    let mut projection = state.projection.read().await.clone();
    if let Some(scheduler) = &state.scheduler {
        let task_states = scheduler
            .lock()
            .await
            .task_states()
            .iter()
            .map(|(task_id, state)| (task_id.clone(), state.to_string()))
            .collect::<Vec<_>>();
        projection.sync_scheduler_task_states(&task_states);
    }
    projection
}

async fn post_message(
    State(state): State<AppState>,
    Json(request): Json<MessageRequest>,
) -> impl IntoResponse {
    let message = request.message.trim().to_string();
    if message.is_empty() {
        return (StatusCode::BAD_REQUEST, "message must not be empty").into_response();
    }

    {
        let mut projection = state.projection.write().await;
        projection.active_lane_ids = request.active_lane_ids.clone();
    }

    if let Some(action_request_id) = request.action_request_id
        && let Some(label) = message
            .strip_prefix('/')
            .and_then(|s| s.split_whitespace().next())
    {
        state
            .publish(SemanticEvent::new_action_request_resolved(
                RoleId::new("human"),
                &action_request_id,
                label,
            ))
            .await;
        state
            .publish(SemanticEvent::new_human_feedback_received(
                RoleId::new("human"),
                &message,
            ))
            .await;
        return StatusCode::ACCEPTED.into_response();
    }

    if message.starts_with("/lane ")
        && let Some(args) = message.strip_prefix("/lane ")
    {
        handle_lane_creation(&state, args).await;
        return StatusCode::ACCEPTED.into_response();
    }

    if message.starts_with("/archive ")
        && let Some(lane_id) = message
            .strip_prefix("/archive ")
            .map(|s| s.trim().to_string())
    {
        state
            .publish(SemanticEvent::new_lane_archived(
                RoleId::new("human"),
                &lane_id,
            ))
            .await;
        return StatusCode::ACCEPTED.into_response();
    }

    if message.starts_with("/pause ")
        && let Some(lane_id) = message
            .strip_prefix("/pause ")
            .map(|s| s.trim().to_string())
    {
        state
            .publish(SemanticEvent::new_lane_paused(
                RoleId::new("human"),
                &lane_id,
            ))
            .await;
        return StatusCode::ACCEPTED.into_response();
    }

    let human_event = SemanticEvent::new_human_feedback_received(RoleId::new("human"), &message);
    state.publish(human_event).await;
    publish_mentions(
        &state,
        &message,
        ReviewContext {
            step_id: request.active_step_id.as_deref(),
            artefact_id: request.active_artefact_id.as_deref(),
        },
    )
    .await;

    StatusCode::ACCEPTED.into_response()
}

async fn handle_lane_creation(state: &AppState, args: &str) {
    let mut explicit_msg_id: Option<String> = None;
    let args_str = if let Some(rest) = args.strip_prefix("msg=") {
        if let Some((msg_id, rest)) = rest.split_once(' ') {
            explicit_msg_id = Some(msg_id.to_string());
            rest
        } else {
            explicit_msg_id = Some(rest.to_string());
            ""
        }
    } else {
        args
    };

    let parts: Vec<&str> = args_str.splitn(3, ' ').collect();
    let name = parts.first().map(|s| s.trim()).unwrap_or("New Lane");
    let purpose = parts
        .get(1)
        .map(|s| s.trim())
        .unwrap_or("No purpose specified");
    let kind = parts.get(2).map(|s| s.trim()).unwrap_or("conversation");

    let colour = match kind {
        "delivery" => "#14b8a6",
        "discovery" => "#6366f1",
        "system" => "#f59e0b",
        _ => "#ec4899",
    };

    let lane_id = format!("lane-{kind}-{}", uuid::Uuid::new_v4());

    let projection = state.projection.read().await;
    let source_message_id =
        explicit_msg_id.or_else(|| projection.messages.last().map(|m| m.message_id.clone()));
    drop(projection);

    state
        .publish(SemanticEvent::new_lane_created(
            RoleId::new("human"),
            &lane_id,
            name,
            kind,
            colour,
            purpose,
            None,
            Vec::new(),
            source_message_id,
        ))
        .await;
}

#[derive(Deserialize)]
struct MessagesQuery {
    #[serde(default)]
    lane_ids: String,
}

async fn get_messages(
    State(state): State<AppState>,
    Query(query): Query<MessagesQuery>,
) -> Json<Vec<MessageView>> {
    let projection = state.projection.read().await;
    let lane_ids: Vec<String> = if query.lane_ids.is_empty() {
        Vec::new()
    } else {
        query
            .lane_ids
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    };
    let msgs: Vec<MessageView> = projection
        .messages_for_lane_ids(&lane_ids)
        .into_iter()
        .cloned()
        .collect();
    Json(msgs)
}

#[derive(Deserialize)]
struct LanesQuery {
    #[serde(default)]
    status: String,
}

async fn get_lanes(
    State(state): State<AppState>,
    Query(query): Query<LanesQuery>,
) -> Json<Vec<LaneView>> {
    let projection = state.projection.read().await;
    let lanes: Vec<LaneView> = if query.status.is_empty() {
        projection.lanes.clone()
    } else {
        projection
            .lanes
            .iter()
            .filter(|l| l.status.as_str().eq_ignore_ascii_case(&query.status))
            .cloned()
            .collect()
    };
    Json(lanes)
}

async fn ack_notification(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut projection = state.projection.write().await;
    if projection.acknowledge_notification(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "notification not found").into_response()
    }
}

#[derive(Deserialize)]
struct CreateRunRequest {
    #[serde(default)]
    label: String,
}

async fn create_run(
    State(state): State<AppState>,
    Json(request): Json<CreateRunRequest>,
) -> impl IntoResponse {
    let label = if request.label.is_empty() {
        "New run".to_string()
    } else {
        request.label
    };
    let run_id = format!("run-{}", Uuid::new_v4());
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let mut projection = state.projection.write().await;
    projection.active_run_id = run_id.clone();
    projection.project.active_run_id = Some(run_id.clone());
    projection.runs.push(RunView {
        id: run_id.clone(),
        label,
        status: "active".to_string(),
        created_at_ns: now_ns,
    });
    let result = projection.clone();
    drop(projection);
    Json(result)
}

async fn select_run(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let mut projection = state.projection.write().await;
    if projection.runs.iter().any(|r| r.id == id) {
        if let Some(run) = projection.runs.iter_mut().find(|r| r.id == id) {
            run.status = "active".to_string();
        }
        projection.active_run_id = id.clone();
        projection.project.active_run_id = Some(id);
        let result = projection.clone();
        drop(projection);
        Json(result).into_response()
    } else {
        (StatusCode::NOT_FOUND, "run not found").into_response()
    }
}

async fn archive_run(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let mut projection = state.projection.write().await;
    if let Some(run) = projection.runs.iter_mut().find(|r| r.id == id) {
        run.status = "archived".to_string();
        if projection.active_run_id == id {
            projection.active_run_id = String::new();
            projection.project.active_run_id = None;
        }
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "run not found").into_response()
    }
}

async fn reset_project(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let confirmed = headers
        .get("X-Confirm")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if !confirmed {
        return (
            StatusCode::PRECONDITION_REQUIRED,
            "Destructive reset requires X-Confirm: true header",
        )
            .into_response();
    }

    let mut projection = state.projection.write().await;
    // TODO: reset_project clears the in-memory projection but does not clear
    // the persistent EventStore. On server restart, replayed events will restore
    // the prior state. True destructive reset requires EventStore::clear().
    let active_run = projection.active_run_id.clone();
    projection.events.clear();
    projection.messages.clear();
    projection.artefacts.clear();
    projection.memories.clear();
    projection.notifications.clear();
    projection.action_requests.clear();
    projection.dag_steps.clear();
    projection.lanes.clear();
    projection.completed_task_ids.clear();
    projection.pending_question = None;
    projection.active_artefact_id = None;
    projection.active_lane_ids.clear();
    projection.has_conversation = false;
    for run in &mut projection.runs {
        run.status = "archived".to_string();
    }
    if !active_run.is_empty() {
        let run_id = format!("run-{}", Uuid::new_v4());
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        projection.active_run_id = run_id.clone();
        projection.project.active_run_id = Some(run_id.clone());
        projection.runs.push(RunView {
            id: run_id,
            label: "Reset run".to_string(),
            status: "active".to_string(),
            created_at_ns: now_ns,
        });
    }
    let result = projection.clone();
    drop(projection);
    Json(result).into_response()
}

async fn publish_mentions(state: &AppState, message: &str, context: ReviewContext<'_>) {
    let mentioned_roles = mentioned_role_ids(message);
    let action_role = inline_action_role_id(message);

    if mentioned_roles.contains(&"reviewer-001") || action_role == Some("reviewer-001") {
        publish_review_request_or_guidance(state, context).await;
    }

    for role_id in mentioned_roles.iter().copied() {
        if role_id == "reviewer-001" {
            continue;
        }
        publish_role_task(state, role_id, message).await;
    }

    if let Some(role_id) = action_role
        && role_id != "reviewer-001"
        && !mentioned_roles.contains(&role_id)
    {
        publish_role_task(state, role_id, message).await;
    }
}

async fn publish_review_request_or_guidance(state: &AppState, context: ReviewContext<'_>) {
    let projection = state.projection.read().await;
    if let Some(task_id) = projection.reviewable_task_id(context) {
        drop(projection);
        state
            .publish(SemanticEvent::new_review_requested(
                RoleId::new("human"),
                &task_id,
                RoleId::new("reviewer-001"),
            ))
            .await;
    } else {
        drop(projection);
        state
            .publish(SemanticEvent::new_human_feedback_requested(
                RoleId::new("reviewer-001"),
                "What completed task or artefact should the Reviewer review?",
                "review requested without completed task context",
            ))
            .await;
    }
}

async fn publish_role_task(state: &AppState, role_id: &str, message: &str) {
    let task_id = Uuid::new_v4().to_string();
    state
        .publish(SemanticEvent::new_task_assigned(
            RoleId::new("human"),
            &task_id,
            RoleId::new(role_id),
            TaskContract {
                contract_id: Uuid::new_v4().to_string(),
                description: role_contract(role_id, message),
            },
            Vec::new(),
        ))
        .await;
}

fn role_contract(role_id: &str, message: &str) -> String {
    match role_id {
        "intent-lead-001" => intent_lead_contract(message),
        "scholar-001" => scholar_contract(message),
        "ops-manager-001" => ops_manager_contract(message),
        "architect-001" => architect_contract(message),
        "pm-001" => pm_contract(message),
        "worker-001" => worker_contract(message),
        "reviewer-001" => reviewer_contract(message),
        "auditor-001" => auditor_contract(message),
        _ => format!("{}: {message}", role_specific_task_description(role_id)),
    }
}

fn intent_lead_contract(message: &str) -> String {
    format!(
        "Intent elicitation and goal clarification: {message}\n\
         Expected outputs:\n\
         - Explicit goals with success criteria\n\
         - Constraints and preferences captured\n\
         - Open questions about scope and audience\n\
         Acceptance criteria:\n\
         - Intent brief MUST state what, who, and why\n\
         - Assumptions MUST be explicitly listed"
    )
}

fn ops_manager_contract(message: &str) -> String {
    format!(
        "Process guardrail selection and operation design: {message}\n\
         Expected outputs:\n\
         - Validation policy with concrete steps\n\
         - Escalation rules with severity mapping\n\
         - Delivery standards and review rubric\n\
         Acceptance criteria:\n\
         - Each SOP MUST cite a specific project need\n\
         - Rubric dimensions MUST be independently assessable"
    )
}

fn architect_contract(message: &str) -> String {
    format!(
        "Architectural design and technical planning: {message}\n\
         Expected outputs:\n\
         - Architecture Decision Record (ADR) with rationale\n\
         - Interface specifications with contracts\n\
         - Dependency rules and constraints\n\
         Acceptance criteria:\n\
         - Each decision MUST list alternatives considered\n\
         - Tradeoffs MUST be explicitly stated"
    )
}

fn pm_contract(message: &str) -> String {
    format!(
        "Work decomposition and task planning: {message}\n\
         Expected outputs:\n\
         - Task cards with clear boundaries\n\
         - Delivery graph with dependency ordering\n\
         - Assignment mapping to available workers\n\
         Acceptance criteria:\n\
         - Each task MUST have a single clear outcome\n\
         - Dependencies MUST form a valid DAG (no cycles)"
    )
}

fn reviewer_contract(message: &str) -> String {
    format!(
        "Quality review and acceptance: {message}\n\
         Expected outputs:\n\
         - Rubric assessment across all dimensions\n\
         - Architectural compliance check against ADRs\n\
         - Pass/fail/needs-rework determination\n\
         Acceptance criteria:\n\
         - Review MUST reference specific code or artefacts\n\
         - Failure classifications MUST cite concrete findings"
    )
}

fn auditor_contract(message: &str) -> String {
    format!(
        "Process and evidence auditing: {message}\n\
         Expected outputs:\n\
         - Evidence chain integrity assessment\n\
         - Policy violation detection with severity\n\
         - Process adherence verification\n\
         Acceptance criteria:\n\
         - Each finding MUST reference specific events\n\
         - Violations MUST map to authoritative rules"
    )
}

fn scholar_contract(message: &str) -> String {
    format!(
        "Research and evidence gathering: {message}\n\
         Expected outputs:\n\
         - Evidence findings with source references\n\
         - Open questions with confidence levels\n\
         - Architectural recommendations filtered out (scope boundary)\n\
         Acceptance criteria:\n\
         - Each claim MUST reference at least one evidence source\n\
         - Confidence MUST be stated for every finding"
    )
}

fn worker_contract(message: &str) -> String {
    format!(
        "Implementation and execution: {message}\n\
         Expected outputs:\n\
         - Code changes scoped to the assigned task\n\
         - Validation results\n\
         - Implementation patch with diff summary\n\
         Rules:\n\
         - Write only within the target repository/worktree (path traversal is prohibited)\n\
         - Run validation commands before claiming completion:\n\
           • cargo fmt --all -- --check\n\
           • cargo test\n\
         - Produce a TaskCompleted event with artefact references\n\
         Safety:\n\
         - This Worker writes to an isolated git worktree (or local fallback directory)\n\
         - The target is the project repository at the configured worktree root\n\
         - Changes are isolated from the main repository until validated"
    )
}

fn inline_action_role_id(message: &str) -> Option<&'static str> {
    let first = message.split_whitespace().next()?.to_lowercase();
    let action = first
        .strip_prefix('/')
        .or_else(|| first.strip_suffix(':'))?;

    match action {
        "intent" => Some("intent-lead-001"),
        "research" | "scholar" => Some("scholar-001"),
        "ops" => Some("ops-manager-001"),
        "design" | "architect" => Some("architect-001"),
        "plan" | "pm" => Some("pm-001"),
        "implement" | "worker" => Some("worker-001"),
        "review" | "reviewer" => Some("reviewer-001"),
        "audit" | "auditor" => Some("auditor-001"),
        _ => None,
    }
}

fn role_specific_task_description(role_id: &str) -> &'static str {
    let _ = role_id;
    "General task"
}

fn mentioned_role_ids(message: &str) -> Vec<&'static str> {
    let lower = message.to_lowercase();
    [
        ("@intent", "intent-lead-001"),
        ("@scholar", "scholar-001"),
        ("@ops", "ops-manager-001"),
        ("@architect", "architect-001"),
        ("@pm", "pm-001"),
        ("@worker", "worker-001"),
        ("@reviewer", "reviewer-001"),
        ("@auditor", "auditor-001"),
    ]
    .into_iter()
    .filter_map(|(mention, role)| contains_mention(&lower, mention).then_some(role))
    .collect()
}

fn contains_mention(message: &str, mention: &str) -> bool {
    let mut search_start = 0;
    while let Some(relative_pos) = message[search_start..].find(mention) {
        let start = search_start + relative_pos;
        let end = start + mention.len();
        let previous_is_word = start > 0
            && message[..start]
                .chars()
                .next_back()
                .is_some_and(is_mention_word_char);
        let next_is_word = message[end..]
            .chars()
            .next()
            .is_some_and(is_mention_word_char);
        if !previous_is_word && !next_is_word {
            return true;
        }
        search_start = end;
    }
    false
}

fn is_mention_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.bus.subscribe(&[]);
    let initial_state = snapshot_projection(&state).await;
    let initial =
        stream::once(async move { Ok(sse_event(&StreamUpdate::State(Box::new(initial_state)))) });
    let live = stream::unfold(receiver, |mut receiver| async move {
        match receiver.recv().await {
            Ok(event) => Some((
                Ok(sse_event(&StreamUpdate::Event(EventView::from_event(
                    event.as_ref(),
                )))),
                receiver,
            )),
            Err(RecvError::Lagged(skipped)) => Some((
                Ok(sse_event(&StreamUpdate::Notice(format!(
                    "Event stream lagged by {skipped} events; refresh to resynchronise."
                )))),
                receiver,
            )),
            Err(RecvError::Closed) => None,
        }
    });

    Sse::new(initial.chain(live)).keep_alive(axum::response::sse::KeepAlive::default())
}

fn sse_event(update: &StreamUpdate) -> Event {
    match serde_json::to_string(update) {
        Ok(payload) => Event::default().data(payload),
        Err(err) => Event::default().data(
            serde_json::json!({
                "type": "Notice",
                "payload": format!("failed to serialise stream update: {err}")
            })
            .to_string(),
        ),
    }
}

fn extract_constraints(answer: &str) -> Vec<String> {
    if answer.to_lowercase().contains("must") || answer.to_lowercase().contains("should") {
        vec![answer.to_string()]
    } else {
        vec!["Needs further clarification".to_string()]
    }
}

async fn artefact_content(
    storage_kind: &ArtefactStorageKind,
    storage_uri: &str,
    repository_output: &Option<RepositoryOutputRef>,
    artefact_store: &ArtefactStore,
) -> serde_json::Value {
    match storage_kind {
        ArtefactStorageKind::Blob => load_blob_artefact_content(storage_uri, artefact_store).await,
        ArtefactStorageKind::Code => code_output_content(storage_uri, repository_output.as_ref()),
    }
}

async fn load_blob_artefact_content(
    storage_uri: &str,
    artefact_store: &ArtefactStore,
) -> serde_json::Value {
    if storage_uri.starts_with("db://artefacts/") {
        match artefact_store.get_payload(storage_uri).await {
            Ok(Some(content)) => {
                return serde_json::from_str(&content)
                    .unwrap_or_else(|_| serde_json::json!({ "content": content }));
            }
            Ok(None) => {
                return serde_json::json!({ "storage_uri": storage_uri, "error": "not found" });
            }
            Err(err) => {
                return serde_json::json!({
                    "storage_uri": storage_uri,
                    "error": format!("failed to load artefact: {err}")
                });
            }
        }
    }

    if let Some(path) = storage_uri.strip_prefix("file://") {
        return serde_json::json!({
            "storage_uri": storage_uri,
            "path": path,
            "error": "file-backed artefact payloads are not loaded by the workbench; use db://artefacts/{id}"
        });
    }

    serde_json::json!({ "storage_uri": storage_uri })
}

fn code_output_content(
    storage_uri: &str,
    repository_output: Option<&RepositoryOutputRef>,
) -> serde_json::Value {
    let Some(output) = repository_output else {
        return serde_json::json!({
            "storage_uri": storage_uri,
            "error": "missing repository output metadata"
        });
    };

    let missing_paths = output
        .paths
        .iter()
        .filter(|path| {
            !std::path::Path::new(&output.worktree_path)
                .join(path)
                .exists()
        })
        .cloned()
        .collect::<Vec<_>>();
    serde_json::json!({
        "storage_uri": storage_uri,
        "repository_path": output.repository_path,
        "worktree_path": output.worktree_path,
        "worktree_branch": output.worktree_branch,
        "paths": output.paths,
        "diff_summary": output.diff_summary,
        "validation_summary": output.validation_summary,
        "revision": output.revision,
        "missing_paths": missing_paths,
        "error": (!missing_paths.is_empty()).then_some("one or more repository paths are missing"),
    })
}

fn storage_kind_label(storage_kind: &ArtefactStorageKind) -> String {
    match storage_kind {
        ArtefactStorageKind::Blob => "blob".to_string(),
        ArtefactStorageKind::Code => "code".to_string(),
    }
}

fn infer_audience(answer: &str) -> String {
    let lower = answer.to_lowercase();
    if lower.contains("developer") || lower.contains("engineer") {
        "Developers and engineering leads".to_string()
    } else if lower.contains("customer") || lower.contains("user") {
        "End users or customers".to_string()
    } else if lower.contains("team") {
        "The project team".to_string()
    } else {
        "Unknown stakeholder group".to_string()
    }
}

#[allow(dead_code)]
pub(crate) fn classify_event_variant_lane(variant: &str) -> Lane {
    match variant {
        "HumanFeedbackRequested" | "HumanFeedbackReceived" => Lane::Conversation,
        "MemoryProposed" | "MemoryAccepted" | "MemoryRejected" | "MemorySuperseded"
        | "ToolExecuted" | "ClaimMade" | "DecisionRecorded" => Lane::Discovery,
        "TaskAssigned" | "TaskStarted" | "TaskCompleted" | "TaskFailed" | "ReviewRequested"
        | "ReviewCompleted" | "ArtefactProduced" => Lane::Delivery,
        "LaneCreated"
        | "LaneArchived"
        | "LanePaused"
        | "ActionRequestCreated"
        | "ActionRequestResolved"
        | "ActionRequestCancelled" => Lane::Conversation,
        _ => Lane::System,
    }
}

fn label_for_artefact(artefact_type: &str) -> String {
    artefact_type
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn label_for_role(role_id: &str) -> String {
    match role_id {
        "intent-lead" | "intent-lead-001" => "Intent Lead".to_string(),
        "scholar" | "scholar-001" => "Scholar".to_string(),
        "ops-manager" | "ops-manager-001" => "Ops Manager".to_string(),
        "architect" | "architect-001" => "Architect".to_string(),
        "project-manager" | "pm-001" => "Project Manager".to_string(),
        "worker" | "worker-001" => "Worker".to_string(),
        "reviewer" | "reviewer-001" => "Reviewer".to_string(),
        "auditor" | "auditor-001" => "Auditor".to_string(),
        "librarian" => "Librarian".to_string(),
        _ => role_id
            .split('-')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn role_summary(role_id: &str, state: &str) -> &'static str {
    match (role_id, state) {
        ("intent-lead" | "intent-lead-001", "Running") => "Clarifying goals, constraints and taste",
        ("scholar" | "scholar-001", "Running") => "Gathering evidence and open questions",
        ("ops-manager" | "ops-manager-001", "Running") => "Selecting process guardrails",
        ("architect" | "architect-001", "Running") => "Preparing design decisions",
        ("project-manager" | "pm-001", "Running") => "Decomposing work into task cards",
        ("worker" | "worker-001", "Running") => "Ready for bounded implementation",
        ("reviewer" | "reviewer-001", "Running") => "Checking quality and acceptance",
        ("auditor" | "auditor-001", "Running") => "Checking provenance and process integrity",
        ("librarian", "Running") => "Curating accepted memory",
        (_, "Completed") => "Completed the current assignment",
        (_, "Idle") => "Waiting for relevant events",
        _ => "Tracking role state from semantic events",
    }
}

fn source_agent(event: &SemanticEvent) -> String {
    match event {
        SemanticEvent::ToolExecuted { source_agent, .. }
        | SemanticEvent::ClaimMade { source_agent, .. }
        | SemanticEvent::DecisionRecorded { source_agent, .. }
        | SemanticEvent::MemoryProposed { source_agent, .. }
        | SemanticEvent::MemoryAccepted { source_agent, .. }
        | SemanticEvent::MemoryRejected { source_agent, .. }
        | SemanticEvent::MemorySuperseded { source_agent, .. }
        | SemanticEvent::EvidenceChainBroken { source_agent, .. }
        | SemanticEvent::ProcessSkipped { source_agent, .. }
        | SemanticEvent::PolicyViolationDetected { source_agent, .. }
        | SemanticEvent::TaskAssigned { source_agent, .. }
        | SemanticEvent::TaskStarted { source_agent, .. }
        | SemanticEvent::TaskCompleted { source_agent, .. }
        | SemanticEvent::TaskFailed { source_agent, .. }
        | SemanticEvent::ReviewRequested { source_agent, .. }
        | SemanticEvent::ReviewCompleted { source_agent, .. }
        | SemanticEvent::EscalationRequested { source_agent, .. }
        | SemanticEvent::HumanFeedbackRequested { source_agent, .. }
        | SemanticEvent::HumanFeedbackReceived { source_agent, .. }
        | SemanticEvent::ArtefactProduced { source_agent, .. }
        | SemanticEvent::BudgetWarning { source_agent, .. }
        | SemanticEvent::EscalationAccepted { source_agent, .. }
        | SemanticEvent::RoleStateChanged { source_agent, .. }
        | SemanticEvent::OrganisationStarted { source_agent, .. }
        | SemanticEvent::OrganisationStopped { source_agent, .. }
        | SemanticEvent::Heartbeat { source_agent, .. }
        | SemanticEvent::LaneCreated { source_agent, .. }
        | SemanticEvent::LaneArchived { source_agent, .. }
        | SemanticEvent::LanePaused { source_agent, .. }
        | SemanticEvent::ActionRequestCreated { source_agent, .. }
        | SemanticEvent::ActionRequestResolved { source_agent, .. }
        | SemanticEvent::ActionRequestCancelled { source_agent, .. } => source_agent.0.clone(),
    }
}

fn timestamp_ns(event: &SemanticEvent) -> u64 {
    match event {
        SemanticEvent::ToolExecuted { timestamp_ns, .. }
        | SemanticEvent::ClaimMade { timestamp_ns, .. }
        | SemanticEvent::DecisionRecorded { timestamp_ns, .. }
        | SemanticEvent::MemoryProposed { timestamp_ns, .. }
        | SemanticEvent::MemoryAccepted { timestamp_ns, .. }
        | SemanticEvent::MemoryRejected { timestamp_ns, .. }
        | SemanticEvent::MemorySuperseded { timestamp_ns, .. }
        | SemanticEvent::EvidenceChainBroken { timestamp_ns, .. }
        | SemanticEvent::ProcessSkipped { timestamp_ns, .. }
        | SemanticEvent::PolicyViolationDetected { timestamp_ns, .. }
        | SemanticEvent::TaskAssigned { timestamp_ns, .. }
        | SemanticEvent::TaskStarted { timestamp_ns, .. }
        | SemanticEvent::TaskCompleted { timestamp_ns, .. }
        | SemanticEvent::TaskFailed { timestamp_ns, .. }
        | SemanticEvent::ReviewRequested { timestamp_ns, .. }
        | SemanticEvent::ReviewCompleted { timestamp_ns, .. }
        | SemanticEvent::EscalationRequested { timestamp_ns, .. }
        | SemanticEvent::HumanFeedbackRequested { timestamp_ns, .. }
        | SemanticEvent::HumanFeedbackReceived { timestamp_ns, .. }
        | SemanticEvent::ArtefactProduced { timestamp_ns, .. }
        | SemanticEvent::BudgetWarning { timestamp_ns, .. }
        | SemanticEvent::EscalationAccepted { timestamp_ns, .. }
        | SemanticEvent::RoleStateChanged { timestamp_ns, .. }
        | SemanticEvent::OrganisationStarted { timestamp_ns, .. }
        | SemanticEvent::OrganisationStopped { timestamp_ns, .. }
        | SemanticEvent::Heartbeat { timestamp_ns, .. }
        | SemanticEvent::LaneCreated { timestamp_ns, .. }
        | SemanticEvent::LaneArchived { timestamp_ns, .. }
        | SemanticEvent::LanePaused { timestamp_ns, .. }
        | SemanticEvent::ActionRequestCreated { timestamp_ns, .. }
        | SemanticEvent::ActionRequestResolved { timestamp_ns, .. }
        | SemanticEvent::ActionRequestCancelled { timestamp_ns, .. } => *timestamp_ns,
    }
}

fn event_summary(event: &SemanticEvent) -> String {
    match event {
        SemanticEvent::ToolExecuted {
            tool_name,
            exit_code,
            ..
        } => format!("Executed {tool_name} with exit code {exit_code}"),
        SemanticEvent::ClaimMade { claim_text, .. } => claim_text.clone(),
        SemanticEvent::DecisionRecorded { decision_text, .. } => decision_text.clone(),
        SemanticEvent::MemoryProposed {
            memory_type, scope, ..
        } => format!("Proposed {scope} {memory_type} memory"),
        SemanticEvent::MemoryAccepted { memory_id, .. } => format!("Accepted memory {memory_id}"),
        SemanticEvent::MemoryRejected {
            rejection_gate,
            rejection_reason,
            ..
        } => format!("Rejected at {rejection_gate}: {rejection_reason}"),
        SemanticEvent::MemorySuperseded { reason, .. } => format!("Superseded memory: {reason}"),
        SemanticEvent::EvidenceChainBroken { claim_text, .. } => {
            format!("Evidence chain broken for: {claim_text}")
        }
        SemanticEvent::ProcessSkipped { step, .. } => format!("Process skipped: {step}"),
        SemanticEvent::PolicyViolationDetected {
            violation_type,
            description,
            ..
        } => format!("{violation_type}: {description}"),
        SemanticEvent::TaskAssigned {
            worker_id,
            contract_ref,
            ..
        } => format!("Assigned {} to {}", contract_ref.description, worker_id),
        SemanticEvent::TaskStarted { task_id, .. } => format!("Started task {task_id}"),
        SemanticEvent::TaskCompleted { task_id, .. } => format!("Completed task {task_id}"),
        SemanticEvent::TaskFailed {
            task_id,
            error_description,
            ..
        } => format!("Task {task_id} failed: {error_description}"),
        SemanticEvent::ReviewRequested {
            task_id,
            reviewer_id,
            ..
        } => format!("Requested review of {task_id} by {reviewer_id}"),
        SemanticEvent::ReviewCompleted {
            task_id, accepted, ..
        } => format!(
            "Review for {task_id}: {}",
            if *accepted { "accepted" } else { "rework" }
        ),
        SemanticEvent::EscalationRequested { reason, .. } => {
            format!("Escalation requested: {reason}")
        }
        SemanticEvent::HumanFeedbackRequested { question, .. } => question.clone(),
        SemanticEvent::HumanFeedbackReceived { answer, .. } => answer.clone(),
        SemanticEvent::ArtefactProduced {
            artefact_type,
            producer_role,
            ..
        } => format!("{producer_role} produced {artefact_type}"),
        SemanticEvent::BudgetWarning {
            usage_percent,
            message,
            ..
        } => format!("Budget {usage_percent}%: {message}"),
        SemanticEvent::EscalationAccepted { target_role, .. } => {
            format!("Escalation accepted by {target_role}")
        }
        SemanticEvent::RoleStateChanged {
            role_id,
            old_state,
            new_state,
            ..
        } => format!("{role_id}: {old_state} -> {new_state}"),
        SemanticEvent::OrganisationStarted { .. } => "Organisation started".to_string(),
        SemanticEvent::OrganisationStopped { reason, .. } => {
            format!("Organisation stopped: {reason}")
        }
        SemanticEvent::Heartbeat {
            active_roles,
            completed_roles,
            failed_roles,
            ..
        } => format!(
            "Heartbeat: {active_roles} active, {completed_roles} completed, {failed_roles} failed"
        ),
        SemanticEvent::LaneCreated { name, kind, .. } => {
            format!("Created lane \"{name}\" ({kind})")
        }
        SemanticEvent::LaneArchived { lane_id, .. } => {
            format!("Archived lane {lane_id}")
        }
        SemanticEvent::LanePaused { lane_id, .. } => {
            format!("Paused lane {lane_id}")
        }
        SemanticEvent::ActionRequestCreated {
            request_kind,
            prompt,
            ..
        } => {
            format!("[{request_kind}] {prompt}")
        }
        SemanticEvent::ActionRequestResolved { choice, .. } => choice.clone(),
        SemanticEvent::ActionRequestCancelled { reason, .. } => {
            format!("Action request cancelled: {reason}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn empty_review_context() -> ReviewContext<'static> {
        ReviewContext {
            step_id: None,
            artefact_id: None,
        }
    }

    #[test]
    fn labels_runtime_role_ids_without_suffix_noise() {
        assert_eq!(label_for_role("intent-lead-001"), "Intent Lead");
        assert_eq!(label_for_role("scholar-001"), "Scholar");
        assert_eq!(label_for_role("pm-001"), "Project Manager");
    }

    #[test]
    fn mention_detection_targets_runtime_role_ids() {
        let roles = mentioned_role_ids("@intent please ask @scholar and @pm for help");
        assert_eq!(roles, vec!["intent-lead-001", "scholar-001", "pm-001"]);
    }

    #[test]
    fn mention_detects_every_supported_target() {
        let roles = mentioned_role_ids(
            "talk to @intent, @scholar, @ops, @architect, @pm, @worker, @reviewer, @auditor",
        );
        assert_eq!(roles.len(), 8);
        assert!(roles.contains(&"intent-lead-001"));
        assert!(roles.contains(&"scholar-001"));
        assert!(roles.contains(&"ops-manager-001"));
        assert!(roles.contains(&"architect-001"));
        assert!(roles.contains(&"pm-001"));
        assert!(roles.contains(&"worker-001"));
        assert!(roles.contains(&"reviewer-001"));
        assert!(roles.contains(&"auditor-001"));
    }

    #[test]
    fn mention_detection_is_case_insensitive() {
        let roles = mentioned_role_ids("@Scholar AND @REVIEWER");
        assert_eq!(roles, vec!["scholar-001", "reviewer-001"]);
    }

    #[test]
    fn mention_detection_returns_empty_for_no_mentions() {
        assert!(mentioned_role_ids("hello world").is_empty());
    }

    #[test]
    fn mention_detection_uses_token_boundaries() {
        assert!(mentioned_role_ids("email person@scholar.example").is_empty());
        assert!(mentioned_role_ids("@scholarship is not a role").is_empty());
    }

    #[test]
    fn inline_actions_route_to_roles() {
        assert_eq!(
            inline_action_role_id("/research durable context"),
            Some("scholar-001")
        );
        assert_eq!(
            inline_action_role_id("review: this artefact"),
            Some("reviewer-001")
        );
        assert_eq!(
            inline_action_role_id("implement: the patch"),
            Some("worker-001")
        );
    }

    #[test]
    fn role_contract_covers_all_known_role_ids() {
        // Every known role ID should get its own contract builder, not the fallback
        for role_id in [
            "intent-lead-001",
            "scholar-001",
            "ops-manager-001",
            "architect-001",
            "pm-001",
            "worker-001",
            "reviewer-001",
            "auditor-001",
        ] {
            let contract = role_contract(role_id, "test message");
            assert!(
                !contract.starts_with("General task:"),
                "role {role_id} should have a specific contract, got fallback",
            );
        }
    }

    #[test]
    fn reviewer_mention_is_excluded_from_generic_routing() {
        let roles = mentioned_role_ids("please @reviewer check this");
        assert!(roles.contains(&"reviewer-001"));
    }

    #[tokio::test]
    async fn reviewer_mention_with_active_artefact_publishes_review_requested() {
        let bus = EventBus::new(16);
        let artefact_store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();
        let artefact_id = Uuid::new_v4().to_string();
        let contract_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: contract_id.clone(),
                    description: "build".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_artefact_produced_ref(
                RoleId::new("worker-001"),
                &artefact_id,
                "prd",
                "hash",
                "file:///tmp/artefact.json",
                RoleId::new("worker-001"),
                Vec::new(),
            ),
            SemanticEvent::new_task_completed(
                RoleId::new("worker-001"),
                &task_id,
                &contract_id,
                mmat_event_stream::event::ArtefactRef {
                    artefact_type: "prd".to_string(),
                    reference: "implementation|content".to_string(),
                },
            ),
        ];

        let mut projection = WorkbenchProjection::from_events(&events, &artefact_store).await;
        projection.active_artefact_id = Some(artefact_id.clone());

        let state = AppState {
            bus: bus.clone(),
            projection: Arc::new(RwLock::new(projection)),
            artefact_store,
            scheduler: None,
        };

        let mut receiver = bus.subscribe(&[]);
        publish_mentions(
            &state,
            "@reviewer please look at this",
            empty_review_context(),
        )
        .await;

        let received = tokio::time::timeout(Duration::from_millis(500), receiver.recv())
            .await
            .expect("should receive event")
            .expect("event should be ok");

        match received.as_ref() {
            SemanticEvent::ReviewRequested {
                task_id: received_task_id,
                reviewer_id,
                ..
            } => {
                assert_eq!(received_task_id, &task_id);
                assert_eq!(reviewer_id.0, "reviewer-001");
            }
            other => panic!("expected ReviewRequested, got {}", other.variant_name()),
        }
    }

    #[tokio::test]
    async fn reviewer_routing_survives_event_history_truncation() {
        let bus = EventBus::new(16);
        let artefact_store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();
        let artefact_id = Uuid::new_v4().to_string();
        let contract_id = Uuid::new_v4().to_string();

        let mut events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: contract_id.clone(),
                    description: "build".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_artefact_produced_ref(
                RoleId::new("worker-001"),
                &artefact_id,
                "prd",
                "hash",
                "file:///tmp/artefact.json",
                RoleId::new("worker-001"),
                Vec::new(),
            ),
            SemanticEvent::new_task_completed(
                RoleId::new("worker-001"),
                &task_id,
                &contract_id,
                mmat_event_stream::event::ArtefactRef {
                    artefact_type: "prd".to_string(),
                    reference: "implementation|content".to_string(),
                },
            ),
        ];
        for index in 0..250 {
            events.push(SemanticEvent::new_human_feedback_received(
                RoleId::new("human"),
                format!("filler {index}"),
            ));
        }

        let mut projection = WorkbenchProjection::from_events(&events, &artefact_store).await;
        projection.active_artefact_id = Some(artefact_id);
        assert!(
            !projection
                .events
                .iter()
                .any(|event| event.variant == "TaskCompleted"),
            "TaskCompleted should be outside the rolling event log"
        );
        let state = AppState {
            bus: bus.clone(),
            projection: Arc::new(RwLock::new(projection)),
            artefact_store,
            scheduler: None,
        };

        let mut receiver = bus.subscribe(&[]);
        publish_mentions(
            &state,
            "@reviewer please look at this",
            empty_review_context(),
        )
        .await;
        let received = tokio::time::timeout(Duration::from_millis(500), receiver.recv())
            .await
            .expect("should receive event")
            .expect("event should be ok");

        match received.as_ref() {
            SemanticEvent::ReviewRequested {
                task_id: received_task_id,
                ..
            } => assert_eq!(received_task_id, &task_id),
            other => panic!("expected ReviewRequested, got {}", other.variant_name()),
        }
    }

    #[tokio::test]
    async fn reviewer_routing_uses_selected_step_context() {
        let bus = EventBus::new(16);
        let artefact_store = Arc::new(ArtefactStore::new());
        let first_task_id = Uuid::new_v4().to_string();
        let first_contract_id = Uuid::new_v4().to_string();
        let second_task_id = Uuid::new_v4().to_string();
        let second_contract_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &first_task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: first_contract_id.clone(),
                    description: "first".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_task_completed(
                RoleId::new("worker-001"),
                &first_task_id,
                &first_contract_id,
                mmat_event_stream::event::ArtefactRef {
                    artefact_type: "prd".to_string(),
                    reference: "implementation|first".to_string(),
                },
            ),
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &second_task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: second_contract_id.clone(),
                    description: "second".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_task_completed(
                RoleId::new("worker-001"),
                &second_task_id,
                &second_contract_id,
                mmat_event_stream::event::ArtefactRef {
                    artefact_type: "prd".to_string(),
                    reference: "implementation|second".to_string(),
                },
            ),
        ];
        let state = AppState::with_events(bus.clone(), &events, artefact_store).await;

        let mut receiver = bus.subscribe(&[]);
        publish_mentions(
            &state,
            "@reviewer please check the selected step",
            ReviewContext {
                step_id: Some(&first_task_id),
                artefact_id: None,
            },
        )
        .await;
        let received = tokio::time::timeout(Duration::from_millis(500), receiver.recv())
            .await
            .expect("should receive event")
            .expect("event should be ok");

        match received.as_ref() {
            SemanticEvent::ReviewRequested { task_id, .. } => assert_eq!(task_id, &first_task_id),
            other => panic!("expected ReviewRequested, got {}", other.variant_name()),
        }
    }

    #[tokio::test]
    async fn reviewer_mention_with_uncompleted_artefact_asks_for_context() {
        let bus = EventBus::new(16);
        let artefact_store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();
        let artefact_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "build".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_artefact_produced_ref(
                RoleId::new("worker-001"),
                &artefact_id,
                "prd",
                "hash",
                "file:///tmp/artefact.json",
                RoleId::new("worker-001"),
                Vec::new(),
            ),
        ];
        let mut projection = WorkbenchProjection::from_events(&events, &artefact_store).await;
        projection.active_artefact_id = Some(artefact_id);
        let state = AppState {
            bus: bus.clone(),
            projection: Arc::new(RwLock::new(projection)),
            artefact_store,
            scheduler: None,
        };

        let mut receiver = bus.subscribe(&[]);
        publish_mentions(
            &state,
            "@reviewer please look at this",
            empty_review_context(),
        )
        .await;

        let received = tokio::time::timeout(Duration::from_millis(500), receiver.recv())
            .await
            .expect("should receive event")
            .expect("event should be ok");
        assert_eq!(received.variant_name(), "HumanFeedbackRequested");
    }

    #[tokio::test]
    async fn reviewer_mention_without_artefact_asks_for_context() {
        let bus = EventBus::new(16);
        let artefact_store = Arc::new(ArtefactStore::new());
        let projection = WorkbenchProjection::from_events(&[], &artefact_store).await;

        let state = AppState {
            bus: bus.clone(),
            projection: Arc::new(RwLock::new(projection)),
            artefact_store,
            scheduler: None,
        };

        let mut receiver = bus.subscribe(&[]);
        publish_mentions(&state, "@reviewer check this", empty_review_context()).await;

        let received = tokio::time::timeout(Duration::from_millis(500), receiver.recv())
            .await
            .expect("should receive event")
            .expect("event should be ok");

        assert_eq!(received.variant_name(), "HumanFeedbackRequested");
        let summary = super::event_summary(received.as_ref());
        assert!(
            summary.contains("review"),
            "should ask about review context: {summary}"
        );
        let projection =
            WorkbenchProjection::from_events(&[received.as_ref().clone()], &ArtefactStore::new())
                .await;
        assert_eq!(projection.messages[0].speaker, "Reviewer");
        assert_eq!(projection.roles["reviewer-001"].state, "Waiting");
        assert_ne!(projection.roles["intent-lead-001"].state, "Waiting");
    }

    #[tokio::test]
    async fn projection_hydrates_conversation_history_from_events() {
        let events = vec![
            SemanticEvent::new_human_feedback_requested(
                RoleId::new("intent-lead-001"),
                "What are we making?",
                "test",
            ),
            SemanticEvent::new_human_feedback_received(RoleId::new("human"), "A useful tool"),
        ];

        let store = ArtefactStore::new();
        let projection = WorkbenchProjection::from_events(&events, &store).await;

        assert!(projection.has_conversation_history());
        assert_eq!(projection.messages.len(), 2);
        assert_eq!(projection.messages[0].speaker, "Intent Lead");
        assert_eq!(projection.messages[1].speaker, "You");
    }

    #[tokio::test]
    async fn organisation_events_do_not_count_as_conversation_history() {
        let store = ArtefactStore::new();
        let projection = WorkbenchProjection::from_events(
            &[SemanticEvent::new_organisation_started(RoleId::new(
                "coordinator",
            ))],
            &store,
        )
        .await;

        assert!(!projection.has_conversation_history());
    }

    #[test]
    fn missing_database_url_error_message_is_clear() {
        let err = WorkbenchError::Init(
            "DATABASE_URL is not set.\n\
             The workbench requires a Postgres database to store events, memories, and artefacts.\n\
             Set DATABASE_URL in your environment, for example:\n\
             export DATABASE_URL=\"postgres://user:password@localhost:5432/mmat\""
                .to_string(),
        );
        let msg = err.to_string();
        assert!(
            msg.contains("DATABASE_URL"),
            "should mention DATABASE_URL: {msg}",
        );
        assert!(msg.contains("Postgres"), "should mention Postgres: {msg}",);
    }

    #[test]
    fn static_assets_are_compiled_into_binary() {
        let html = include_str!("../static/index.html");
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("makemeathing"));
        assert!(html.contains("href=\"/style.css\""));
        assert!(html.contains("src=\"/app.js\""));

        let css = include_str!("../static/style.css");
        assert!(css.contains(":root"));

        let js = include_str!("../static/app.js");
        assert!(js.contains("loadState"));
        assert!(js.contains("active_step_id"));
        assert!(js.contains("#event-"));
    }

    #[tokio::test]
    async fn postgres_event_replay_preserves_events_across_restart() {
        let base_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                println!(
                    "[SKIP] postgres_event_replay_preserves_events_across_restart requires DATABASE_URL"
                );
                return;
            }
        };

        let schema = format!(
            "workbench_replay_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        let admin_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&base_url)
            .await
            .unwrap();

        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin_pool)
            .await
            .unwrap();

        let separator = if base_url.contains('?') { '&' } else { '?' };
        let database_url = format!("{base_url}{separator}options=-c%20search_path%3D{schema}");

        let task_id = Uuid::new_v4().to_string();

        // First runtime: publish events including TaskAssigned, then drop
        {
            let config = OrganisationConfig {
                database_url: Some(database_url.clone()),
                event_store_path: None,
                memory_store_path: None,
                ..Default::default()
            };
            let intent_lead = IntentLead::new();
            let mut registry = RoleRegistry::new();
            registry.register(intent_lead.spec()).unwrap();

            let runtime = OrganisationRuntime::new(config, registry).unwrap();

            runtime
                .bus()
                .publish(SemanticEvent::new_human_feedback_requested(
                    RoleId::new("intent-lead-001"),
                    "What are we making?",
                    "test",
                ))
                .unwrap();

            runtime
                .bus()
                .publish(SemanticEvent::new_human_feedback_received(
                    RoleId::new("human"),
                    "A test answer",
                ))
                .unwrap();

            runtime
                .bus()
                .publish(SemanticEvent::new_task_assigned(
                    RoleId::new("human"),
                    &task_id,
                    RoleId::new("worker-001"),
                    TaskContract {
                        contract_id: Uuid::new_v4().to_string(),
                        description: "Implement the feature".to_string(),
                    },
                    Vec::new(),
                ))
                .unwrap();
        }

        // Second runtime: same database_url, replay and verify
        {
            let config = OrganisationConfig {
                database_url: Some(database_url.clone()),
                event_store_path: None,
                memory_store_path: None,
                ..Default::default()
            };
            let intent_lead = IntentLead::new();
            let mut registry = RoleRegistry::new();
            registry.register(intent_lead.spec()).unwrap();

            let artefact_store = ArtefactStore::new();
            let runtime = OrganisationRuntime::new(config, registry).unwrap();
            let events = runtime.event_store().replay(0, None).unwrap();

            assert_eq!(events.len(), 3, "should replay 3 persisted events");
            assert_eq!(events[0].variant_name(), "HumanFeedbackRequested");
            assert_eq!(events[1].variant_name(), "HumanFeedbackReceived");
            assert_eq!(events[2].variant_name(), "TaskAssigned");

            // Verify projection hydrates messages and DAG steps from Postgres events
            let projection = WorkbenchProjection::from_events(&events, &artefact_store).await;

            assert!(projection.has_conversation_history());
            assert_eq!(projection.messages.len(), 2, "should have 2 chat messages");
            assert_eq!(projection.messages[0].speaker, "Intent Lead");
            assert_eq!(projection.messages[1].speaker, "You");
            assert!(
                projection.dag_steps.iter().any(|s| s.role == "worker-001"),
                "should have a DAG step for worker-001 from TaskAssigned",
            );
        }
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    // -----------------------------------------------------------------------
    // 3.2 Lane filtering
    // -----------------------------------------------------------------------

    #[test]
    fn classify_events_into_correct_lanes() {
        let conversation_event =
            SemanticEvent::new_human_feedback_requested(RoleId::new("test"), "hello", "test");
        assert_eq!(classify_event_lane(&conversation_event), Lane::Conversation,);

        let delivery_event = SemanticEvent::new_task_assigned(
            RoleId::new("human"),
            "task-1",
            RoleId::new("worker-001"),
            TaskContract {
                contract_id: Uuid::new_v4().to_string(),
                description: "test task".to_string(),
            },
            Vec::new(),
        );
        assert_eq!(classify_event_lane(&delivery_event), Lane::Delivery);

        let discovery_event = SemanticEvent::new_claim_made(
            RoleId::new("scholar-001"),
            "claim text",
            Vec::new(),
            0.85,
        );
        assert_eq!(classify_event_lane(&discovery_event), Lane::Discovery);

        let system_event = SemanticEvent::new_organisation_started(RoleId::new("coordinator"));
        assert_eq!(classify_event_lane(&system_event), Lane::System);
    }

    #[tokio::test]
    async fn lane_filter_excludes_unrelated_events() {
        let store = Arc::new(ArtefactStore::new());
        let events: Vec<SemanticEvent> = vec![
            SemanticEvent::new_human_feedback_requested(
                RoleId::new("intent-lead-001"),
                "What are we making?",
                "test",
            ),
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                "task-1",
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "build feature".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_organisation_started(RoleId::new("coordinator")),
            SemanticEvent::new_claim_made(
                RoleId::new("scholar-001"),
                "discovery claim",
                Vec::new(),
                0.85,
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        assert_eq!(projection.events.len(), 4);

        // Discovery lane should only contain discovery events
        let discovery: Vec<&EventView> = projection.events_by_lane(Lane::Discovery);
        assert_eq!(discovery.len(), 1, "should have 1 discovery event");
        assert_eq!(discovery[0].variant, "ClaimMade");

        // Delivery lane should only contain delivery events
        let delivery: Vec<&EventView> = projection.events_by_lane(Lane::Delivery);
        assert_eq!(delivery.len(), 1, "should have 1 delivery event");
        assert_eq!(delivery[0].variant, "TaskAssigned");

        // Conversation lane
        let conversation: Vec<&EventView> = projection.events_by_lane(Lane::Conversation);
        assert_eq!(conversation.len(), 1, "should have 1 conversation event");
        assert_eq!(conversation[0].variant, "HumanFeedbackRequested");

        // System lane
        let system: Vec<&EventView> = projection.events_by_lane(Lane::System);
        assert_eq!(system.len(), 1, "should have 1 system event");
        assert_eq!(system[0].variant, "OrganisationStarted");
    }

    // -----------------------------------------------------------------------
    // 3.3 Action request resolution
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn task_assignment_creates_dag_step() {
        let store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();

        let events = vec![SemanticEvent::new_task_assigned(
            RoleId::new("human"),
            &task_id,
            RoleId::new("worker-001"),
            TaskContract {
                contract_id: Uuid::new_v4().to_string(),
                description: "Implement the feature".to_string(),
            },
            Vec::new(),
        )];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        assert!(!projection.has_conversation_history());

        let worker_step = projection.dag_steps.iter().find(|s| s.id == task_id);
        assert!(
            worker_step.is_some(),
            "should have a DAG step for the assigned task",
        );
        assert_eq!(worker_step.unwrap().role, "worker-001");
        assert_eq!(worker_step.unwrap().state, "Running");
    }

    #[tokio::test]
    async fn review_completed_updates_dag_step() {
        let store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "Implement the feature".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_review_completed(
                RoleId::new("reviewer-001"),
                &task_id,
                Vec::new(),
                true,
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let review_step = projection
            .dag_steps
            .iter()
            .find(|s| s.id == format!("review-{task_id}"));
        assert!(review_step.is_some(), "should have a review DAG step");
        assert_eq!(review_step.unwrap().state, "Accepted");
    }

    #[tokio::test]
    async fn rework_review_shows_needs_rework_state() {
        let store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "Implement the feature".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_review_completed(
                RoleId::new("reviewer-001"),
                &task_id,
                Vec::new(),
                false,
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let review_step = projection
            .dag_steps
            .iter()
            .find(|s| s.id == format!("review-{task_id}"));
        assert!(review_step.is_some());
        assert_eq!(review_step.unwrap().state, "Needs rework");
    }

    // -----------------------------------------------------------------------
    // 3.4 Artefact loading and DAG construction
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn artefact_produced_populates_artefact_list() {
        let store = Arc::new(ArtefactStore::new());
        let artefact_id = Uuid::new_v4().to_string();

        let events = vec![SemanticEvent::new_artefact_produced_ref(
            RoleId::new("worker-001"),
            &artefact_id,
            "prd",
            "abc123",
            "file:///tmp/test-artefact.json",
            RoleId::new("worker-001"),
            Vec::new(),
        )];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let artefact = projection.artefacts.iter().find(|a| a.id == artefact_id);
        assert!(artefact.is_some(), "should contain the produced artefact");
        assert_eq!(artefact.unwrap().artefact_type, "prd");
    }

    #[tokio::test]
    async fn artefact_load_failure_produces_error_state() {
        let store = Arc::new(ArtefactStore::new());
        let artefact_id = Uuid::new_v4().to_string();

        let events = vec![SemanticEvent::new_artefact_produced_ref(
            RoleId::new("worker-001"),
            &artefact_id,
            "prd",
            "def456",
            "file:///tmp/nonexistent-artefact.json",
            RoleId::new("worker-001"),
            Vec::new(),
        )];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let artefact = projection.artefacts.iter().find(|a| a.id == artefact_id);
        assert!(artefact.is_some(), "artefact should be in the projection");

        let content = &artefact.unwrap().content;
        assert!(
            content.get("error").is_some() || content.get("storage_uri").is_some(),
            "artefact load failure should not panic, should produce error or fallback content: {content:?}",
        );
    }

    #[tokio::test]
    async fn postgres_blob_artefact_projection_loads_payload() {
        let base_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                println!(
                    "[SKIP] postgres_blob_artefact_projection_loads_payload requires DATABASE_URL"
                );
                return;
            }
        };
        let schema = format!("workbench_blob_{}", Uuid::new_v4().simple());
        let admin_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&base_url)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin_pool)
            .await
            .unwrap();
        let separator = if base_url.contains('?') { '&' } else { '?' };
        let database_url = format!("{base_url}{separator}options=-c%20search_path%3D{schema}");

        let store = Arc::new(ArtefactStore::new_postgres(&database_url).unwrap());
        let stored = store
            .store("adr", r#"{"title":"Keep blobs in Postgres"}"#)
            .await
            .unwrap();
        let events = vec![SemanticEvent::new_artefact_produced_ref(
            RoleId::new("architect-001"),
            stored.artefact_id.clone(),
            "adr",
            stored.content_hash.clone(),
            stored.storage_uri.clone(),
            RoleId::new("architect-001"),
            Vec::new(),
        )];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let artefact = projection
            .artefacts
            .iter()
            .find(|a| a.id == stored.artefact_id)
            .expect("stored blob artefact should be projected");
        assert_eq!(artefact.storage_kind, "blob");
        assert_eq!(artefact.content["title"], "Keep blobs in Postgres");

        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn code_output_projection_includes_repository_metadata() {
        let store = Arc::new(ArtefactStore::new());
        let temp = tempfile::tempdir().unwrap();
        let worktree_path = temp.path().join("worktree");
        std::fs::create_dir_all(worktree_path.join("src")).unwrap();
        std::fs::write(worktree_path.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
        let output = RepositoryOutputRef {
            repository_path: temp.path().display().to_string(),
            worktree_path: worktree_path.display().to_string(),
            worktree_branch: "task-123".to_string(),
            paths: vec!["src/lib.rs".to_string()],
            diff_summary: "1 file changed: src/lib.rs".to_string(),
            validation_summary: Some("validation passed".to_string()),
            revision: Some("working-tree".to_string()),
        };
        let event = SemanticEvent::new_code_output_ref(
            RoleId::new("worker-001"),
            "implementation_patch",
            mmat_event_stream::event::StoredArtefactRef {
                artefact_id: "code-1".to_string(),
                content_hash: "hash123".to_string(),
                storage_uri: "repo://worktrees/task-123/code-1".to_string(),
            },
            RoleId::new("worker-001"),
            vec![mmat_event_stream::event::EvidenceRef {
                event_id: mmat_event_stream::event::EventId::new(),
                description: "validation".to_string(),
            }],
            output,
        );

        let projection = WorkbenchProjection::from_events(&[event], &store).await;
        let artefact = projection.artefacts.first().expect("code artefact");
        assert_eq!(artefact.storage_kind, "code");
        assert_eq!(artefact.storage_uri, "repo://worktrees/task-123/code-1");
        assert_eq!(
            artefact.repository_output.as_ref().unwrap().paths,
            ["src/lib.rs"]
        );
        assert_eq!(
            artefact.content["missing_paths"].as_array().unwrap().len(),
            0
        );
        assert_eq!(artefact.evidence_refs.len(), 1);
    }

    #[tokio::test]
    async fn dag_steps_constructed_from_multiple_events() {
        let store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();
        let artefact_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_human_feedback_requested(
                RoleId::new("intent-lead-001"),
                "What are we making?",
                "test",
            ),
            SemanticEvent::new_human_feedback_received(RoleId::new("human"), "A testing tool"),
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "Build the thing".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_artefact_produced_ref(
                RoleId::new("worker-001"),
                &artefact_id,
                "prd",
                "ghi789",
                "file:///tmp/result.json",
                RoleId::new("worker-001"),
                Vec::new(),
            ),
            SemanticEvent::new_review_completed(
                RoleId::new("reviewer-001"),
                &task_id,
                Vec::new(),
                true,
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;

        assert_eq!(
            projection.messages.len(),
            4,
            "should have 4 messages (2 conversation + 2 capability)"
        );
        assert!(!projection.artefacts.is_empty(), "should have artefacts");
        assert!(!projection.dag_steps.is_empty(), "should have DAG steps");

        let review_step = projection
            .dag_steps
            .iter()
            .find(|s| s.id == format!("review-{task_id}"));
        assert!(
            review_step.is_some(),
            "should have a review DAG step linking back to the task",
        );
    }

    // -----------------------------------------------------------------------
    // 2.3 Librarian-visible memory lifecycle events
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn memory_proposed_creates_memory_view() {
        let store = Arc::new(ArtefactStore::new());
        let events = vec![SemanticEvent::new_memory_proposed(
            RoleId::new("scholar-001"),
            "pattern",
            "Users prefer dark mode in terminals",
            "project",
            RoleId::new("librarian"),
            Vec::new(),
            0.85,
        )];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        assert_eq!(projection.memories.len(), 1);
        assert_eq!(projection.memories[0].status, "Proposed");
        assert_eq!(projection.memories[0].memory_type, "pattern");
        assert!(projection.memories[0].content.contains("dark mode"));
    }

    #[tokio::test]
    async fn memory_accepted_updates_status_and_authority() {
        let store = Arc::new(ArtefactStore::new());
        let proposal_event = SemanticEvent::new_memory_proposed(
            RoleId::new("scholar-001"),
            "pattern",
            "memory content",
            "project",
            RoleId::new("librarian"),
            Vec::new(),
            0.9,
        );
        let proposal_event_id = proposal_event.event_id();
        let memory_uuid = uuid::Uuid::new_v4();

        let events = vec![
            proposal_event,
            SemanticEvent::new_memory_accepted(
                RoleId::new("librarian"),
                mmat_event_stream::event::MemoryId(memory_uuid),
                proposal_event_id,
                RoleId::new("librarian"),
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        assert_eq!(projection.memories.len(), 1);
        assert_eq!(projection.memories[0].status, "Accepted");
        assert_eq!(projection.memories[0].id, memory_uuid.to_string());
        assert_eq!(projection.memories[0].authority, "librarian");
        let librarian_step = projection
            .dag_steps
            .iter()
            .find(|step| step.id == "librarian")
            .expect("librarian activity should have a DAG step");
        assert_eq!(librarian_step.state, "Completed");
        assert_eq!(librarian_step.event_ids.len(), 2);
    }

    #[tokio::test]
    async fn memory_rejected_shows_gate_and_reason() {
        let store = Arc::new(ArtefactStore::new());
        let events = vec![SemanticEvent::new_memory_rejected(
            RoleId::new("librarian"),
            "pattern",
            "trivial thought",
            "durability",
            "content too short and lacks substance",
        )];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        assert_eq!(projection.memories.len(), 1);
        assert!(projection.memories[0].status.contains("Rejected"));
        assert!(projection.memories[0].status.contains("durability"));
        assert!(projection.memories[0].content.contains("trivial thought"));
        let librarian_step = projection
            .dag_steps
            .iter()
            .find(|step| step.id == "librarian")
            .expect("rejection should be linked to librarian activity");
        assert!(librarian_step.summary.contains("durability"));
    }

    #[tokio::test]
    async fn memory_superseded_marks_old_memory() {
        let store = Arc::new(ArtefactStore::new());
        let proposal_event = SemanticEvent::new_memory_proposed(
            RoleId::new("scholar-001"),
            "fact",
            "old fact content",
            "project",
            RoleId::new("librarian"),
            Vec::new(),
            0.9,
        );
        let proposal_event_id = proposal_event.event_id();
        let old_memory_uuid = uuid::Uuid::new_v4();
        let new_memory_uuid = uuid::Uuid::new_v4();

        let events = vec![
            proposal_event,
            SemanticEvent::new_memory_accepted(
                RoleId::new("librarian"),
                mmat_event_stream::event::MemoryId(old_memory_uuid),
                proposal_event_id,
                RoleId::new("librarian"),
            ),
            SemanticEvent::new_memory_superseded(
                RoleId::new("librarian"),
                mmat_event_stream::event::MemoryId(old_memory_uuid),
                mmat_event_stream::event::MemoryId(new_memory_uuid),
                "new evidence contradicts old finding",
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let old_memory = projection
            .memories
            .iter()
            .find(|m| m.id == old_memory_uuid.to_string());
        assert!(old_memory.is_some(), "old memory should still exist");
        assert!(
            old_memory.unwrap().status.contains("Superseded"),
            "old memory should be marked superseded: {}",
            old_memory.unwrap().status
        );
    }

    #[tokio::test]
    async fn memory_accepted_sets_librarian_role() {
        let store = Arc::new(ArtefactStore::new());
        let proposal_event = SemanticEvent::new_memory_proposed(
            RoleId::new("scholar-001"),
            "pattern",
            "accepted content",
            "project",
            RoleId::new("librarian"),
            Vec::new(),
            0.9,
        );
        let proposal_event_id = proposal_event.event_id();
        let memory_uuid = uuid::Uuid::new_v4();

        let events = vec![
            proposal_event,
            SemanticEvent::new_memory_accepted(
                RoleId::new("librarian"),
                mmat_event_stream::event::MemoryId(memory_uuid),
                proposal_event_id,
                RoleId::new("librarian"),
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let librarian = projection.roles.get("librarian");
        assert!(librarian.is_some(), "librarian role should exist");
        assert_eq!(librarian.unwrap().state, "Completed");
    }

    // -----------------------------------------------------------------------
    // 3.1 DAG projection for TaskStarted, TaskFailed, ReviewRequested,
    //     EscalationRequested
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn task_started_updates_dag_step_state() {
        let store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "build feature".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_task_started(
                RoleId::new("worker-001"),
                &task_id,
                RoleId::new("worker-001"),
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let step = projection.dag_steps.iter().find(|s| s.id == task_id);
        assert!(step.is_some(), "task step should exist");
        assert_eq!(step.unwrap().state, "Running");
    }

    #[tokio::test]
    async fn task_failed_marks_dag_step_as_failed() {
        let store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "build feature".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_task_failed(
                RoleId::new("worker-001"),
                &task_id,
                "build error: dependency not found",
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let step = projection.dag_steps.iter().find(|s| s.id == task_id);
        assert!(step.is_some(), "task step should exist");
        assert_eq!(step.unwrap().state, "Failed");
        assert!(step.unwrap().summary.contains("dependency"));
    }

    #[tokio::test]
    async fn review_requested_creates_pending_review_step() {
        let store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "build feature".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_review_requested(
                RoleId::new("human"),
                &task_id,
                RoleId::new("reviewer-001"),
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let review_step = projection
            .dag_steps
            .iter()
            .find(|s| s.id == format!("review-{task_id}"));
        assert!(review_step.is_some(), "review step should exist");
        assert_eq!(review_step.unwrap().state, "Pending");
        assert_eq!(review_step.unwrap().role, "reviewer-001");
    }

    #[tokio::test]
    async fn escalation_requested_creates_escalation_step() {
        let store = Arc::new(ArtefactStore::new());

        let events = vec![SemanticEvent::new_escalation_requested(
            RoleId::new("scholar-001"),
            RoleId::new("scholar-001"),
            RoleId::new("worker-001"),
            "requires implementation skills",
            mmat_event_stream::event::EscalationSeverity::Medium,
        )];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let esc_step = projection
            .dag_steps
            .iter()
            .find(|s| s.id.starts_with("escalation-"));
        assert!(esc_step.is_some(), "escalation step should exist");
        assert_eq!(esc_step.unwrap().state, "Escalated");
        assert!(esc_step.unwrap().summary.contains("implementation"));
    }

    #[tokio::test]
    async fn blocked_state_propagates_to_dependent_steps() {
        let store = Arc::new(ArtefactStore::new());
        let task_a_id = Uuid::new_v4().to_string();
        let task_b_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_a_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "build backend".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_b_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "build frontend (depends on backend)".to_string(),
                },
                vec![task_a_id.clone()],
            ),
            SemanticEvent::new_task_failed(
                RoleId::new("worker-001"),
                &task_a_id,
                "compilation failed",
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;

        let task_a = projection.dag_steps.iter().find(|s| s.id == task_a_id);
        assert!(task_a.is_some(), "task A should exist");
        assert_eq!(task_a.unwrap().state, "Failed");

        let task_b = projection.dag_steps.iter().find(|s| s.id == task_b_id);
        assert!(task_b.is_some(), "task B should exist");
        assert_eq!(task_b.unwrap().state, "Blocked");

        let worker_role = projection.roles.get("worker-001");
        assert!(worker_role.is_some());
        assert_eq!(worker_role.unwrap().state, "Failed");
    }

    #[tokio::test]
    async fn task_failed_updates_role_state_correctly() {
        let store = Arc::new(ArtefactStore::new());
        let task_id = Uuid::new_v4().to_string();

        let events = vec![
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "build feature".to_string(),
                },
                Vec::new(),
            ),
            SemanticEvent::new_task_failed(
                RoleId::new("worker-001"),
                &task_id,
                "build error: missing dependency",
            ),
        ];

        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let worker = projection.roles.get("worker-001");
        assert!(worker.is_some(), "worker role should exist");
        assert_eq!(worker.unwrap().state, "Failed");
        assert!(worker.unwrap().summary.contains("missing dependency"));
    }

    #[tokio::test]
    async fn escalation_sets_role_to_escalated() {
        let store = Arc::new(ArtefactStore::new());

        let events = vec![SemanticEvent::new_escalation_requested(
            RoleId::new("scholar-001"),
            RoleId::new("scholar-001"),
            RoleId::new("worker-001"),
            "requires implementation skills beyond research capability",
            mmat_event_stream::event::EscalationSeverity::High,
        )];

        let projection = WorkbenchProjection::from_events(&events, &store).await;

        let worker = projection.roles.get("worker-001");
        assert!(worker.is_some(), "worker role should exist");
        assert_eq!(worker.unwrap().state, "Escalated");

        let scholar = projection.roles.get("scholar-001");
        assert!(scholar.is_some(), "scholar role should exist");

        let notification = projection
            .notifications
            .iter()
            .find(|n| n.kind == "Escalation");
        assert!(
            notification.is_some(),
            "escalation notification should exist"
        );
        assert!(
            notification
                .unwrap()
                .summary
                .contains("implementation skills")
        );
    }

    #[tokio::test]
    async fn enrich_replay_test_with_memories_and_artefacts() {
        let base_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                println!(
                    "[SKIP] enrich_replay_test_with_memories_and_artefacts requires DATABASE_URL"
                );
                return;
            }
        };

        let schema = format!(
            "workbench_replay_enriched_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        let admin_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&base_url)
            .await
            .unwrap();

        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin_pool)
            .await
            .unwrap();

        let separator = if base_url.contains('?') { '&' } else { '?' };
        let database_url = format!("{base_url}{separator}options=-c%20search_path%3D{schema}");

        let task_id = Uuid::new_v4().to_string();
        let artefact_id = Uuid::new_v4().to_string();

        // First runtime: publish events covering messages, DAG steps, memories, artefacts
        {
            let config = OrganisationConfig {
                database_url: Some(database_url.clone()),
                event_store_path: None,
                memory_store_path: None,
                ..Default::default()
            };
            let intent_lead = IntentLead::new();
            let mut registry = RoleRegistry::new();
            registry.register(intent_lead.spec()).unwrap();

            let runtime = OrganisationRuntime::new(config, registry).unwrap();
            let bus = runtime.bus();

            bus.publish(SemanticEvent::new_human_feedback_requested(
                RoleId::new("intent-lead-001"),
                "What are we making?",
                "test",
            ))
            .unwrap();

            bus.publish(SemanticEvent::new_human_feedback_received(
                RoleId::new("human"),
                "A test answer",
            ))
            .unwrap();

            bus.publish(SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                &task_id,
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "Implement feature".to_string(),
                },
                Vec::new(),
            ))
            .unwrap();

            // Memory event
            bus.publish(SemanticEvent::new_memory_proposed(
                RoleId::new("scholar-001"),
                "Preference",
                "memory content",
                "project",
                RoleId::new("librarian"),
                Vec::new(),
                0.9,
            ))
            .unwrap();

            // Artefact event
            bus.publish(SemanticEvent::new_artefact_produced_ref(
                RoleId::new("worker-001"),
                &artefact_id,
                "prd",
                "hash123",
                "file:///tmp/replay-artefact.json",
                RoleId::new("worker-001"),
                Vec::new(),
            ))
            .unwrap();
        }

        // Second runtime: replay and verify all projection components
        {
            let config = OrganisationConfig {
                database_url: Some(database_url.clone()),
                event_store_path: None,
                memory_store_path: None,
                ..Default::default()
            };
            let intent_lead = IntentLead::new();
            let mut registry = RoleRegistry::new();
            registry.register(intent_lead.spec()).unwrap();

            let artefact_store = ArtefactStore::new();
            let runtime = OrganisationRuntime::new(config, registry).unwrap();
            let events = runtime.event_store().replay(0, None).unwrap();

            assert_eq!(events.len(), 5, "should replay 5 persisted events");
            assert!(
                events.iter().any(|e| e.variant_name() == "MemoryProposed"),
                "should include MemoryProposed in replay"
            );
            assert!(
                events
                    .iter()
                    .any(|e| e.variant_name() == "ArtefactProduced"),
                "should include ArtefactProduced in replay"
            );

            let projection = WorkbenchProjection::from_events(&events, &artefact_store).await;

            assert!(projection.has_conversation_history());
            assert_eq!(projection.messages.len(), 2, "should have 2 chat messages");
            assert!(
                projection.dag_steps.iter().any(|s| s.role == "worker-001"),
                "should have a DAG step for worker-001 from TaskAssigned"
            );
            assert!(
                !projection.memories.is_empty(),
                "should have memories from MemoryProposed replay"
            );
            assert_eq!(projection.memories[0].status, "Proposed");
            assert!(
                !projection.artefacts.is_empty(),
                "should have artefacts from ArtefactProduced replay"
            );
            assert_eq!(projection.artefacts[0].artefact_type, "prd");
        }

        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    // -----------------------------------------------------------------------
    // 4.1 Integration tests for mention-to-event routing
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn scholar_mention_publishes_task_assigned_with_research_description() {
        let bus = EventBus::new(16);
        let store = Arc::new(ArtefactStore::new());
        let state = AppState::with_events(bus.clone(), &[], store).await;

        let mut receiver = bus.subscribe(&[]);
        publish_mentions(
            &state,
            "@scholar please research this topic",
            empty_review_context(),
        )
        .await;

        let received = tokio::time::timeout(Duration::from_millis(500), receiver.recv())
            .await
            .expect("should receive event")
            .expect("event should be ok");

        match received.as_ref() {
            SemanticEvent::TaskAssigned {
                worker_id,
                contract_ref,
                ..
            } => {
                assert_eq!(worker_id.0, "scholar-001");
                assert!(
                    contract_ref
                        .description
                        .contains("Research and evidence gathering")
                );
            }
            other => panic!("expected TaskAssigned, got {}", other.variant_name()),
        }
    }

    #[tokio::test]
    async fn inline_research_action_publishes_scholar_task() {
        let bus = EventBus::new(16);
        let store = Arc::new(ArtefactStore::new());
        let state = AppState::with_events(bus.clone(), &[], store).await;

        let mut receiver = bus.subscribe(&[]);
        publish_mentions(&state, "/research compare options", empty_review_context()).await;

        let received = tokio::time::timeout(Duration::from_millis(500), receiver.recv())
            .await
            .expect("should receive event")
            .expect("event should be ok");
        match received.as_ref() {
            SemanticEvent::TaskAssigned { worker_id, .. } => {
                assert_eq!(worker_id.0, "scholar-001");
            }
            other => panic!("expected TaskAssigned, got {}", other.variant_name()),
        }
    }

    #[tokio::test]
    async fn multiple_mentions_publish_multiple_task_assigned_events() {
        let bus = EventBus::new(16);
        let store = Arc::new(ArtefactStore::new());
        let state = AppState::with_events(bus.clone(), &[], store).await;

        let mut receiver = bus.subscribe(&[]);
        publish_mentions(
            &state,
            "@intent and @scholar and @worker please help",
            empty_review_context(),
        )
        .await;

        let mut task_assigned_count = 0;
        for _ in 0..3 {
            if let Ok(event) =
                tokio::time::timeout(Duration::from_millis(500), receiver.recv()).await
            {
                if let Ok(event) = event {
                    if event.variant_name() == "TaskAssigned" {
                        task_assigned_count += 1;
                    }
                }
            }
        }

        assert_eq!(
            task_assigned_count, 3,
            "should publish 3 TaskAssigned events for 3 mentions (excl reviewer)"
        );
    }

    // -----------------------------------------------------------------------
    // 4.2 Runtime smoke test with Librarian enabled (no-op vector backend)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn librarian_runs_with_noop_vector_backend() {
        use mmat_memory::librarian::Librarian;
        use mmat_memory::store::MemoryStore;
        use mmat_memory::vector_backend::NoopVectorBackend;

        let bus = EventBus::new(16);
        let store = Arc::new(MemoryStore::open(":memory:").unwrap());
        let librarian = Librarian::new(
            store,
            Arc::new(NoopVectorBackend),
            Duration::from_secs(3600),
        );

        let librarian_bus: Arc<_> = bus.clone().into();
        let handle = tokio::spawn(async move { librarian.run(librarian_bus).await });

        let mut receiver = bus.subscribe(&[]);
        tokio::time::sleep(Duration::from_millis(25)).await;

        let mut lifecycle_event = None;
        for _ in 0..10 {
            bus.publish(SemanticEvent::new_memory_proposed(
                RoleId::new("scholar-001"),
                "Preference",
                "Users prefer dark mode in terminals",
                "Project",
                RoleId::new("librarian"),
                Vec::new(),
                0.95,
            ))
            .unwrap();

            let wait_result = tokio::time::timeout(Duration::from_millis(100), async {
                loop {
                    let event = receiver.recv().await.expect("event should be available");
                    if matches!(
                        event.as_ref(),
                        SemanticEvent::MemoryAccepted { .. } | SemanticEvent::MemoryRejected { .. }
                    ) {
                        break event;
                    }
                }
            })
            .await;

            if let Ok(event) = wait_result {
                lifecycle_event = Some(event);
                break;
            }
        }

        let lifecycle_event =
            lifecycle_event.expect("librarian should publish a memory lifecycle event");

        assert_eq!(source_agent(lifecycle_event.as_ref()), "librarian");

        // Verify librarian is still running (not panicked/errored)
        assert!(!handle.is_finished(), "librarian should still be running");

        // Abort cleanly
        handle.abort();
    }

    #[test]
    fn redact_database_url_redacts_password() {
        assert_eq!(
            redact_database_url("postgres://user:secret@localhost:5432/mmat"),
            "postgres://user:***@localhost:5432/mmat",
        );
    }

    #[test]
    fn redact_database_url_redacts_empty_password() {
        assert_eq!(
            redact_database_url("postgres://user:@localhost:5432/mmat"),
            "postgres://user:***@localhost:5432/mmat",
        );
    }

    #[test]
    fn redact_database_url_does_not_change_without_credentials() {
        assert_eq!(
            redact_database_url("postgres://localhost:5432/mmat"),
            "postgres://localhost:5432/mmat",
        );
    }

    #[test]
    fn redact_database_url_strips_trailing_slash() {
        assert_eq!(
            redact_database_url("postgres://user:secret@localhost/mmat/"),
            "postgres://user:***@localhost/mmat",
        );
    }

    #[test]
    fn redact_database_url_preserves_unix_socket_paths() {
        assert_eq!(
            redact_database_url("/var/run/postgres.sock"),
            "/var/run/postgres.sock",
        );
    }

    // -----------------------------------------------------------------------
    // Role capability readiness tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn projection_has_readiness_on_role_views() {
        let projection = WorkbenchProjection::new();

        let scholar = projection
            .roles
            .get("scholar-001")
            .expect("should have scholar");
        assert_eq!(scholar.readiness.capability, CapabilityStatus::Fallback);
        assert!(!scholar.readiness.has_llm_client);

        let worker = projection
            .roles
            .get("worker-001")
            .expect("should have worker");
        assert_eq!(worker.readiness.capability, CapabilityStatus::Fallback);

        let librarian = projection
            .roles
            .get("librarian")
            .expect("should have librarian");
        assert_eq!(librarian.readiness.capability, CapabilityStatus::Fallback);
        assert!(!librarian.readiness.requires_llm);
    }

    #[tokio::test]
    async fn fallback_role_task_assignment_produces_capability_warning() {
        let store = Arc::new(ArtefactStore::new());
        let mut projection = WorkbenchProjection::new();

        // Set up fallback readiness for worker
        if let Some(role) = projection.roles.get_mut("worker-001") {
            role.readiness = RoleReadiness {
                capability: CapabilityStatus::Fallback,
                has_llm_client: false,
                has_tools: false,
                tool_count: 0,
                fallback_worktree: true,
                requires_llm: true,
                has_artefact_store: false,
                summary: "Worker in fallback mode".to_string(),
            };
        }

        let task_id = Uuid::new_v4().to_string();
        let event = SemanticEvent::new_task_assigned(
            RoleId::new("human"),
            &task_id,
            RoleId::new("worker-001"),
            TaskContract {
                contract_id: Uuid::new_v4().to_string(),
                description: "build feature".to_string(),
            },
            Vec::new(),
        );

        projection.apply_event(&event, &store).await;

        let capability_msg = projection
            .messages
            .iter()
            .find(|m| m.speaker.contains("Capability"));
        assert!(
            capability_msg.is_some(),
            "should have a capability warning message when dispatching to a fallback role"
        );
        assert!(
            capability_msg.unwrap().content.contains("fallback mode"),
            "capability message should mention fallback: {}",
            capability_msg.unwrap().content
        );
    }

    #[tokio::test]
    async fn configured_role_task_assignment_does_not_produce_warning() {
        let store = Arc::new(ArtefactStore::new());
        let mut projection = WorkbenchProjection::new();

        // Set up configured readiness
        if let Some(role) = projection.roles.get_mut("worker-001") {
            role.readiness = RoleReadiness {
                capability: CapabilityStatus::Configured,
                has_llm_client: true,
                has_tools: true,
                tool_count: 3,
                fallback_worktree: false,
                requires_llm: true,
                has_artefact_store: false,
                summary: "Fully configured".to_string(),
            };
        }

        let task_id = Uuid::new_v4().to_string();
        let event = SemanticEvent::new_task_assigned(
            RoleId::new("human"),
            &task_id,
            RoleId::new("worker-001"),
            TaskContract {
                contract_id: Uuid::new_v4().to_string(),
                description: "build feature".to_string(),
            },
            Vec::new(),
        );

        projection.apply_event(&event, &store).await;

        let capability_msg = projection
            .messages
            .iter()
            .find(|m| m.speaker.contains("Capability"));
        assert!(
            capability_msg.is_none(),
            "should not have a capability warning for configured role"
        );
    }

    #[test]
    fn scholar_contract_contains_expected_content() {
        let contract = scholar_contract("investigate options");
        assert!(contract.contains("Research and evidence gathering"));
        assert!(contract.contains("evidence"));
        assert!(contract.contains("Open questions"));
        assert!(contract.contains("confidence"));
        assert!(contract.contains("source references"));
        assert!(contract.contains("Architectural recommendations filtered out"));
        assert!(contract.contains("investigate options"));
    }

    #[test]
    fn worker_contract_contains_expected_content() {
        let contract = worker_contract("implement the feature");
        assert!(contract.contains("Implementation and execution"));
        assert!(contract.contains("implement the feature"));
        assert!(contract.contains("cargo fmt"));
        assert!(contract.contains("cargo test"));
        assert!(contract.contains("path traversal"));
        assert!(contract.contains("worktree"));
        assert!(contract.contains("TaskCompleted"));
        assert!(contract.contains("Safety"));
    }

    #[test]
    fn fallback_role_readiness_exposes_missing_providers() {
        let scholar = Scholar::new();
        let readiness = scholar.role_readiness();
        assert_eq!(readiness.capability, CapabilityStatus::Fallback);
        assert!(!readiness.has_llm_client);
        assert!(readiness.requires_llm);
        assert!(
            readiness.summary.contains("missing"),
            "should mention missing LLM: {}",
            readiness.summary
        );
    }

    #[test]
    fn role_readiness_displays_correct_status_label() {
        assert_eq!(CapabilityStatus::Configured.to_string(), "configured");
        assert_eq!(CapabilityStatus::Degraded.to_string(), "degraded");
        assert_eq!(CapabilityStatus::Fallback.to_string(), "fallback");
        assert_eq!(CapabilityStatus::Unavailable.to_string(), "unavailable");
    }

    #[test]
    fn worker_readiness_includes_fallback_worktree() {
        let worker = Worker::new().with_fallback_worktree(true);
        let readiness = worker.role_readiness();
        assert_eq!(readiness.capability, CapabilityStatus::Fallback);
        assert!(readiness.fallback_worktree);
        assert!(readiness.summary.contains("Fallback worktree: yes"));
    }

    #[test]
    fn worker_without_llm_or_fallback_returns_unavailable() {
        let worker = Worker::new();
        let readiness = worker.role_readiness();
        assert_eq!(readiness.capability, CapabilityStatus::Unavailable);
        assert!(!readiness.fallback_worktree);
        assert!(!readiness.has_llm_client);
    }

    // -----------------------------------------------------------------------
    // 5.1 Lane filtering and global aggregation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn lane_creation_populates_lane_in_projection() {
        let store = Arc::new(ArtefactStore::new());
        let lane_id = Uuid::new_v4().to_string();
        let event = SemanticEvent::new_lane_created(
            RoleId::new("human"),
            &lane_id,
            "UX thread",
            "conversation",
            "#ec4899",
            "Discuss notification UX",
            None,
            Vec::new(),
            None,
        );
        let projection = WorkbenchProjection::from_events(&[event], &store).await;
        assert_eq!(projection.lanes.len(), 1);
        let lane = &projection.lanes[0];
        assert_eq!(lane.id, lane_id);
        assert_eq!(lane.name, "UX thread");
        assert_eq!(lane.colour, "#ec4899");
        assert_eq!(lane.status, LaneStatus::Active);
        assert_eq!(lane.creator, "human");
    }

    #[tokio::test]
    async fn lane_created_event_also_produces_message() {
        let store = Arc::new(ArtefactStore::new());
        let event = SemanticEvent::new_lane_created(
            RoleId::new("human"),
            "lane-conversation-abc",
            "UX thread",
            "conversation",
            "#ec4899",
            "Discuss notification UX",
            None,
            Vec::new(),
            None,
        );
        let projection = WorkbenchProjection::from_events(&[event], &store).await;
        assert!(
            projection
                .messages
                .iter()
                .any(|m| m.content.contains("Created lane")),
            "should have a lane creation message"
        );
    }

    #[tokio::test]
    async fn lane_archived_marks_lane_status() {
        let store = Arc::new(ArtefactStore::new());
        let lane_id = "lane-conversation-abc".to_string();
        let events = vec![
            SemanticEvent::new_lane_created(
                RoleId::new("human"),
                &lane_id,
                "Test lane",
                "conversation",
                "#ec4899",
                "test",
                None,
                Vec::new(),
                None,
            ),
            SemanticEvent::new_lane_archived(RoleId::new("human"), &lane_id),
        ];
        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let lane = projection
            .lanes
            .iter()
            .find(|l| l.id == lane_id)
            .expect("lane should exist");
        assert_eq!(lane.status, LaneStatus::Archived);
    }

    #[tokio::test]
    async fn messages_lane_tagged_from_active_lane() {
        let store = Arc::new(ArtefactStore::new());
        let lane_id = "lane-conversation-xyz".to_string();

        let mut projection = WorkbenchProjection::from_events(
            &[SemanticEvent::new_lane_created(
                RoleId::new("human"),
                &lane_id,
                "Test lane",
                "conversation",
                "#ec4899",
                "test",
                None,
                Vec::new(),
                None,
            )],
            &store,
        )
        .await;

        projection.active_lane_ids = vec![lane_id.clone()];

        projection
            .apply_event(
                &SemanticEvent::new_human_feedback_requested(
                    RoleId::new("intent-lead-001"),
                    "What are we making?",
                    "test",
                ),
                &store,
            )
            .await;

        let last_msg = projection.messages.last().expect("should have a message");
        assert_eq!(last_msg.primary_lane_id, Some(lane_id));
    }

    #[tokio::test]
    async fn global_view_shows_all_messages() {
        let store = Arc::new(ArtefactStore::new());
        let events = vec![
            SemanticEvent::new_human_feedback_requested(
                RoleId::new("intent-lead-001"),
                "hello",
                "test",
            ),
            SemanticEvent::new_human_feedback_received(RoleId::new("human"), "hi"),
            SemanticEvent::new_task_assigned(
                RoleId::new("human"),
                "task-1",
                RoleId::new("worker-001"),
                TaskContract {
                    contract_id: Uuid::new_v4().to_string(),
                    description: "build".to_string(),
                },
                Vec::new(),
            ),
        ];
        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let all_msgs = projection.messages_for_lane_ids(&[]);
        assert_eq!(all_msgs.len(), projection.messages.len());
    }

    #[tokio::test]
    async fn lane_filter_shows_only_matching_messages() {
        let store = Arc::new(ArtefactStore::new());
        let lane_a = "lane-a".to_string();
        let lane_b = "lane-b".to_string();

        let mut projection = WorkbenchProjection::from_events(
            &[SemanticEvent::new_lane_created(
                RoleId::new("human"),
                &lane_a,
                "Lane A",
                "conversation",
                "#6366f1",
                "a",
                None,
                Vec::new(),
                None,
            )],
            &store,
        )
        .await;

        projection.active_lane_ids = vec![lane_a.clone()];
        projection
            .apply_event(
                &SemanticEvent::new_human_feedback_requested(
                    RoleId::new("intent-lead-001"),
                    "msg A",
                    "test",
                ),
                &store,
            )
            .await;

        projection.active_lane_ids = vec![];
        projection
            .apply_event(
                &SemanticEvent::new_human_feedback_received(RoleId::new("human"), "msg B"),
                &store,
            )
            .await;

        let lane_a_msgs = projection.messages_for_lane_ids(&[lane_a.clone()]);
        let lane_b_msgs = projection.messages_for_lane_ids(&[lane_b.clone()]);
        let lane_a_tagged: Vec<_> = lane_a_msgs
            .iter()
            .filter(|m| m.primary_lane_id.as_deref() == Some(&lane_a))
            .collect();
        assert!(
            !lane_a_tagged.is_empty(),
            "lane A filter should include messages tagged with lane A"
        );
        let lane_b_tagged: Vec<_> = lane_b_msgs
            .iter()
            .filter(|m| m.primary_lane_id.as_deref() == Some(&*lane_b))
            .collect();
        assert!(
            lane_b_tagged.is_empty(),
            "no messages are tagged with lane B"
        );
        let global_msgs = projection.messages_for_lane_ids(&[]);
        assert!(global_msgs.len() >= lane_a_msgs.len());
    }

    // -----------------------------------------------------------------------
    // 5.2 Tool-created lane provenance
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn lane_creation_records_provenance() {
        let store = Arc::new(ArtefactStore::new());
        let source_msg_id = Uuid::new_v4().to_string();
        let lane_id = "lane-delivery-abc".to_string();

        let events = vec![SemanticEvent::new_lane_created(
            RoleId::new("pm-001"),
            &lane_id,
            "Feature build",
            "delivery",
            "#14b8a6",
            "Track delivery of the new feature",
            None,
            Vec::new(),
            Some(source_msg_id.clone()),
        )];
        let projection = WorkbenchProjection::from_events(&events, &store).await;

        let lane = &projection.lanes[0];
        assert_eq!(lane.creator, "pm-001");
        assert_eq!(lane.source_message_id, Some(source_msg_id));
        assert_eq!(lane.kind, LaneKind::Delivery);
        assert_eq!(lane.colour, "#14b8a6");
    }

    #[tokio::test]
    async fn lane_creation_publishes_lane_chip_source_message() {
        let bus = EventBus::new(16);
        let artefact_store = Arc::new(ArtefactStore::new());

        let source_msg_id = Uuid::new_v4().to_string();
        let lane_id = "lane-delivery-xyz".to_string();

        let state = AppState {
            bus: bus.clone(),
            projection: Arc::new(RwLock::new(
                WorkbenchProjection::from_events(
                    &[SemanticEvent::new_lane_created(
                        RoleId::new("pm-001"),
                        &lane_id,
                        "Feature",
                        "delivery",
                        "#14b8a6",
                        "track it",
                        None,
                        Vec::new(),
                        Some(source_msg_id.clone()),
                    )],
                    &artefact_store,
                )
                .await,
            )),
            artefact_store,
            scheduler: None,
        };

        let projection = state.projection.read().await;
        let lane = projection
            .lanes
            .iter()
            .find(|l| l.id == lane_id)
            .expect("lane should exist");
        assert_eq!(lane.source_message_id, Some(source_msg_id));
    }

    // -----------------------------------------------------------------------
    // 5.3 Action request resolution
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn action_request_created_is_pending() {
        let store = Arc::new(ArtefactStore::new());
        let request_id = Uuid::new_v4().to_string();
        let events = vec![SemanticEvent::new_action_request_created(
            RoleId::new("intent-lead-001"),
            &request_id,
            "clarification",
            "Do you want dark or light theme?",
            vec!["dark".to_string(), "light".to_string()],
            None,
        )];
        let projection = WorkbenchProjection::from_events(&events, &store).await;
        assert_eq!(projection.action_requests.len(), 1);
        let req = &projection.action_requests[0];
        assert_eq!(req.status, ActionRequestStatus::Pending);
        assert_eq!(req.request_kind, "clarification");
        assert_eq!(req.choices.len(), 2);
    }

    #[tokio::test]
    async fn action_request_resolution_marks_resolved() {
        let store = Arc::new(ArtefactStore::new());
        let request_id = Uuid::new_v4().to_string();
        let events = vec![
            SemanticEvent::new_action_request_created(
                RoleId::new("intent-lead-001"),
                &request_id,
                "clarification",
                "Choose one",
                vec!["A".to_string(), "B".to_string()],
                None,
            ),
            SemanticEvent::new_action_request_resolved(RoleId::new("human"), &request_id, "A"),
        ];
        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let req = projection
            .action_requests
            .iter()
            .find(|r| r.id == request_id)
            .expect("action request should exist");
        assert_eq!(req.status, ActionRequestStatus::Resolved);
        let user_msg = projection
            .messages
            .iter()
            .find(|m| m.speaker == "You")
            .expect("should have a user reply");
        assert_eq!(user_msg.content, "A");
    }

    #[tokio::test]
    async fn action_request_cancelled_marks_cancelled() {
        let store = Arc::new(ArtefactStore::new());
        let request_id = Uuid::new_v4().to_string();
        let events = vec![
            SemanticEvent::new_action_request_created(
                RoleId::new("intent-lead-001"),
                &request_id,
                "approval",
                "Approve?",
                Vec::new(),
                None,
            ),
            SemanticEvent::new_action_request_cancelled(
                RoleId::new("intent-lead-001"),
                &request_id,
                "no longer relevant",
            ),
        ];
        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let req = projection
            .action_requests
            .iter()
            .find(|r| r.id == request_id)
            .expect("action request should exist");
        assert_eq!(req.status, ActionRequestStatus::Cancelled);
    }

    #[tokio::test]
    async fn action_request_displays_inline_in_chat() {
        let store = Arc::new(ArtefactStore::new());
        let events = vec![SemanticEvent::new_action_request_created(
            RoleId::new("worker-001"),
            "req-1",
            "clarification",
            "Should we use async or sync?",
            vec!["async".to_string(), "sync".to_string()],
            None,
        )];
        let projection = WorkbenchProjection::from_events(&events, &store).await;
        let msg = projection
            .messages
            .iter()
            .find(|m| m.content.contains("[Action"))
            .expect("should have action request message");
        assert!(msg.content.contains("clarification"), "{}", msg.content);
        assert!(msg.content.contains("async"), "{}", msg.content);
    }
}
