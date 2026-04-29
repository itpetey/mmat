use std::{
    collections::{BTreeMap, VecDeque},
    env, fs,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{oneshot, watch};

use crate::{
    deliver::{BuildJob, BuildJobStatus},
    liveview::event::{FrontendEvent, RunSummaryEvent},
    project::{NewProject, ProjectConfig, ProjectId, ProjectRegistryStore},
};

const EVENT_HISTORY_CAP: usize = 256;
const CONVERSATION_STORE_ENV: &str = "MMAT_CONVERSATION_SQLITE_PATH";
const DATA_DIR_ENV: &str = "MMAT_DATA_DIR";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UiSnapshot {
    pub projects: Vec<ProjectConfig>,
    pub active_project: ProjectConfig,
    pub history: VecDeque<UiEventEntry>,
    pub conversation: VecDeque<ConversationEntry>,
    pub pending_prompt: Option<PendingPromptSnapshot>,
    pub composer_mode: ComposerMode,
    pub run_summary: Option<RunSummary>,
    pub queue: Vec<BuildJobSnapshot>,
    pub worker_summary: Vec<ProjectWorkerSnapshot>,
}

#[derive(Debug)]
pub struct UiState {
    projects: Mutex<Vec<ProjectConfig>>,
    active_project_id: Mutex<ProjectId>,
    project_states: Mutex<BTreeMap<ProjectId, ProjectUiState>>,
    registry_store: Option<Arc<ProjectRegistryStore>>,
    conversation_store: Option<Arc<ConversationHistoryStore>>,
    pending_initial_input: Mutex<Option<oneshot::Sender<ProjectPrompt>>>,
    next_event_id: Mutex<u64>,
    version_tx: watch::Sender<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UiEventEntry {
    pub id: u64,
    pub event: UiEvent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationEntry {
    UserMessage { text: String },
    AssistantQuestion { question: String },
    AssistantReasoning { text: String, complete: bool },
    AssistantMessage { text: String, complete: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ComposerMode {
    InitialPrompt,
    Reply,
    Working,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectPrompt {
    pub project_id: ProjectId,
    pub prompt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum UiEvent {
    Log { level: String, message: String },
    StepStarted { task_label: String },
    StepCompleted { task_label: String, attempts: usize },
    StepFailed { task_label: String, stage: String },
    ComponentStarted { component: String, name: String },
    ComponentCompleted { component: String, name: String },
    ComponentFailed { component: String, name: String },
}

#[derive(Debug)]
pub struct PendingPrompt {
    pub question: String,
    pub choices: Option<Vec<String>>,
    pub reply: oneshot::Sender<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BuildJobSnapshot {
    pub id: String,
    pub status: String,
    pub prompt: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProjectWorkerSnapshot {
    pub project_id: ProjectId,
    pub project_name: String,
    pub pending: usize,
    pub running: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PendingPromptSnapshot {
    pub question: String,
    pub choices: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunSummary {
    pub project_id: ProjectId,
    pub run_id: String,
    pub project_root: String,
    pub run_root: String,
    pub prompt: String,
    pub status: String,
    pub current_stage: String,
    pub next_step: Option<String>,
}

#[derive(Debug, Default)]
struct ProjectUiState {
    event_history: VecDeque<UiEventEntry>,
    conversation_history: VecDeque<ConversationEntry>,
    pending_prompt: Option<PendingPrompt>,
    run_summary: Option<RunSummary>,
    queue: Vec<BuildJobSnapshot>,
}

#[derive(Debug, Error)]
pub enum ConversationHistoryError {
    #[error("conversation history failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("conversation history io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("conversation history serialisation failed: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct ConversationHistoryStore {
    path: PathBuf,
}

impl ConversationHistoryStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ConversationHistoryError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let store = Self { path };
        store.initialise()?;
        Ok(store)
    }

    pub fn open_default() -> Result<Self, ConversationHistoryError> {
        Self::open(default_conversation_history_path()?)
    }

    pub fn load(
        &self,
        project_id: &ProjectId,
    ) -> Result<VecDeque<ConversationEntry>, ConversationHistoryError> {
        let Some(entries_json) = self
            .connection()?
            .query_row(
                "SELECT entries_json FROM project_conversations WHERE project_id = ?1",
                [project_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        else {
            return Ok(VecDeque::new());
        };

        serde_json::from_str(&entries_json).map_err(ConversationHistoryError::from)
    }

    pub fn save(
        &self,
        project_id: &ProjectId,
        entries: &VecDeque<ConversationEntry>,
    ) -> Result<(), ConversationHistoryError> {
        self.connection()?.execute(
            "INSERT INTO project_conversations (project_id, entries_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(project_id) DO UPDATE SET
                 entries_json = excluded.entries_json,
                 updated_at = excluded.updated_at",
            params![
                project_id.as_str(),
                serde_json::to_string(entries)?,
                now_unix_seconds(),
            ],
        )?;
        Ok(())
    }

    fn initialise(&self) -> Result<(), ConversationHistoryError> {
        self.connection()?.execute_batch(
            "CREATE TABLE IF NOT EXISTS project_conversations (
                 project_id TEXT PRIMARY KEY NOT NULL,
                 entries_json TEXT NOT NULL,
                 updated_at INTEGER NOT NULL
             );",
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection, ConversationHistoryError> {
        Ok(Connection::open(&self.path)?)
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        let default_project = ProjectConfig::default_for_root(
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        )
        .expect("default project config should be valid");
        Self::with_projects(vec![default_project], None)
    }

    pub fn with_projects(
        projects: Vec<ProjectConfig>,
        registry_store: Option<Arc<ProjectRegistryStore>>,
    ) -> Self {
        Self::with_projects_and_conversation_store(projects, registry_store, None)
    }

    pub fn with_projects_and_conversation_store(
        projects: Vec<ProjectConfig>,
        registry_store: Option<Arc<ProjectRegistryStore>>,
        conversation_store: Option<Arc<ConversationHistoryStore>>,
    ) -> Self {
        let (version_tx, _version_rx) = watch::channel(0);
        let projects = if projects.is_empty() {
            vec![
                ProjectConfig::default_for_root(
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                )
                .expect("default project config should be valid"),
            ]
        } else {
            projects
        };
        let active_project_id = projects
            .first()
            .expect("projects should not be empty")
            .id
            .clone();
        let project_states = projects
            .iter()
            .map(|project| {
                let conversation_history = conversation_store
                    .as_ref()
                    .map(|store| store.load(&project.id))
                    .transpose()
                    .unwrap_or_else(|error| {
                        tracing::warn!(
                            project_id = %project.id,
                            %error,
                            "failed to load conversation history"
                        );
                        None
                    })
                    .unwrap_or_default();

                (
                    project.id.clone(),
                    ProjectUiState {
                        conversation_history,
                        ..ProjectUiState::default()
                    },
                )
            })
            .collect();

        Self {
            projects: Mutex::new(projects),
            active_project_id: Mutex::new(active_project_id),
            project_states: Mutex::new(project_states),
            registry_store,
            conversation_store,
            pending_initial_input: Mutex::new(None),
            next_event_id: Mutex::new(0),
            version_tx,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.version_tx.subscribe()
    }

    pub fn prepare_initial_input(&self, sender: oneshot::Sender<ProjectPrompt>) {
        *self.pending_initial_input.lock() = Some(sender);
        self.bump_version();
    }

    pub fn bump_version(&self) {
        let next = self.version_tx.borrow().saturating_add(1);
        let _ = self.version_tx.send(next);
    }

    pub fn push_event(&self, event: UiEvent) {
        let project_id = self.active_project_id.lock().clone();
        self.push_project_event(&project_id, event);
    }

    pub fn push_project_event(&self, project_id: &ProjectId, event: UiEvent) {
        let mut next_event_id = self.next_event_id.lock();
        let event_id = *next_event_id;
        *next_event_id += 1;
        drop(next_event_id);

        let mut states = self.project_states.lock();
        let state = states.entry(project_id.clone()).or_default();
        if state.event_history.len() >= EVENT_HISTORY_CAP {
            state.event_history.pop_front();
        }
        state.event_history.push_back(UiEventEntry {
            id: event_id,
            event,
        });
        drop(states);
        self.bump_version();
    }

    pub fn set_run_summary(&self, summary: RunSummary) {
        self.set_project_run_summary(summary.project_id.clone(), summary);
    }

    pub fn set_project_run_summary(&self, project_id: ProjectId, summary: RunSummary) {
        self.project_states
            .lock()
            .entry(project_id)
            .or_default()
            .run_summary = Some(summary);
        self.bump_version();
    }

    pub fn record_user_message(&self, text: String) {
        let project_id = self.active_project_id.lock().clone();
        self.record_project_user_message(&project_id, text);
    }

    pub fn record_project_user_message(&self, project_id: &ProjectId, text: String) {
        self.push_project_conversation_entry(project_id, ConversationEntry::UserMessage { text });
    }

    pub fn record_assistant_message_delta(&self, delta: &str) {
        let project_id = self.active_project_id.lock().clone();
        self.record_project_assistant_message_delta(&project_id, delta);
    }

    pub fn record_project_assistant_message_delta(&self, project_id: &ProjectId, delta: &str) {
        if delta.is_empty() {
            return;
        }

        let trimmed = delta.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('{') || trimmed.starts_with('[') {
            return;
        }

        let mut states = self.project_states.lock();
        let state = states.entry(project_id.clone()).or_default();
        let conv = &mut state.conversation_history;
        if let Some(ConversationEntry::AssistantMessage {
            text,
            complete: false,
        }) = conv.back_mut()
        {
            text.push_str(delta);
        } else {
            Self::push_conversation_entry_locked(
                conv,
                ConversationEntry::AssistantMessage {
                    text: delta.to_string(),
                    complete: false,
                },
            );
        }
        let conversation = conv.clone();
        drop(states);
        self.persist_project_conversation(project_id, &conversation);
        self.bump_version();
    }

    pub fn record_assistant_reasoning_delta(&self, delta: &str) {
        let project_id = self.active_project_id.lock().clone();
        self.record_project_assistant_reasoning_delta(&project_id, delta);
    }

    pub fn record_project_assistant_reasoning_delta(&self, project_id: &ProjectId, delta: &str) {
        if delta.is_empty() {
            return;
        }

        let mut states = self.project_states.lock();
        let state = states.entry(project_id.clone()).or_default();
        let conv = &mut state.conversation_history;
        if let Some(ConversationEntry::AssistantReasoning {
            text,
            complete: false,
        }) = conv.back_mut()
        {
            text.push_str(delta);
        } else {
            Self::push_conversation_entry_locked(
                conv,
                ConversationEntry::AssistantReasoning {
                    text: delta.to_string(),
                    complete: false,
                },
            );
        }
        let conversation = conv.clone();
        drop(states);
        self.persist_project_conversation(project_id, &conversation);
        self.bump_version();
    }

    pub fn finish_assistant_reasoning(&self) {
        let project_id = self.active_project_id.lock().clone();
        self.finish_project_assistant_reasoning(&project_id);
    }

    pub fn finish_project_assistant_reasoning(&self, project_id: &ProjectId) {
        let mut states = self.project_states.lock();
        let state = states.entry(project_id.clone()).or_default();
        let conv = &mut state.conversation_history;
        let mut changed = false;

        if let Some(index) = conv.iter().rposition(|entry| {
            matches!(
                entry,
                ConversationEntry::AssistantReasoning {
                    complete: false,
                    ..
                }
            )
        }) {
            let remove_entry = matches!(
                conv.get(index),
                Some(ConversationEntry::AssistantReasoning {
                    text,
                    complete: false,
                }) if text.trim().is_empty()
            );

            if remove_entry {
                conv.remove(index);
                changed = true;
            } else if let Some(ConversationEntry::AssistantReasoning { complete, .. }) =
                conv.get_mut(index)
                && !*complete
            {
                *complete = true;
                changed = true;
            }
        }

        if let Some(index) = conv.iter().rposition(|entry| {
            matches!(
                entry,
                ConversationEntry::AssistantMessage {
                    complete: false,
                    ..
                }
            )
        }) && let Some(ConversationEntry::AssistantMessage { complete, .. }) =
            conv.get_mut(index)
            && !*complete
        {
            *complete = true;
            changed = true;
        }

        let conversation = changed.then(|| conv.clone());
        drop(states);
        if changed {
            if let Some(conversation) = conversation {
                self.persist_project_conversation(project_id, &conversation);
            }
            self.bump_version();
        }
    }

    pub fn record_assistant_message(&self, text: String) {
        let project_id = self.active_project_id.lock().clone();
        self.record_project_assistant_message(&project_id, text);
    }

    pub fn record_project_assistant_message(&self, project_id: &ProjectId, text: String) {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with('{') || trimmed.starts_with('[') {
            if let Some(conversation) =
                self.finish_or_remove_project_incomplete_assistant_message(project_id)
            {
                self.persist_project_conversation(project_id, &conversation);
                self.bump_version();
            }
            return;
        }

        let mut states = self.project_states.lock();
        let state = states.entry(project_id.clone()).or_default();
        let conv = &mut state.conversation_history;
        if let Some(ConversationEntry::AssistantMessage {
            text: existing,
            complete,
        }) = conv.back_mut()
            && !*complete
        {
            *existing = text;
            *complete = true;
        } else {
            Self::push_conversation_entry_locked(
                conv,
                ConversationEntry::AssistantMessage {
                    text,
                    complete: true,
                },
            );
        }
        let conversation = conv.clone();
        drop(states);
        self.persist_project_conversation(project_id, &conversation);
        self.bump_version();
    }

    fn finish_or_remove_project_incomplete_assistant_message(
        &self,
        project_id: &ProjectId,
    ) -> Option<VecDeque<ConversationEntry>> {
        let mut states = self.project_states.lock();
        let state = states.entry(project_id.clone()).or_default();
        let conv = &mut state.conversation_history;
        let index = conv.iter().rposition(|entry| {
            matches!(
                entry,
                ConversationEntry::AssistantMessage {
                    complete: false,
                    ..
                }
            )
        })?;

        if matches!(
            conv.get(index),
            Some(ConversationEntry::AssistantMessage { text, .. })
                if Self::looks_like_structured_payload(text)
        ) {
            conv.remove(index);
            return Some(conv.clone());
        }

        if let Some(ConversationEntry::AssistantMessage { complete, .. }) = conv.get_mut(index) {
            *complete = true;
            return Some(conv.clone());
        }

        None
    }

    fn looks_like_structured_payload(text: &str) -> bool {
        let trimmed = text.trim_start();
        trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with('"')
    }

    pub fn set_pending_prompt(&self, prompt: Option<PendingPrompt>) {
        let project_id = self.active_project_id.lock().clone();
        self.set_project_pending_prompt(&project_id, prompt);
    }

    pub fn set_project_pending_prompt(
        &self,
        project_id: &ProjectId,
        prompt: Option<PendingPrompt>,
    ) {
        let question = prompt.as_ref().map(|p| p.question.clone());

        let mut states = self.project_states.lock();
        let state = states.entry(project_id.clone()).or_default();
        state.pending_prompt = prompt;

        if let Some(question) = question {
            let conv = &mut state.conversation_history;

            if let Some(ConversationEntry::AssistantReasoning { complete, .. }) = conv.back_mut()
                && !*complete
            {
                *complete = true;
            }

            Self::push_conversation_entry_locked(
                conv,
                ConversationEntry::AssistantQuestion { question },
            );
        }
        let conversation = state.conversation_history.clone();

        drop(states);
        self.persist_project_conversation(project_id, &conversation);
        self.bump_version();
    }

    pub fn send_initial_input(&self, text: String) -> bool {
        let mut pending = self.pending_initial_input.lock();
        if let Some(sender) = pending.take() {
            drop(pending);
            let project_id = self.active_project_id.lock().clone();
            self.record_project_user_message(&project_id, text.clone());
            let ok = sender
                .send(ProjectPrompt {
                    project_id,
                    prompt: text,
                })
                .is_ok();
            self.bump_version();
            ok
        } else {
            false
        }
    }

    pub fn send_pending_prompt(&self, text: String) -> bool {
        let project_id = self.active_project_id.lock().clone();
        let mut states = self.project_states.lock();
        let state = states.entry(project_id.clone()).or_default();
        if let Some(prompt) = state.pending_prompt.take() {
            drop(states);
            self.record_project_user_message(&project_id, text.clone());
            let ok = prompt.reply.send(text).is_ok();
            self.bump_version();
            ok
        } else {
            false
        }
    }

    pub fn snapshot(&self) -> UiSnapshot {
        let projects = self.projects.lock().clone();
        let active_project_id = self.active_project_id.lock().clone();
        let active_project = projects
            .iter()
            .find(|project| project.id == active_project_id)
            .cloned()
            .or_else(|| projects.first().cloned())
            .expect("ui state should have at least one project");
        let states = self.project_states.lock();
        let current = states.get(&active_project.id);
        let history = current
            .map(|state| state.event_history.clone())
            .unwrap_or_default();
        let conversation = current
            .map(|state| state.conversation_history.clone())
            .unwrap_or_default();
        let has_pending_input = self.pending_initial_input.lock().is_some();
        let pending_prompt = current
            .and_then(|state| state.pending_prompt.as_ref())
            .as_ref()
            .map(|p| PendingPromptSnapshot {
                question: p.question.clone(),
                choices: p.choices.clone(),
            });
        let has_pending_prompt = pending_prompt.is_some();
        let run_summary = current.and_then(|state| state.run_summary.clone());
        let queue = current.map(|state| state.queue.clone()).unwrap_or_default();
        let worker_summary = worker_summary(&projects, &states);
        drop(states);

        let composer_mode = if has_pending_input {
            ComposerMode::InitialPrompt
        } else if has_pending_prompt {
            ComposerMode::Reply
        } else {
            ComposerMode::Working
        };

        UiSnapshot {
            projects,
            active_project,
            history,
            conversation,
            pending_prompt,
            composer_mode,
            run_summary,
            queue,
            worker_summary,
        }
    }

    pub fn switch_project(&self, project_id: ProjectId) -> bool {
        if self
            .projects
            .lock()
            .iter()
            .any(|project| project.id == project_id)
        {
            *self.active_project_id.lock() = project_id;
            self.bump_version();
            true
        } else {
            false
        }
    }

    pub fn register_project(&self, name: String, root: String) -> Result<ProjectId, String> {
        let new_project = NewProject::from_root(name, root);
        let project = if let Some(store) = &self.registry_store {
            store
                .register_project(new_project)
                .map_err(|error| error.to_string())?
        } else {
            ProjectConfig::default_for_root(new_project.root).map_err(|error| error.to_string())?
        };
        let project_id = project.id.clone();
        self.projects.lock().push(project.clone());
        self.project_states
            .lock()
            .entry(project_id.clone())
            .or_default();
        self.persist_project_conversation(&project_id, &VecDeque::new());
        *self.active_project_id.lock() = project_id.clone();
        self.bump_version();
        Ok(project_id)
    }

    pub fn active_project(&self) -> ProjectConfig {
        let snapshot = self.snapshot();
        snapshot.active_project
    }

    pub fn set_project_queue(&self, project_id: &ProjectId, jobs: Vec<BuildJob>) {
        let queue = jobs.into_iter().map(BuildJobSnapshot::from).collect();
        self.project_states
            .lock()
            .entry(project_id.clone())
            .or_default()
            .queue = queue;
        self.bump_version();
    }

    fn push_project_conversation_entry(&self, project_id: &ProjectId, entry: ConversationEntry) {
        let mut states = self.project_states.lock();
        let state = states.entry(project_id.clone()).or_default();
        Self::push_conversation_entry_locked(&mut state.conversation_history, entry);
        let conversation = state.conversation_history.clone();
        drop(states);
        self.persist_project_conversation(project_id, &conversation);
        self.bump_version();
    }

    fn push_conversation_entry_locked(
        conv: &mut VecDeque<ConversationEntry>,
        entry: ConversationEntry,
    ) {
        if conv.len() >= EVENT_HISTORY_CAP {
            conv.pop_front();
        }
        conv.push_back(entry);
    }

    fn persist_project_conversation(
        &self,
        project_id: &ProjectId,
        conversation: &VecDeque<ConversationEntry>,
    ) {
        if let Some(store) = &self.conversation_store
            && let Err(error) = store.save(project_id, conversation)
        {
            tracing::warn!(%project_id, %error, "failed to save conversation history");
        }
    }
}

fn default_conversation_history_path() -> Result<PathBuf, ConversationHistoryError> {
    if let Some(path) = env::var(CONVERSATION_STORE_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(PathBuf::from(path));
    }

    if let Some(base) = env::var(DATA_DIR_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(PathBuf::from(base).join("conversations.sqlite3"));
    }

    Ok(env::current_dir()?
        .join(".mmat")
        .join("conversations.sqlite3"))
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

impl From<RunSummaryEvent> for RunSummary {
    fn from(value: RunSummaryEvent) -> Self {
        Self {
            project_id: value.project_id,
            run_id: value.run_id,
            project_root: value.project_root,
            run_root: value.run_root,
            prompt: value.prompt,
            status: value.status,
            current_stage: value.current_stage,
            next_step: value.next_step,
        }
    }
}

impl From<&FrontendEvent> for UiEvent {
    fn from(event: &FrontendEvent) -> Self {
        match event {
            FrontendEvent::ProjectScoped { event, .. } => UiEvent::from(event.as_ref()),
            FrontendEvent::StepStarted { task_label } => Self::StepStarted {
                task_label: task_label.clone(),
            },
            FrontendEvent::StepCompleted {
                task_label,
                attempts,
            } => Self::StepCompleted {
                task_label: task_label.clone(),
                attempts: *attempts,
            },
            FrontendEvent::StepFailed { task_label, stage } => Self::StepFailed {
                task_label: task_label.clone(),
                stage: stage.clone(),
            },
            FrontendEvent::ComponentStarted { component, name } => Self::ComponentStarted {
                component: component.clone(),
                name: name.clone(),
            },
            FrontendEvent::ComponentCompleted { component, name } => Self::ComponentCompleted {
                component: component.clone(),
                name: name.clone(),
            },
            FrontendEvent::ComponentFailed { component, name } => Self::ComponentFailed {
                component: component.clone(),
                name: name.clone(),
            },
            FrontendEvent::Log { level, message, .. } => Self::Log {
                level: level.to_string(),
                message: message.clone(),
            },
            FrontendEvent::StepAttemptStarted { .. }
            | FrontendEvent::StepAttemptValidated { .. }
            | FrontendEvent::StepRepairStarted { .. }
            | FrontendEvent::StepRejected { .. }
            | FrontendEvent::AssistantReasoningDelta { .. }
            | FrontendEvent::AssistantMessageDelta { .. }
            | FrontendEvent::AssistantResponseCompleted { .. }
            | FrontendEvent::HumanPrompt { .. }
            | FrontendEvent::RunSummary(_)
            | FrontendEvent::Quit => Self::Log {
                level: "INFO".to_string(),
                message: event.to_string(),
            },
        }
    }
}

impl From<BuildJob> for BuildJobSnapshot {
    fn from(value: BuildJob) -> Self {
        Self {
            id: value.id.to_string(),
            status: value.status.as_str().to_string(),
            prompt: value.handoff.prompt,
            error: value.error,
        }
    }
}

fn worker_summary(
    projects: &[ProjectConfig],
    states: &BTreeMap<ProjectId, ProjectUiState>,
) -> Vec<ProjectWorkerSnapshot> {
    projects
        .iter()
        .filter_map(|project| {
            let queue = &states.get(&project.id)?.queue;
            let pending = queue
                .iter()
                .filter(|job| job.status == BuildJobStatus::Pending.as_str())
                .count();
            let running = queue
                .iter()
                .filter(|job| job.status == BuildJobStatus::Running.as_str())
                .count();
            let failed = queue
                .iter()
                .filter(|job| job.status == BuildJobStatus::Failed.as_str())
                .count();
            (pending + running + failed > 0).then(|| ProjectWorkerSnapshot {
                project_id: project.id.clone(),
                project_name: project.name.clone(),
                pending,
                running,
                failed,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::oneshot;

    use crate::{
        deliver::{BuildJob, BuildJobId, BuildJobStatus},
        plan::DesignHandoff,
        project::{ProjectConfig, ProjectId},
    };

    use super::{
        ComposerMode, ConversationEntry, ConversationHistoryStore, PendingPrompt, UiEvent, UiState,
    };

    #[test]
    fn initial_input_resolves_and_records_user_message() {
        let state = UiState::new();
        let (tx, mut rx) = oneshot::channel();

        state.prepare_initial_input(tx);

        assert_eq!(state.snapshot().composer_mode, ComposerMode::InitialPrompt);
        assert!(state.send_initial_input("Build a tool".to_string()));
        assert_eq!(
            rx.try_recv().expect("reply should be sent").prompt,
            "Build a tool"
        );

        let snapshot = state.snapshot();
        assert_eq!(snapshot.composer_mode, ComposerMode::Working);
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::UserMessage { text }) if text == "Build a tool"
        ));
    }

    #[test]
    fn pending_prompt_resolves_and_records_reply() {
        let state = UiState::new();
        let (reply, mut rx) = oneshot::channel();

        state.set_pending_prompt(Some(PendingPrompt {
            question: "Which path?".to_string(),
            choices: Some(vec!["Conservative".to_string(), "Ambitious".to_string()]),
            reply,
        }));

        let snapshot = state.snapshot();
        assert_eq!(snapshot.composer_mode, ComposerMode::Reply);
        assert_eq!(
            snapshot
                .pending_prompt
                .as_ref()
                .map(|p| p.question.as_str()),
            Some("Which path?")
        );

        assert!(state.send_pending_prompt("Ambitious".to_string()));
        assert_eq!(rx.try_recv().expect("reply should be sent"), "Ambitious");
        assert_eq!(state.snapshot().composer_mode, ComposerMode::Working);
    }

    #[test]
    fn event_history_is_capped() {
        let state = UiState::new();

        for index in 0..300 {
            state.push_event(UiEvent::Log {
                level: "INFO".to_string(),
                message: format!("event {index}"),
            });
        }

        let snapshot = state.snapshot();
        assert_eq!(snapshot.history.len(), 256);
        assert_eq!(snapshot.history.front().map(|entry| entry.id), Some(44));
    }

    #[test]
    fn assistant_reasoning_and_message_deltas_accumulate() {
        let state = UiState::new();

        state.record_assistant_reasoning_delta("Thinking ");
        state.record_assistant_reasoning_delta("more");
        state.record_assistant_message_delta("Answer ");
        state.record_assistant_message_delta("now");
        state.finish_assistant_reasoning();

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantReasoning { text, complete: true })
                if text == "Thinking more"
        ));
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantMessage { text, complete: true })
                if text == "Answer now"
        ));
    }

    #[test]
    fn json_final_message_removes_streamed_assistant_delta() {
        let state = UiState::new();

        state.record_assistant_message_delta("\"decision\"");
        state.record_assistant_message("{\"decision\":\"approve\"}".to_string());

        assert!(state.snapshot().conversation.is_empty());
    }

    #[test]
    fn json_final_message_keeps_unstructured_streamed_text() {
        let state = UiState::new();

        state.record_assistant_message_delta("Inspecting repository");
        state.record_assistant_message("{\"decision\":\"approve\"}".to_string());

        let snapshot = state.snapshot();
        assert_eq!(snapshot.conversation.len(), 1);
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantMessage { text, complete: true })
                if text == "Inspecting repository"
        ));
    }

    #[test]
    fn json_final_message_keeps_completed_reasoning() {
        let state = UiState::new();

        state.record_assistant_reasoning_delta("Inspecting repository");
        state.record_assistant_message_delta("\"decision\"");
        state.record_assistant_message("{\"decision\":\"approve\"}".to_string());
        state.finish_assistant_reasoning();

        let snapshot = state.snapshot();
        assert_eq!(snapshot.conversation.len(), 1);
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantReasoning { text, complete: true })
                if text == "Inspecting repository"
        ));
    }

    #[test]
    fn switching_projects_changes_visible_conversation_and_queue() {
        let first = project("first");
        let second = project("second");
        let state = UiState::with_projects(vec![first.clone(), second.clone()], None);

        state.record_project_user_message(&first.id, "First prompt".to_string());
        state.record_project_user_message(&second.id, "Second prompt".to_string());
        state.set_project_queue(&first.id, vec![job(&first.id, "First build")]);
        state.set_project_queue(&second.id, vec![job(&second.id, "Second build")]);

        let first_snapshot = state.snapshot();
        assert!(matches!(
            first_snapshot.conversation.back(),
            Some(ConversationEntry::UserMessage { text }) if text == "First prompt"
        ));
        assert_eq!(first_snapshot.queue[0].prompt, "First build");

        assert!(state.switch_project(second.id.clone()));
        let second_snapshot = state.snapshot();
        assert!(matches!(
            second_snapshot.conversation.back(),
            Some(ConversationEntry::UserMessage { text }) if text == "Second prompt"
        ));
        assert_eq!(second_snapshot.queue[0].prompt, "Second build");
        assert!(
            second_snapshot
                .worker_summary
                .iter()
                .any(|worker| worker.project_id == first.id)
        );
    }

    #[test]
    fn conversation_history_restores_from_store() {
        let project = project("persisted");
        let path = std::env::temp_dir().join(format!(
            "mmat-conversation-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let store = Arc::new(
            ConversationHistoryStore::open(&path).expect("conversation store should open"),
        );
        let state =
            UiState::with_projects_and_conversation_store(vec![project.clone()], None, Some(store));

        state.record_project_user_message(&project.id, "Keep me".to_string());
        state.record_project_assistant_message(&project.id, "Still here".to_string());

        let restored_store = Arc::new(
            ConversationHistoryStore::open(&path).expect("conversation store should reopen"),
        );
        let restored = UiState::with_projects_and_conversation_store(
            vec![project],
            None,
            Some(restored_store),
        );
        let snapshot = restored.snapshot();

        assert_eq!(snapshot.conversation.len(), 2);
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::UserMessage { text }) if text == "Keep me"
        ));
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantMessage { text, complete: true }) if text == "Still here"
        ));
    }

    fn project(id: &str) -> ProjectConfig {
        let project_id = ProjectId::new(id).expect("project id should parse");
        let root = std::env::temp_dir().join(format!("mmat-ui-{id}"));
        ProjectConfig {
            id: project_id,
            name: id.to_string(),
            root: root.clone(),
            data_dir: crate::project::default_data_dir_for_root(&root),
            enabled: true,
            qdrant_collection_prefix: format!("p_{id}"),
            repo_label: Some(id.to_string()),
        }
    }

    fn job(project_id: &ProjectId, prompt: &str) -> BuildJob {
        BuildJob {
            id: BuildJobId::new(format!("job_{prompt}")),
            project_id: project_id.clone(),
            status: BuildJobStatus::Pending,
            handoff: DesignHandoff {
                design_run_id: uuid::Uuid::new_v4(),
                prompt: prompt.to_string(),
                architect_plan: serde_json::json!({"summary": prompt}).to_string(),
                knowledge_collections: Vec::new(),
            },
            error: None,
            created_at: 1,
            updated_at: 1,
            started_at: None,
            completed_at: None,
        }
    }
}
