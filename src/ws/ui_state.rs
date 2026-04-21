use std::collections::VecDeque;

use parking_lot::Mutex;
use tokio::sync::{oneshot, watch};

use crate::models::RunSummary;
use crate::ws::event::FrontendEvent;

const EVENT_HISTORY_CAP: usize = 256;

#[derive(Debug)]
pub struct UiState {
    pub event_history: Mutex<VecDeque<UiEvent>>,
    pub pending_initial_input: Mutex<Option<oneshot::Sender<String>>>,
    pub pending_prompt: Mutex<Option<PendingPrompt>>,
    pub run_summary: Mutex<Option<RunSummary>>,
    pub planning_started: Mutex<bool>,
    version: Mutex<u64>,
    version_tx: watch::Sender<u64>,
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct UiSnapshot {
    pub history: VecDeque<UiEvent>,
    pub has_pending_input: bool,
    pub pending_prompt: Option<PendingPromptSnapshot>,
    pub run_summary: Option<RunSummary>,
    pub planning_started: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum UiEvent {
    Log {
        level: String,
        target: String,
        message: String,
    },
    StepStarted {
        task_name: String,
        task_label: String,
    },
    StepCompleted {
        task_name: String,
        task_label: String,
        attempts: usize,
    },
    StepFailed {
        task_name: String,
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
    PlanningTriggered,
}

#[derive(Debug)]
pub struct PendingPrompt {
    pub question: String,
    pub choices: Option<Vec<String>>,
    pub reply: oneshot::Sender<String>,
}

#[derive(Clone)]
pub struct PendingPromptSnapshot {
    pub question: String,
    #[allow(dead_code)]
    pub choices: Option<Vec<String>>,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        let (tx, _rx) = watch::channel(0u64);
        Self {
            event_history: Mutex::new(VecDeque::with_capacity(EVENT_HISTORY_CAP)),
            pending_initial_input: Mutex::new(None),
            pending_prompt: Mutex::new(None),
            run_summary: Mutex::new(None),
            planning_started: Mutex::new(false),
            version: Mutex::new(0),
            version_tx: tx,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.version_tx.subscribe()
    }

    fn bump_version(&self) {
        let mut v = self.version.lock();
        *v += 1;
        let _ = self.version_tx.send(*v);
    }

    pub fn push_event(&self, event: UiEvent) {
        let mut history = self.event_history.lock();
        if history.len() >= EVENT_HISTORY_CAP {
            history.pop_front();
        }
        history.push_back(event);
        drop(history);
        self.bump_version();
    }

    pub fn set_pending_prompt(&self, prompt: Option<PendingPrompt>) {
        let mut pending = self.pending_prompt.lock();
        *pending = prompt;
        drop(pending);
        self.bump_version();
    }

    pub fn set_planning_started(&self) {
        *self.planning_started.lock() = true;
        self.bump_version();
    }

    pub fn snapshot(&self) -> UiSnapshot {
        let history = self.event_history.lock().clone();
        let has_pending_input = self.pending_initial_input.lock().is_some();
        let pending_prompt = self
            .pending_prompt
            .lock()
            .as_ref()
            .map(|p| PendingPromptSnapshot {
                question: p.question.clone(),
                choices: p.choices.clone(),
            });
        let run_summary = self.run_summary.lock().clone();
        let planning_started = *self.planning_started.lock();

        UiSnapshot {
            history,
            has_pending_input,
            pending_prompt,
            run_summary,
            planning_started,
        }
    }
}

impl From<&FrontendEvent> for UiEvent {
    fn from(event: &FrontendEvent) -> Self {
        match event {
            FrontendEvent::StepStarted {
                task_name,
                task_label,
            } => Self::StepStarted {
                task_name: task_name.clone(),
                task_label: task_label.clone(),
            },
            FrontendEvent::StepCompleted {
                task_name,
                task_label,
                attempts,
            } => Self::StepCompleted {
                task_name: task_name.clone(),
                task_label: task_label.clone(),
                attempts: *attempts,
            },
            FrontendEvent::StepFailed {
                task_name,
                task_label,
                stage,
            } => Self::StepFailed {
                task_name: task_name.clone(),
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
            FrontendEvent::Log {
                level,
                target,
                message,
            } => Self::Log {
                level: level.to_string(),
                target: target.clone(),
                message: message.clone(),
            },
            FrontendEvent::StepAttemptStarted { .. }
            | FrontendEvent::StepAttemptValidated { .. }
            | FrontendEvent::StepRepairStarted { .. }
            | FrontendEvent::StepRejected { .. }
            | FrontendEvent::HumanPrompt { .. }
            | FrontendEvent::Quit => Self::Log {
                level: "INFO".to_string(),
                target: "mmat::ui".to_string(),
                message: event.to_string(),
            },
        }
    }
}
