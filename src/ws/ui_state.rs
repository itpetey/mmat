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

#[derive(Clone)]
pub struct UiSnapshot {
    pub history: VecDeque<UiEvent>,
    pub has_pending_input: bool,
    pub pending_prompt: Option<PendingPromptSnapshot>,
}

#[derive(Clone, Debug)]
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

#[derive(Clone)]
pub struct PendingPromptSnapshot {
    pub question: String,
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

    pub(crate) fn bump_version(&self) {
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

    pub fn send_initial_input(&self, text: String) -> bool {
        let mut pending = self.pending_initial_input.lock();
        if let Some(sender) = pending.take() {
            drop(pending);
            let ok = sender.send(text).is_ok();
            self.bump_version();
            ok
        } else {
            false
        }
    }

    pub fn send_pending_prompt(&self, text: String) -> bool {
        let mut pending = self.pending_prompt.lock();
        if let Some(prompt) = pending.take() {
            drop(pending);
            let ok = prompt.reply.send(text).is_ok();
            self.bump_version();
            ok
        } else {
            false
        }
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

        UiSnapshot {
            history,
            has_pending_input,
            pending_prompt,
        }
    }
}

impl From<&FrontendEvent> for UiEvent {
    fn from(event: &FrontendEvent) -> Self {
        match event {
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
            | FrontendEvent::HumanPrompt { .. }
            | FrontendEvent::Quit => Self::Log {
                level: "INFO".to_string(),
                message: event.to_string(),
            },
        }
    }
}
