use std::collections::VecDeque;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::oneshot;

use crate::models::RunSummary;
use crate::ws::event::FrontendEvent;

const EVENT_HISTORY_CAP: usize = 256;

#[derive(Debug)]
pub struct UiState {
    pub event_history: Mutex<VecDeque<UiEventEntry>>,
    pub conversation_history: Mutex<VecDeque<ConversationEntry>>,
    pub pending_initial_input: Mutex<Option<oneshot::Sender<String>>>,
    pub pending_prompt: Mutex<Option<PendingPrompt>>,
    pub run_summary: Mutex<Option<RunSummary>>,
    next_event_id: Mutex<u64>,
}

#[derive(Clone, Serialize)]
pub struct UiSnapshot {
    pub history: VecDeque<UiEventEntry>,
    pub conversation: VecDeque<ConversationEntry>,
    pub pending_prompt: Option<PendingPromptSnapshot>,
    pub composer_mode: ComposerMode,
    pub run_summary: Option<RunSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UiEventEntry {
    pub id: u64,
    pub event: UiEvent,
}

#[derive(Clone, Debug, Serialize)]
#[allow(dead_code)]
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

#[derive(Clone, Debug, PartialEq, Serialize)]
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

#[derive(Clone, Serialize)]
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
        Self {
            event_history: Mutex::new(VecDeque::with_capacity(EVENT_HISTORY_CAP)),
            conversation_history: Mutex::new(VecDeque::with_capacity(EVENT_HISTORY_CAP)),
            pending_initial_input: Mutex::new(None),
            pending_prompt: Mutex::new(None),
            run_summary: Mutex::new(None),
            next_event_id: Mutex::new(0),
        }
    }

    pub(crate) fn bump_version(&self) {}

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

    fn push_conversation_entry(&self, entry: ConversationEntry) {
        let mut conv = self.conversation_history.lock();
        if conv.len() >= EVENT_HISTORY_CAP {
            conv.pop_front();
        }
        conv.push_back(entry);
        drop(conv);
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

    #[allow(dead_code)]
    pub fn record_assistant_message(&self, text: String) {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with('{') || trimmed.starts_with('[') {
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

    fn push_conversation_entry_locked(
        conv: &mut VecDeque<ConversationEntry>,
        entry: ConversationEntry,
    ) {
        if conv.len() >= EVENT_HISTORY_CAP {
            conv.pop_front();
        }
        conv.push_back(entry);
    }

    pub fn set_pending_prompt(&self, prompt: Option<PendingPrompt>) {
        let question = prompt.as_ref().map(|p| p.question.clone());

        let mut pending = self.pending_prompt.lock();
        *pending = prompt;
        drop(pending);

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
            drop(conv);
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
        let has_pending_prompt = self.pending_prompt.lock().is_some();
        let pending_prompt = self
            .pending_prompt
            .lock()
            .as_ref()
            .map(|p| PendingPromptSnapshot {
                question: p.question.clone(),
                choices: p.choices.clone(),
            });
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
}

#[cfg(test)]
mod tests {
    use super::{ConversationEntry, UiState};
    use tokio::sync::oneshot;

    #[test]
    fn assistant_reasoning_deltas_accumulate_into_one_entry() {
        let state = UiState::new();

        state.record_assistant_reasoning_delta("First ");
        state.record_assistant_reasoning_delta("second");
        state.record_assistant_message_delta("Answer");

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantReasoning { text, complete: false })
                if text == "First second"
        ));
    }

    #[test]
    fn assistant_message_deltas_accumulate_into_one_entry() {
        let state = UiState::new();

        state.record_assistant_message_delta("First ");
        state.record_assistant_message_delta("second");

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantMessage { text, complete: false })
                if text == "First second"
        ));
    }

    #[test]
    fn assistant_message_completion_reuses_streamed_entry() {
        let state = UiState::new();

        state.record_assistant_message_delta("Ans");
        state.record_assistant_message("Answer".to_string());

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantMessage { text, complete: true })
                if text == "Answer"
        ));
    }

    #[test]
    fn assistant_message_closes_reasoning_entry() {
        let state = UiState::new();

        state.record_assistant_reasoning_delta("Thinking");
        state.record_assistant_message("Answer".to_string());
        state.finish_assistant_reasoning();

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantReasoning { text, complete: true })
                if text == "Thinking"
        ));
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantMessage { text, complete: true })
                if text == "Answer"
        ));
    }

    #[test]
    fn json_like_message_deltas_are_hidden() {
        let state = UiState::new();

        state.record_assistant_message_delta("  ");
        state.record_assistant_message_delta("{\"status\":\"ok\"}");

        let snapshot = state.snapshot();
        assert!(snapshot.conversation.is_empty());
    }

    #[test]
    fn finished_reasoning_without_visible_message_remains_visible() {
        let state = UiState::new();

        state.record_assistant_reasoning_delta("Thinking");
        state.finish_assistant_reasoning();

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantReasoning { text, complete: true })
                if text == "Thinking"
        ));
    }

    #[test]
    fn snapshot_shows_buffered_reasoning_while_turn_is_in_progress() {
        let state = UiState::new();

        state.record_assistant_reasoning_delta("Thinking");

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantReasoning { text, complete: false })
                if text == "Thinking"
        ));
    }

    #[test]
    fn pending_prompt_flushes_reasoning_before_question() {
        let state = UiState::new();
        let (reply, _rx) = oneshot::channel();

        state.record_assistant_reasoning_delta("Thinking");
        state.set_pending_prompt(Some(super::PendingPrompt {
            question: "What are we building?".to_string(),
            choices: None,
            reply,
        }));

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantReasoning { text, complete: true })
                if text == "Thinking"
        ));
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantQuestion { question })
                if question == "What are we building?"
        ));
    }

    #[test]
    fn finished_reasoning_is_preserved_until_prompt_becomes_visible() {
        let state = UiState::new();
        let (reply, _rx) = oneshot::channel();

        state.record_assistant_reasoning_delta("Thinking");
        state.finish_assistant_reasoning();
        state.set_pending_prompt(Some(super::PendingPrompt {
            question: "What are we building?".to_string(),
            choices: None,
            reply,
        }));

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantReasoning { text, complete: true })
                if text == "Thinking"
        ));
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantQuestion { question })
                if question == "What are we building?"
        ));
    }

    #[test]
    fn prompt_turn_finalises_reasoning_before_next_turn() {
        let state = UiState::new();
        let (reply, _rx) = oneshot::channel();

        state.record_assistant_reasoning_delta("Thinking");
        state.finish_assistant_reasoning();
        state.set_pending_prompt(Some(super::PendingPrompt {
            question: "What are we building?".to_string(),
            choices: None,
            reply,
        }));
        state.record_user_message("Build a CLI".to_string());
        state.record_assistant_reasoning_delta("Next thought");

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantReasoning { text, complete: true })
                if text == "Thinking"
        ));
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantReasoning { text, complete: false })
                if text == "Next thought"
        ));
    }

    #[test]
    fn new_reply_starts_new_assistant_entry() {
        let state = UiState::new();

        state.record_assistant_message_delta("First");
        state.record_assistant_message("First".to_string());
        state.record_user_message("Again".to_string());
        state.record_assistant_message_delta("Second");

        let snapshot = state.snapshot();
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantMessage { text, complete: true })
                if text == "First"
        ));
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantMessage { text, complete: false })
                if text == "Second"
        ));
    }

    #[test]
    fn final_only_reply_appends_instead_of_replacing_previous_message() {
        let state = UiState::new();

        state.record_assistant_message("First".to_string());
        state.record_assistant_message("Second".to_string());

        let snapshot = state.snapshot();
        assert_eq!(snapshot.conversation.len(), 2);
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantMessage { text, complete: true })
                if text == "First"
        ));
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantMessage { text, complete: true })
                if text == "Second"
        ));
    }

    #[test]
    fn finish_reasoning_completes_streamed_message_without_final_text_rewrite() {
        let state = UiState::new();

        state.record_assistant_message_delta("Draft answer");
        state.finish_assistant_reasoning();
        state.record_assistant_message_delta("Next answer");

        let snapshot = state.snapshot();
        assert_eq!(snapshot.conversation.len(), 2);
        assert!(matches!(
            snapshot.conversation.front(),
            Some(ConversationEntry::AssistantMessage { text, complete: true })
                if text == "Draft answer"
        ));
        assert!(matches!(
            snapshot.conversation.back(),
            Some(ConversationEntry::AssistantMessage { text, complete: false })
                if text == "Next answer"
        ));
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
