use std::collections::VecDeque;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::{oneshot, watch};

use crate::liveview::event::{FrontendEvent, RunSummaryEvent};

const EVENT_HISTORY_CAP: usize = 256;

#[derive(Debug)]
pub struct UiState {
    event_history: Mutex<VecDeque<UiEventEntry>>,
    conversation_history: Mutex<VecDeque<ConversationEntry>>,
    pending_initial_input: Mutex<Option<oneshot::Sender<String>>>,
    pending_prompt: Mutex<Option<PendingPrompt>>,
    run_summary: Mutex<Option<RunSummary>>,
    next_event_id: Mutex<u64>,
    version_tx: watch::Sender<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UiSnapshot {
    pub history: VecDeque<UiEventEntry>,
    pub conversation: VecDeque<ConversationEntry>,
    pub pending_prompt: Option<PendingPromptSnapshot>,
    pub composer_mode: ComposerMode,
    pub run_summary: Option<RunSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UiEventEntry {
    pub id: u64,
    pub event: UiEvent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
pub struct PendingPromptSnapshot {
    pub question: String,
    pub choices: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    pub project_root: String,
    pub run_root: String,
    pub prompt: String,
    pub status: String,
    pub current_stage: String,
    pub next_step: Option<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        let (version_tx, _version_rx) = watch::channel(0);
        Self {
            event_history: Mutex::new(VecDeque::with_capacity(EVENT_HISTORY_CAP)),
            conversation_history: Mutex::new(VecDeque::with_capacity(EVENT_HISTORY_CAP)),
            pending_initial_input: Mutex::new(None),
            pending_prompt: Mutex::new(None),
            run_summary: Mutex::new(None),
            next_event_id: Mutex::new(0),
            version_tx,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.version_tx.subscribe()
    }

    pub fn prepare_initial_input(&self, sender: oneshot::Sender<String>) {
        *self.pending_initial_input.lock() = Some(sender);
        self.bump_version();
    }

    pub fn bump_version(&self) {
        let next = self.version_tx.borrow().saturating_add(1);
        let _ = self.version_tx.send(next);
    }

    pub fn push_event(&self, event: UiEvent) {
        let mut next_event_id = self.next_event_id.lock();
        let event_id = *next_event_id;
        *next_event_id += 1;
        drop(next_event_id);

        let mut history = self.event_history.lock();
        if history.len() >= EVENT_HISTORY_CAP {
            history.pop_front();
        }
        history.push_back(UiEventEntry {
            id: event_id,
            event,
        });
        drop(history);
        self.bump_version();
    }

    pub fn set_run_summary(&self, summary: RunSummary) {
        *self.run_summary.lock() = Some(summary);
        self.bump_version();
    }

    pub fn record_user_message(&self, text: String) {
        self.push_conversation_entry(ConversationEntry::UserMessage { text });
    }

    pub fn record_assistant_message_delta(&self, delta: &str) {
        if delta.is_empty() {
            return;
        }

        let trimmed = delta.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('{') || trimmed.starts_with('[') {
            return;
        }

        let mut conv = self.conversation_history.lock();
        if let Some(ConversationEntry::AssistantMessage {
            text,
            complete: false,
        }) = conv.back_mut()
        {
            text.push_str(delta);
        } else {
            Self::push_conversation_entry_locked(
                &mut conv,
                ConversationEntry::AssistantMessage {
                    text: delta.to_string(),
                    complete: false,
                },
            );
        }
        drop(conv);
        self.bump_version();
    }

    pub fn record_assistant_reasoning_delta(&self, delta: &str) {
        if delta.is_empty() {
            return;
        }

        let mut conv = self.conversation_history.lock();
        if let Some(ConversationEntry::AssistantReasoning {
            text,
            complete: false,
        }) = conv.back_mut()
        {
            text.push_str(delta);
        } else {
            Self::push_conversation_entry_locked(
                &mut conv,
                ConversationEntry::AssistantReasoning {
                    text: delta.to_string(),
                    complete: false,
                },
            );
        }
        drop(conv);
        self.bump_version();
    }

    pub fn finish_assistant_reasoning(&self) {
        let mut conv = self.conversation_history.lock();
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

        drop(conv);
        if changed {
            self.bump_version();
        }
    }

    pub fn record_assistant_message(&self, text: String) {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with('{') || trimmed.starts_with('[') {
            let removed = self.remove_incomplete_assistant_message();
            if removed {
                self.bump_version();
            }
            return;
        }

        let mut conv = self.conversation_history.lock();
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
                &mut conv,
                ConversationEntry::AssistantMessage {
                    text,
                    complete: true,
                },
            );
        }
        drop(conv);
        self.bump_version();
    }

    fn remove_incomplete_assistant_message(&self) -> bool {
        let mut conv = self.conversation_history.lock();
        let Some(index) = conv.iter().rposition(|entry| {
            matches!(
                entry,
                ConversationEntry::AssistantMessage {
                    complete: false,
                    ..
                }
            )
        }) else {
            return false;
        };

        conv.remove(index);
        true
    }

    pub fn set_pending_prompt(&self, prompt: Option<PendingPrompt>) {
        let question = prompt.as_ref().map(|p| p.question.clone());

        *self.pending_prompt.lock() = prompt;

        if let Some(question) = question {
            let mut conv = self.conversation_history.lock();

            if let Some(ConversationEntry::AssistantReasoning { complete, .. }) = conv.back_mut()
                && !*complete
            {
                *complete = true;
            }

            Self::push_conversation_entry_locked(
                &mut conv,
                ConversationEntry::AssistantQuestion { question },
            );
        }

        self.bump_version();
    }

    pub fn send_initial_input(&self, text: String) -> bool {
        let mut pending = self.pending_initial_input.lock();
        if let Some(sender) = pending.take() {
            drop(pending);
            self.record_user_message(text.clone());
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
            self.record_user_message(text.clone());
            let ok = prompt.reply.send(text).is_ok();
            self.bump_version();
            ok
        } else {
            false
        }
    }

    pub fn snapshot(&self) -> UiSnapshot {
        let history = self.event_history.lock().clone();
        let conversation = self.conversation_history.lock().clone();
        let has_pending_input = self.pending_initial_input.lock().is_some();
        let pending_prompt = self
            .pending_prompt
            .lock()
            .as_ref()
            .map(|p| PendingPromptSnapshot {
                question: p.question.clone(),
                choices: p.choices.clone(),
            });
        let has_pending_prompt = pending_prompt.is_some();
        let run_summary = self.run_summary.lock().clone();

        let composer_mode = if has_pending_input {
            ComposerMode::InitialPrompt
        } else if has_pending_prompt {
            ComposerMode::Reply
        } else {
            ComposerMode::Working
        };

        UiSnapshot {
            history,
            conversation,
            pending_prompt,
            composer_mode,
            run_summary,
        }
    }

    fn push_conversation_entry(&self, entry: ConversationEntry) {
        let mut conv = self.conversation_history.lock();
        Self::push_conversation_entry_locked(&mut conv, entry);
        drop(conv);
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
}

impl From<RunSummaryEvent> for RunSummary {
    fn from(value: RunSummaryEvent) -> Self {
        Self {
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

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

    use super::{ComposerMode, ConversationEntry, PendingPrompt, UiEvent, UiState};

    #[test]
    fn initial_input_resolves_and_records_user_message() {
        let state = UiState::new();
        let (tx, mut rx) = oneshot::channel();

        state.prepare_initial_input(tx);

        assert_eq!(state.snapshot().composer_mode, ComposerMode::InitialPrompt);
        assert!(state.send_initial_input("Build a tool".to_string()));
        assert_eq!(rx.try_recv().expect("reply should be sent"), "Build a tool");

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
}
