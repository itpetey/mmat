use std::{cmp::Reverse, collections::BTreeMap, convert::Infallible, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Sse, sse::Event},
    routing::{get, post},
};
use futures_util::{Stream, StreamExt, stream};
use mmat_coordinator::{OrganisationConfig, OrganisationRuntime, Role, RoleRegistry};
use mmat_event_stream::{
    event::{RoleId, SemanticEvent, TaskContract},
    event_bus::{EventBus, RecvError},
};
use mmat_memory::artefact_store::ArtefactStore;
use mmat_roles::{
    Architect, Auditor, IntentLead, OpsManager, ProjectManager, Reviewer, Scholar, Worker,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::error;
use uuid::Uuid;

pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";

#[derive(Clone)]
pub struct AppState {
    bus: EventBus,
    projection: Arc<RwLock<WorkbenchProjection>>,
    artefact_store: Arc<ArtefactStore>,
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
    pub(crate) dag_steps: Vec<DagStepView>,
    pub(crate) pending_question: Option<String>,
    pub(crate) active_artefact_id: Option<String>,
    pub(crate) active_step_id: Option<String>,
    pub(crate) has_conversation: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) understanding: UnderstandingView,
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
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MessageView {
    pub(crate) speaker: String,
    pub(crate) content: String,
    pub(crate) timestamp_ns: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArtefactView {
    pub(crate) id: String,
    pub(crate) artefact_type: String,
    pub(crate) title: String,
    pub(crate) producer_role: String,
    pub(crate) content_hash: String,
    pub(crate) content: serde_json::Value,
    pub(crate) evidence_refs: Vec<String>,
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
        .route("/api/messages", post(post_message))
        .route("/api/notifications/{id}/ack", post(ack_notification))
        .with_state(state)
}

impl AppState {
    pub fn with_events(
        bus: EventBus,
        events: &[SemanticEvent],
        artefact_store: Arc<ArtefactStore>,
    ) -> Self {
        Self {
            bus,
            projection: Arc::new(RwLock::new(WorkbenchProjection::from_events(
                events,
                &artefact_store,
            ))),
            artefact_store,
        }
    }

    pub fn publish(&self, event: SemanticEvent) {
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

pub fn build_runtime() -> Result<(AppState, OrganisationRuntime), WorkbenchError> {
    let database_url = require_database_url()?;

    let intent_lead = IntentLead::new();
    let scholar = Scholar::new();
    let ops_manager = OpsManager::new();
    let architect = Architect::new();
    let project_manager = ProjectManager::new();
    let worker = Worker::new().with_fallback_worktree(true);
    let reviewer = Reviewer::new();
    let auditor = Auditor::new();

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
    );

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
                },
            )
        })
        .collect();

        Self {
            project: ProjectView {
                id: "project-workbench-mvp".to_string(),
                name: "SELIUM".to_string(),
                status: "New project".to_string(),
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
            pending_question: None,
            active_artefact_id: None,
            active_step_id: Some("intent".to_string()),
            has_conversation: false,
        }
    }

    pub fn from_events(events: &[SemanticEvent], artefact_store: &ArtefactStore) -> Self {
        let mut projection = Self::new();
        for event in events {
            projection.apply_event(event, artefact_store);
        }
        projection
    }

    pub fn has_conversation_history(&self) -> bool {
        self.has_conversation
    }

    #[allow(dead_code)]
    pub(crate) fn events_by_lane(&self, lane: Lane) -> Vec<&EventView> {
        self.events
            .iter()
            .filter(|ev| classify_event_variant_lane(&ev.variant) == lane)
            .collect()
    }

    fn apply_event(&mut self, event: &SemanticEvent, artefact_store: &ArtefactStore) {
        self.events.push(EventView::from_event(event));
        if self.events.len() > 200 {
            let overflow = self.events.len().saturating_sub(200);
            self.events.drain(0..overflow);
        }

        match event {
            SemanticEvent::HumanFeedbackRequested {
                event_id,
                question,
                timestamp_ns,
                ..
            } => {
                self.has_conversation = true;
                self.pending_question = Some(question.clone());
                self.messages.push(MessageView {
                    speaker: "Intent Lead".to_string(),
                    content: question.clone(),
                    timestamp_ns: *timestamp_ns,
                });
                self.set_role(
                    "intent-lead-001",
                    "Waiting",
                    "Interviewing the human stakeholder",
                );
                self.add_step_event("intent", event_id.to_string());
                self.add_notification(NotificationView {
                    id: event_id.to_string(),
                    kind: "Question".to_string(),
                    title: "Intent Lead question".to_string(),
                    summary: question.clone(),
                    target: "chat".to_string(),
                    acknowledged: false,
                    timestamp_ns: *timestamp_ns,
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
                self.pending_question = None;
                self.messages.push(MessageView {
                    speaker: "You".to_string(),
                    content: answer.clone(),
                    timestamp_ns: *timestamp_ns,
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
            } => self.memories.push(MemoryView {
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
            }),
            SemanticEvent::MemoryAccepted {
                proposal_event_id,
                memory_id,
                accepted_authority,
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
            }
            SemanticEvent::TaskAssigned {
                event_id,
                task_id,
                worker_id,
                contract_ref,
                dependencies,
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
                self.set_role("reviewer", "Completed", summary);
                self.upsert_step(DagStepView {
                    id: format!("review-{task_id}"),
                    label: "Review".to_string(),
                    role: "reviewer".to_string(),
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
                        title: label_for_artefact(artefact_type),
                        producer_role: producer_role.0.clone(),
                        content_hash: content_hash.clone(),
                        content: load_artefact_content(storage_uri, artefact_store),
                        evidence_refs: evidence_refs
                            .iter()
                            .map(|evidence| evidence.event_id.to_string())
                            .collect(),
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
                });
            }
            SemanticEvent::PolicyViolationDetected { .. }
            | SemanticEvent::EvidenceChainBroken { .. } => {
                self.set_role(
                    "auditor",
                    "Running",
                    "Inspecting evidence and process integrity",
                );
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
    }

    fn update_step(&mut self, id: &str, state: &str, summary: &str) {
        if let Some(step) = self.dag_steps.iter_mut().find(|step| step.id == id) {
            step.state = state.to_string();
            step.summary = summary.to_string();
        }
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
                state,
                summary,
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
                    projection.apply_event(event.as_ref(), &state.artefact_store);
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

    state.publish(SemanticEvent::new_human_feedback_requested(
        RoleId::new("intent-lead-001"),
        "What are we making, who is it for, and what would make it excellent?",
        "Start of the Intent Lead interview.",
    ));
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
    Json(state.projection.read().await.clone())
}

async fn post_message(
    State(state): State<AppState>,
    Json(request): Json<MessageRequest>,
) -> impl IntoResponse {
    let message = request.message.trim().to_string();
    if message.is_empty() {
        return (StatusCode::BAD_REQUEST, "message must not be empty").into_response();
    }

    let human_event = SemanticEvent::new_human_feedback_received(RoleId::new("human"), &message);
    state.publish(human_event);
    publish_mentions(&state, &message);

    StatusCode::ACCEPTED.into_response()
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

fn publish_mentions(state: &AppState, message: &str) {
    for role in mentioned_roles(message) {
        let task_id = Uuid::new_v4().to_string();
        state.publish(SemanticEvent::new_task_assigned(
            RoleId::new("human"),
            &task_id,
            RoleId::new(role),
            TaskContract {
                contract_id: Uuid::new_v4().to_string(),
                description: format!("Mentioned in SELIUM channel: {message}"),
            },
            Vec::new(),
        ));
    }
}

fn mentioned_roles(message: &str) -> Vec<&'static str> {
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
    .filter_map(|(mention, role)| lower.contains(mention).then_some(role))
    .collect()
}

async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.bus.subscribe(&[]);
    let initial_state = state.projection.read().await.clone();
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

fn load_artefact_content(storage_uri: &str, artefact_store: &ArtefactStore) -> serde_json::Value {
    if storage_uri.starts_with("db://") {
        let handle = tokio::runtime::Handle::try_current();
        if let Ok(handle) = handle {
            return tokio::task::block_in_place(|| {
                match handle.block_on(artefact_store.get_payload(storage_uri)) {
                    Ok(Some(content)) => serde_json::from_str(&content)
                        .unwrap_or_else(|_| serde_json::json!({ "content": content })),
                    Ok(None) => {
                        serde_json::json!({ "storage_uri": storage_uri, "error": "not found" })
                    }
                    Err(err) => serde_json::json!({
                        "storage_uri": storage_uri,
                        "error": format!("failed to load artefact: {err}")
                    }),
                }
            });
        }
    }

    if let Some(path) = storage_uri.strip_prefix("file://") {
        return match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content)
                .unwrap_or_else(|_| serde_json::json!({ "content": content })),
            Err(err) => serde_json::json!({
                "storage_uri": storage_uri,
                "error": format!("failed to read artefact payload: {err}")
            }),
        };
    }

    serde_json::json!({ "storage_uri": storage_uri })
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
        | SemanticEvent::Heartbeat { source_agent, .. } => source_agent.0.clone(),
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
        | SemanticEvent::Heartbeat { timestamp_ns, .. } => *timestamp_ns,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_runtime_role_ids_without_suffix_noise() {
        assert_eq!(label_for_role("intent-lead-001"), "Intent Lead");
        assert_eq!(label_for_role("scholar-001"), "Scholar");
        assert_eq!(label_for_role("pm-001"), "Project Manager");
    }

    #[test]
    fn mention_detection_targets_runtime_role_ids() {
        let roles = mentioned_roles("@intent please ask @scholar and @pm for help");
        assert_eq!(roles, vec!["intent-lead-001", "scholar-001", "pm-001"]);
    }

    #[test]
    fn projection_hydrates_conversation_history_from_events() {
        let events = vec![
            SemanticEvent::new_human_feedback_requested(
                RoleId::new("intent-lead-001"),
                "What are we making?",
                "test",
            ),
            SemanticEvent::new_human_feedback_received(RoleId::new("human"), "A useful tool"),
        ];

        let store = ArtefactStore::new();
        let projection = WorkbenchProjection::from_events(&events, &store);

        assert!(projection.has_conversation_history());
        assert_eq!(projection.messages.len(), 2);
        assert_eq!(projection.messages[0].speaker, "Intent Lead");
        assert_eq!(projection.messages[1].speaker, "You");
    }

    #[test]
    fn organisation_events_do_not_count_as_conversation_history() {
        let store = ArtefactStore::new();
        let projection = WorkbenchProjection::from_events(
            &[SemanticEvent::new_organisation_started(RoleId::new(
                "coordinator",
            ))],
            &store,
        );

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
            let projection = WorkbenchProjection::from_events(&events, &artefact_store);

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

    #[test]
    fn lane_filter_excludes_unrelated_events() {
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

        let projection = WorkbenchProjection::from_events(&events, &store);
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

    #[test]
    fn task_assignment_creates_dag_step() {
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

        let projection = WorkbenchProjection::from_events(&events, &store);
        assert!(!projection.has_conversation_history());

        let worker_step = projection.dag_steps.iter().find(|s| s.id == task_id);
        assert!(
            worker_step.is_some(),
            "should have a DAG step for the assigned task",
        );
        assert_eq!(worker_step.unwrap().role, "worker-001");
        assert_eq!(worker_step.unwrap().state, "Running");
    }

    #[test]
    fn review_completed_updates_dag_step() {
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

        let projection = WorkbenchProjection::from_events(&events, &store);
        let review_step = projection
            .dag_steps
            .iter()
            .find(|s| s.id == format!("review-{task_id}"));
        assert!(review_step.is_some(), "should have a review DAG step");
        assert_eq!(review_step.unwrap().state, "Accepted");
    }

    #[test]
    fn rework_review_shows_needs_rework_state() {
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

        let projection = WorkbenchProjection::from_events(&events, &store);
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

    #[test]
    fn artefact_produced_populates_artefact_list() {
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

        let projection = WorkbenchProjection::from_events(&events, &store);
        let artefact = projection.artefacts.iter().find(|a| a.id == artefact_id);
        assert!(artefact.is_some(), "should contain the produced artefact");
        assert_eq!(artefact.unwrap().artefact_type, "prd");
    }

    #[test]
    fn artefact_load_failure_produces_error_state() {
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

        let projection = WorkbenchProjection::from_events(&events, &store);
        let artefact = projection.artefacts.iter().find(|a| a.id == artefact_id);
        assert!(artefact.is_some(), "artefact should be in the projection");

        let content = &artefact.unwrap().content;
        assert!(
            content.get("error").is_some() || content.get("storage_uri").is_some(),
            "artefact load failure should not panic, should produce error or fallback content: {content:?}",
        );
    }

    #[test]
    fn dag_steps_constructed_from_multiple_events() {
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

        let projection = WorkbenchProjection::from_events(&events, &store);

        assert_eq!(projection.messages.len(), 2, "should have 2 messages");
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
                "pattern",
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

            let projection = WorkbenchProjection::from_events(&events, &artefact_store);

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
}
