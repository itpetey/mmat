use std::{cmp::Reverse, collections::BTreeMap, convert::Infallible, net::SocketAddr, sync::Arc};

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
use tracing::{error, info};
use uuid::Uuid;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";

#[derive(Clone)]
struct AppState {
    bus: EventBus,
    projection: Arc<RwLock<WorkbenchProjection>>,
    artefact_store: Arc<ArtefactStore>,
}

#[derive(Debug, Error)]
enum WorkbenchError {
    #[error("invalid bind address {address}: {source}")]
    InvalidBindAddress {
        address: String,
        source: std::net::AddrParseError,
    },
    #[error("failed to bind listener: {0}")]
    Bind(#[from] std::io::Error),
    #[error("server failed: {0}")]
    Server(std::io::Error),
    #[error("failed to initialise workbench runtime: {0}")]
    Init(String),
}

#[derive(Clone, Debug, Serialize)]
struct WorkbenchProjection {
    project: ProjectView,
    roles: BTreeMap<String, RoleView>,
    events: Vec<EventView>,
    messages: Vec<MessageView>,
    artefacts: Vec<ArtefactView>,
    memories: Vec<MemoryView>,
    notifications: Vec<NotificationView>,
    dag_steps: Vec<DagStepView>,
    pending_question: Option<String>,
    active_artefact_id: Option<String>,
    active_step_id: Option<String>,
    has_conversation: bool,
}

#[derive(Clone, Debug, Serialize)]
struct ProjectView {
    id: String,
    name: String,
    status: String,
    understanding: UnderstandingView,
}

#[derive(Clone, Debug, Serialize)]
struct UnderstandingView {
    intent: String,
    audience: String,
    success: Vec<String>,
    constraints: Vec<String>,
    open_questions: Vec<String>,
    confidence: f64,
}

#[derive(Clone, Debug, Serialize)]
struct RoleView {
    id: String,
    label: String,
    state: String,
    summary: String,
}

#[derive(Clone, Debug, Serialize)]
struct EventView {
    id: String,
    variant: String,
    source_agent: String,
    timestamp_ns: u64,
    summary: String,
    detail: serde_json::Value,
}

#[derive(Clone, Debug, Serialize)]
struct MessageView {
    speaker: String,
    content: String,
    timestamp_ns: u64,
}

#[derive(Clone, Debug, Serialize)]
struct ArtefactView {
    id: String,
    artefact_type: String,
    title: String,
    producer_role: String,
    content_hash: String,
    content: serde_json::Value,
    evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct MemoryView {
    id: String,
    memory_type: String,
    scope: String,
    authority: String,
    confidence: f64,
    content: String,
    evidence_refs: Vec<String>,
    status: String,
}

#[derive(Clone, Debug, Serialize)]
struct NotificationView {
    id: String,
    kind: String,
    title: String,
    summary: String,
    target: String,
    acknowledged: bool,
    timestamp_ns: u64,
}

#[derive(Clone, Debug, Serialize)]
struct DagStepView {
    id: String,
    label: String,
    role: String,
    state: String,
    summary: String,
    dependencies: Vec<String>,
    artefact_ids: Vec<String>,
    event_ids: Vec<String>,
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

#[tokio::main]
async fn main() -> Result<(), WorkbenchError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "mmat_workbench=info".to_string()),
        )
        .init();

    let bind_addr =
        std::env::var("MMAT_WORKBENCH_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
    let socket_addr =
        bind_addr
            .parse::<SocketAddr>()
            .map_err(|source| WorkbenchError::InvalidBindAddress {
                address: bind_addr.clone(),
                source,
            })?;

    let (state, runtime) = build_runtime()?;
    spawn_projection_task(state.clone());
    tokio::spawn(async move {
        if let Err(err) = runtime.run().await {
            error!("MMAT organisation runtime stopped with error: {}", err);
        }
    });
    seed_workbench(&state).await;

    let app = Router::new()
        .route("/", get(index))
        .route("/events", get(events))
        .route("/api/state", get(snapshot))
        .route("/api/messages", post(post_message))
        .route("/api/notifications/{id}/ack", post(ack_notification))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(socket_addr).await?;
    info!("MMAT workbench listening on http://{}", socket_addr);

    axum::serve(listener, app)
        .await
        .map_err(WorkbenchError::Server)
}

impl AppState {
    fn with_events(
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

    fn publish(&self, event: SemanticEvent) {
        if let Err(err) = self.bus.publish(event) {
            error!("failed to publish workbench event: {}", err);
        }
    }
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

fn build_runtime() -> Result<(AppState, OrganisationRuntime), WorkbenchError> {
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
    registry
        .register(intent_lead.spec())
        .map_err(|err| WorkbenchError::Init(err.to_string()))?;
    registry
        .register(scholar.spec())
        .map_err(|err| WorkbenchError::Init(err.to_string()))?;
    registry
        .register(ops_manager.spec())
        .map_err(|err| WorkbenchError::Init(err.to_string()))?;
    registry
        .register(architect.spec())
        .map_err(|err| WorkbenchError::Init(err.to_string()))?;
    registry
        .register(project_manager.spec())
        .map_err(|err| WorkbenchError::Init(err.to_string()))?;
    registry
        .register(worker.spec())
        .map_err(|err| WorkbenchError::Init(err.to_string()))?;
    registry
        .register(reviewer.spec())
        .map_err(|err| WorkbenchError::Init(err.to_string()))?;
    registry
        .register(auditor.spec())
        .map_err(|err| WorkbenchError::Init(err.to_string()))?;

    let config = OrganisationConfig {
        database_url: Some(database_url),
        event_store_path: None,
        memory_store_path: None,
        ..OrganisationConfig::default()
    };

    let mut runtime = OrganisationRuntime::new(config, registry)
        .map_err(|err| WorkbenchError::Init(err.to_string()))?;

    let replayed_events = runtime
        .event_store()
        .replay(0, None)
        .map_err(|err| WorkbenchError::Init(format!("failed to replay events: {err}")))?;

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

impl WorkbenchProjection {
    fn new() -> Self {
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

    fn from_events(events: &[SemanticEvent], artefact_store: &ArtefactStore) -> Self {
        let mut projection = Self::new();
        for event in events {
            projection.apply_event(event, artefact_store);
        }
        projection
    }

    fn has_conversation_history(&self) -> bool {
        self.has_conversation
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

    fn acknowledge_notification(&mut self, id: &str) -> bool {
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

fn spawn_projection_task(state: AppState) {
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

async fn seed_workbench(state: &AppState) {
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
    Html(INDEX_HTML)
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
        assert!(
            msg.contains("Postgres"),
            "should mention Postgres: {msg}",
        );
    }

    #[tokio::test]
    async fn postgres_event_replay_preserves_events_across_restart() {
        let base_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => return,
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
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>MMAT Workbench</title>
  <style>
    :root {
      color-scheme: dark;
      --bg: #020407;
      --text: #f4f7fb;
      --muted: #8b8b8b;
      --line: #8b8b8b;
      --me: #23e09e;
      --intent: #00c0e8;
      --scholar: #ff2d55;
      --ops: #f2c572;
      --architect: #eace5e;
      --panel: #070a0f;
      --badge: #ff2d55;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      background: var(--bg);
      color: var(--text);
      font: 14px/1.35 Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    button, textarea { font: inherit; }
    button { cursor: pointer; }
    .app-shell { min-height: 100vh; padding: 32px 96px 22px; display: grid; grid-template-rows: 48px 1fr auto; gap: 28px; }
    .topbar { display: grid; grid-template-columns: 1fr auto 1fr; align-items: start; }
    .project-mark { justify-self: start; position: relative; margin-left: 14px; margin-top: 3px; color: white; font-size: 12px; line-height: 27px; min-width: 62px; text-align: center; border: 1px solid white; background: var(--bg); }
    .project-mark::before { content: ""; position: absolute; inset: -4px 4px 4px -4px; border: 1px solid white; z-index: -1; }
    .wordmark { justify-self: center; margin-top: -32px; background: white; color: #111; padding: 7px 10px 4px; font-family: "Major Mono Display", "Courier New", monospace; font-size: 24px; letter-spacing: 0.02em; text-decoration: underline; line-height: 1; }
    .view-actions { justify-self: end; display: flex; gap: 24px; align-items: center; position: relative; }
    .icon-button { position: relative; width: 24px; height: 24px; color: white; background: transparent; border: 0; padding: 0; opacity: 0.78; }
    .icon-button.active, .icon-button:hover { opacity: 1; color: var(--me); }
    .icon-button svg { width: 24px; height: 24px; stroke: currentColor; fill: none; stroke-width: 1.6; }
    .count-badge { position: absolute; top: -8px; right: -9px; min-width: 16px; height: 16px; padding: 0 4px; border-radius: 999px; background: var(--badge); color: white; font-size: 10px; line-height: 16px; text-align: center; }
    .notification-panel { position: absolute; right: 0; top: 34px; width: min(380px, calc(100vw - 2rem)); background: #080b10; border: 1px solid #30343b; padding: 10px; display: none; z-index: 20; box-shadow: 0 20px 80px rgba(0,0,0,0.6); }
    .notification-panel.open { display: block; }
    .notice { padding: 10px 0; border-bottom: 1px solid #242830; }
    .notice:last-child { border-bottom: 0; }
    .notice strong { display: block; margin-bottom: 4px; }
    .notice div { color: var(--muted); margin-bottom: 8px; }
    .notice button { background: transparent; border: 1px solid var(--line); color: white; padding: 4px 8px; }
    .workspace { min-height: 0; }
    .view { display: none; min-height: calc(100vh - 165px); }
    .view.active { display: block; }
    .channel { height: calc(100vh - 186px); overflow: auto; padding-top: 10px; }
    .channel-row { display: grid; grid-template-columns: 84px minmax(0, 1fr); column-gap: 12px; margin-bottom: 14px; }
    .speaker-label { text-align: right; font-size: 14px; white-space: nowrap; }
    .speaker-label::after { content: " >"; }
    .speaker-me { color: var(--me); }
    .speaker-intent { color: var(--intent); }
    .speaker-scholar { color: var(--scholar); }
    .speaker-ops { color: var(--ops); }
    .speaker-architect { color: var(--architect); }
    .speaker-muted { color: var(--muted); }
    .message-body { max-width: 1166px; white-space: pre-wrap; }
    .message-body.log { color: var(--muted); font-family: "Intel One Mono", "SFMono-Regular", Consolas, monospace; font-size: 13px; }
    .message-body.system { color: var(--muted); }
    .message-body .mention { color: white; font-weight: 700; }
    .message-body .code-token { color: var(--architect); font-family: "Intel One Mono", "SFMono-Regular", Consolas, monospace; }
    .composer-wrap { border-top: 1px solid var(--line); padding-top: 24px; }
    form { display: grid; gap: 8px; }
    textarea { width: 100%; min-height: 54px; resize: vertical; background: transparent; border: 0; color: white; outline: none; padding: 0; }
    .submit-hint { color: var(--muted); font-size: 13px; }
    .submit-hint strong { color: var(--muted); }
    .submit-hint code { font-family: "Intel One Mono", "SFMono-Regular", Consolas, monospace; }
    .dag-layout { display: grid; grid-template-columns: minmax(0, 1fr) 390px; gap: 32px; }
    .dag-canvas { min-height: calc(100vh - 186px); border-top: 1px solid #20242b; padding-top: 40px; display: grid; grid-template-columns: repeat(3, minmax(180px, 1fr)); gap: 34px; align-content: start; }
    .dag-step { color: white; background: transparent; border: 1px solid #30343b; text-align: left; padding: 16px; min-height: 120px; }
    .dag-step.active { border-color: var(--me); box-shadow: 0 0 0 1px var(--me); }
    .dag-step strong { display: block; margin-bottom: 10px; }
    .dag-step .state { color: var(--me); font-size: 12px; margin-bottom: 8px; text-transform: uppercase; letter-spacing: 0.08em; }
    .side-panel { border-left: 1px solid #30343b; padding-left: 24px; min-height: calc(100vh - 186px); overflow: auto; }
    .side-panel h2 { margin: 0 0 14px; font-size: 13px; color: var(--muted); font-weight: 400; text-transform: uppercase; letter-spacing: 0.1em; }
    .detail-grid { display: grid; gap: 10px; }
    .detail-card { border-top: 1px solid #30343b; padding: 14px 0; }
    .detail-card h3 { margin: 0 0 8px; font-size: 13px; color: var(--muted); text-transform: uppercase; letter-spacing: 0.08em; }
    .memory { margin-bottom: 12px; }
    .memory .meta { color: var(--muted); font-size: 12px; margin-bottom: 6px; }
    pre { white-space: pre-wrap; word-break: break-word; margin: 0; color: var(--muted); font-family: "Intel One Mono", "SFMono-Regular", Consolas, monospace; font-size: 12px; }
    .compact-list { list-style: none; padding: 0; margin: 0; display: grid; gap: 6px; }
    .compact-list li, .empty { color: var(--muted); }
    @media (max-width: 920px) { .app-shell { padding: 22px; } .topbar { grid-template-columns: 1fr auto; gap: 14px; } .wordmark { grid-column: 1 / -1; grid-row: 1; margin-top: 0; } .project-mark { grid-row: 2; } .view-actions { grid-row: 2; } .channel-row { grid-template-columns: 72px minmax(0, 1fr); } .dag-layout { grid-template-columns: 1fr; } .side-panel { border-left: 0; padding-left: 0; } }
  </style>
</head>
<body>
  <div class="app-shell">
    <header class="topbar">
      <div id="project-chip" class="project-mark">SELIUM</div>
      <div class="wordmark">makemeathing</div>
      <div class="view-actions">
        <button id="dag-view-button" class="icon-button" type="button" aria-label="Show DAG">
          <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="6" cy="12" r="2"/><circle cx="17" cy="6" r="2"/><circle cx="17" cy="18" r="2"/><path d="M8 11l7-4M8 13l7 4"/></svg>
        </button>
        <button id="chat-view-button" class="icon-button active" type="button" aria-label="Show chat">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 6.5h14v9H9l-4 3v-12z"/><path d="M8 9.5h8M8 12.5h5"/></svg>
          <span id="notification-count" class="count-badge" hidden>0</span>
        </button>
        <div id="notification-panel" class="notification-panel"></div>
      </div>
    </header>
    <main class="workspace">
      <section id="chat-view" class="view active">
        <div id="conversation" class="channel"></div>
      </section>
      <section id="dag-view" class="view">
        <div class="dag-layout">
          <div id="dag" class="dag-canvas"></div>
          <aside id="step-detail" class="side-panel"></aside>
        </div>
      </section>
    </main>
    <div class="composer-wrap">
      <form id="message-form">
        <textarea id="message" placeholder="Message SELIUM. Mention @intent, @scholar, @ops, @architect, @pm, @worker, @reviewer or @auditor to summon attention."></textarea>
        <div class="submit-hint"><strong>Press</strong> <code>⌘ + Return</code> <strong>to submit</strong></div>
      </form>
    </div>
  </div>
  <script>
    let state = { project: {}, roles: {}, events: [], messages: [], artefacts: [], memories: [], notifications: [], dag_steps: [], active_artefact_id: null, active_step_id: null };
    let selectedArtefactId = null;
    let selectedStepId = null;
    let activeView = 'chat';

    async function loadState() {
      const response = await fetch('/api/state');
      state = await response.json();
      render();
    }

    function connectEvents() {
      const source = new EventSource('/events');
      source.onmessage = (message) => {
        const update = JSON.parse(message.data);
        if (update.type === 'State') state = update.payload;
        if (update.type === 'Event') state.events.push(update.payload);
        if (update.type === 'Notice') console.warn(update.payload);
        if (state.events.length > 200) state.events = state.events.slice(-200);
        loadState();
      };
    }

    function render() {
      renderHeader();
      renderConversation();
      renderDag();
      renderStepDetail();
      renderNotifications();
    }

    function renderHeader() {
      const project = state.project || {};
      document.getElementById('project-chip').textContent = project.name || 'SELIUM';
      document.getElementById('chat-view-button').classList.toggle('active', activeView === 'chat');
      document.getElementById('dag-view-button').classList.toggle('active', activeView === 'dag');
      document.getElementById('chat-view').classList.toggle('active', activeView === 'chat');
      document.getElementById('dag-view').classList.toggle('active', activeView === 'dag');
    }

    function renderConversation() {
      const root = document.getElementById('conversation');
      root.innerHTML = '';
      const entries = channelEntries();
      for (const entry of entries) {
        const el = document.createElement('div');
        el.className = 'channel-row';
        el.innerHTML = `<div class="speaker-label ${speakerClass(entry.speaker)}">${escapeHtml(entry.speaker)}</div><div class="message-body ${entry.kind}">${formatMessage(entry.content)}</div>`;
        root.appendChild(el);
      }
      root.scrollTop = root.scrollHeight;
    }

    function channelEntries() {
      return (state.events || []).filter(isChannelEvent).map(event => {
        const detail = event.detail || {};
        switch (event.variant) {
          case 'HumanFeedbackReceived':
            return { speaker: 'ME', kind: 'text', content: detail.answer || event.summary };
          case 'HumanFeedbackRequested':
            return { speaker: roleName(event.source_agent), kind: 'text', content: `@me ${detail.question || event.summary}` };
          case 'ToolExecuted':
            return { speaker: roleName(event.source_agent), kind: 'log', content: toolText(detail) };
          case 'ClaimMade':
            return { speaker: roleName(event.source_agent), kind: 'text', content: detail.claim_text || event.summary };
          case 'DecisionRecorded':
            return { speaker: roleName(event.source_agent), kind: 'text', content: detail.decision_text || event.summary };
          case 'ArtefactProduced':
            return { speaker: roleName(event.source_agent), kind: 'system', content: `Produced ${detail.artefact_type || 'artefact'} ${detail.artefact_id || ''}` };
          case 'ReviewCompleted':
            return { speaker: roleName(event.source_agent), kind: 'system', content: event.summary };
          default:
            return { speaker: roleName(event.source_agent), kind: 'system', content: event.summary };
        }
      });
    }

    function isChannelEvent(event) {
      return !['OrganisationStarted', 'OrganisationStopped', 'RoleStateChanged', 'Heartbeat', 'MemoryAccepted'].includes(event.variant);
    }

    function renderDag() {
      const root = document.getElementById('dag');
      const steps = state.dag_steps || [];
      const activeId = selectedStepId || state.active_step_id || (steps[0] && steps[0].id);
      root.innerHTML = steps.length ? '' : '<div class="empty">No project flow yet.</div>';
      for (const step of steps) {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = `dag-step ${step.id === activeId ? 'active' : ''}`;
        button.onclick = () => { selectedStepId = step.id; renderDag(); renderStepDetail(); };
        button.innerHTML = `<strong>${escapeHtml(step.label)}</strong><div class="state">${escapeHtml(step.state)} · ${escapeHtml(step.role)}</div><div>${escapeHtml(step.summary)}</div>`;
        root.appendChild(button);
      }
    }

    function renderStepDetail() {
      const root = document.getElementById('step-detail');
      const steps = state.dag_steps || [];
      const step = steps.find(s => s.id === (selectedStepId || state.active_step_id)) || steps[0];
      if (!step) {
        root.innerHTML = '<div class="empty">Select a step to inspect artefacts, logs and semantic evidence.</div>';
        return;
      }
      const artefacts = (state.artefacts || []).filter(a => (step.artefact_ids || []).includes(a.id));
      const events = (state.events || []).filter(e => (step.event_ids || []).includes(e.id));
      root.innerHTML = `
        <div class="detail-card"><h3>${escapeHtml(step.label)}</h3><div>${escapeHtml(step.summary)}</div><div class="empty">Role: ${escapeHtml(step.role)} · State: ${escapeHtml(step.state)}</div></div>
        <div class="detail-grid">
          <div class="detail-card"><h3>Artefacts</h3>${artefactsHtml(artefacts)}</div>
          <div class="detail-card"><h3>Logs</h3>${eventsHtml(events)}</div>
          <div class="detail-card"><h3>Memory</h3>${memoriesHtml(state.memories || [])}</div>
          <div class="detail-card"><h3>CoT</h3><p class="empty">Raw chain-of-thought is intentionally not shown. MMAT exposes consequential semantic events, claims, artefacts and evidence instead.</p></div>
        </div>
      `;
    }

    function renderNotifications() {
      const pending = (state.notifications || []).filter(n => !n.acknowledged);
      const badge = document.getElementById('notification-count');
      badge.textContent = pending.length;
      badge.hidden = pending.length === 0;
      const panel = document.getElementById('notification-panel');
      panel.innerHTML = pending.length ? '' : '<div class="empty">No items need your attention.</div>';
      for (const item of pending) {
        const el = document.createElement('div');
        el.className = 'notice';
        el.innerHTML = `<strong>${escapeHtml(item.title)}</strong><div>${escapeHtml(item.summary)}</div><button type="button">Acknowledge</button>`;
        el.querySelector('button').onclick = async () => {
          await fetch(`/api/notifications/${encodeURIComponent(item.id)}/ack`, { method: 'POST' });
          await loadState();
        };
        panel.appendChild(el);
      }
    }

    function roleName(role) {
      return ({
        human: 'ME',
        'intent-lead': 'INTENT',
        'intent-lead-001': 'INTENT',
        scholar: 'SCHOLAR',
        'scholar-001': 'SCHOLAR',
        'ops-manager': 'OPS',
        'ops-manager-001': 'OPS',
        architect: 'ARCHITECT',
        'architect-001': 'ARCHITECT',
        'project-manager': 'PM',
        'pm-001': 'PM',
        worker: 'WORKER',
        'worker-001': 'WORKER',
        reviewer: 'REVIEWER',
        'reviewer-001': 'REVIEWER',
        auditor: 'AUDITOR',
        'auditor-001': 'AUDITOR',
        librarian: 'LIBRARIAN',
        coordinator: 'SYSTEM'
      })[role] || String(role || 'SYSTEM').toUpperCase();
    }

    function speakerClass(speaker) {
      const normalised = String(speaker).toLowerCase();
      if (normalised === 'me') return 'speaker-me';
      if (normalised === 'intent') return 'speaker-intent';
      if (normalised === 'scholar') return 'speaker-scholar';
      if (normalised === 'ops') return 'speaker-ops';
      if (normalised === 'architect') return 'speaker-architect';
      return 'speaker-muted';
    }

    function toolText(detail) {
      const command = detail.tool_name || 'tool';
      const output = detail.stdout ? `\n${detail.stdout}` : '';
      return `${command}${output}`;
    }

    function formatMessage(value) {
      return escapeHtml(value)
        .replace(/(@[a-zA-Z][a-zA-Z0-9_-]*)/g, '<span class="mention">$1</span>')
        .replace(/(`[^`]+`)/g, '<span class="code-token">$1</span>');
    }

    document.getElementById('message-form').addEventListener('submit', async (event) => {
      event.preventDefault();
      const textarea = document.getElementById('message');
      const message = textarea.value.trim();
      if (!message) return;
      textarea.value = '';
      await fetch('/api/messages', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ message }) });
      await loadState();
    });

    document.getElementById('chat-view-button').addEventListener('click', () => {
      activeView = 'chat';
      renderHeader();
    });

    document.getElementById('dag-view-button').addEventListener('click', () => {
      activeView = 'dag';
      renderHeader();
    });

    document.getElementById('notification-count').addEventListener('click', (event) => {
      event.stopPropagation();
      document.getElementById('notification-panel').classList.toggle('open');
    });

    document.getElementById('message').addEventListener('keydown', (event) => {
      if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
        event.preventDefault();
        document.getElementById('message-form').requestSubmit();
      }
    });

    function listHtml(items) {
      if (!items.length) return '<span class="empty">Not known yet.</span>';
      return `<ul class="compact-list">${items.map(item => `<li>${escapeHtml(item)}</li>`).join('')}</ul>`;
    }

    function artefactsHtml(artefacts) {
      if (!artefacts.length) return '<div class="empty">No artefact linked to this step yet.</div>';
      return artefacts.map(artefact => `<div class="memory"><div class="meta">${escapeHtml(artefact.title)} · ${escapeHtml(artefact.producer_role)}</div><pre>${escapeHtml(JSON.stringify(artefact.content, null, 2))}</pre></div>`).join('');
    }

    function eventsHtml(events) {
      if (!events.length) return '<div class="empty">No logs linked to this step yet.</div>';
      return `<ul class="compact-list">${events.map(event => `<li><strong>${escapeHtml(event.variant)}</strong> ${escapeHtml(event.summary)}</li>`).join('')}</ul>`;
    }

    function memoriesHtml(memories) {
      if (!memories.length) return '<div class="empty">No memory candidates yet.</div>';
      return memories.slice().reverse().slice(0, 4).map(memory => `<div class="memory"><div class="meta">${escapeHtml(memory.status)} · ${escapeHtml(memory.scope)} · ${escapeHtml(memory.memory_type)}</div><div>${escapeHtml(memory.content)}</div></div>`).join('');
    }

    function escapeHtml(value) {
      return String(value).replace(/[&<>'"]/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '\'': '&#39;', '"': '&quot;' }[char]));
    }

    loadState();
    connectEvents();
  </script>
</body>
</html>"#;
