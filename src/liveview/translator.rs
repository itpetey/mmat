use std::sync::Arc;

use tokio::sync::mpsc;

use crate::liveview::{
    event::FrontendEvent,
    state::{PendingPrompt, RunSummary, UiEvent, UiState},
};

pub fn spawn_event_translator(
    mut event_rx: mpsc::UnboundedReceiver<FrontendEvent>,
    ui_state: Arc<UiState>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                FrontendEvent::StepStarted { task_label } => {
                    ui_state.push_event(UiEvent::StepStarted { task_label });
                }
                FrontendEvent::StepCompleted {
                    task_label,
                    attempts,
                } => {
                    ui_state.push_event(UiEvent::StepCompleted {
                        task_label,
                        attempts,
                    });
                }
                FrontendEvent::StepFailed { task_label, stage } => {
                    ui_state.push_event(UiEvent::StepFailed { task_label, stage });
                }
                FrontendEvent::ComponentStarted { component, name } => {
                    ui_state.push_event(UiEvent::ComponentStarted { component, name });
                }
                FrontendEvent::ComponentCompleted { component, name } => {
                    ui_state.push_event(UiEvent::ComponentCompleted { component, name });
                }
                FrontendEvent::ComponentFailed { component, name } => {
                    ui_state.push_event(UiEvent::ComponentFailed { component, name });
                }
                FrontendEvent::Log {
                    level,
                    target,
                    message,
                } => {
                    if should_store_raw_log(&target, level) {
                        ui_state.push_event(UiEvent::Log {
                            level: level.to_string(),
                            message,
                        });
                    }
                }
                FrontendEvent::AssistantReasoningDelta { delta } => {
                    ui_state.record_assistant_reasoning_delta(&delta);
                }
                FrontendEvent::AssistantMessageDelta { delta } => {
                    ui_state.record_assistant_message_delta(&delta);
                }
                FrontendEvent::AssistantResponseCompleted { message } => {
                    if let Some(message) = message {
                        ui_state.record_assistant_message(message);
                    }
                    ui_state.finish_assistant_reasoning();
                }
                FrontendEvent::HumanPrompt {
                    question,
                    choices,
                    reply,
                } => {
                    ui_state.set_pending_prompt(Some(PendingPrompt {
                        question,
                        choices: (!choices.is_empty()).then_some(choices),
                        reply,
                    }));
                }
                FrontendEvent::RunSummary(summary) => {
                    ui_state.set_run_summary(RunSummary::from(summary));
                }
                FrontendEvent::Quit => {
                    break;
                }
                FrontendEvent::StepAttemptStarted { .. }
                | FrontendEvent::StepAttemptValidated { .. }
                | FrontendEvent::StepRepairStarted { .. }
                | FrontendEvent::StepRejected { .. } => {}
            }
        }
    })
}

fn should_store_raw_log(target: &str, level: tracing::Level) -> bool {
    matches!(level, tracing::Level::WARN | tracing::Level::ERROR)
        || target == env!("CARGO_CRATE_NAME")
        || target.starts_with(concat!(env!("CARGO_CRATE_NAME"), "::"))
        || target.starts_with("naaf_")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::mpsc;

    use crate::liveview::{FrontendEvent, UiEvent, UiState, spawn_event_translator};

    #[tokio::test]
    async fn translator_filters_raw_logs() {
        let state = Arc::new(UiState::new());
        let (tx, rx) = mpsc::unbounded_channel();
        let translator = spawn_event_translator(rx, state.clone());

        tx.send(FrontendEvent::Log {
            level: tracing::Level::INFO,
            target: "dependency".to_string(),
            message: "hidden".to_string(),
        })
        .expect("translator should receive");
        tx.send(FrontendEvent::Log {
            level: tracing::Level::WARN,
            target: "dependency".to_string(),
            message: "visible".to_string(),
        })
        .expect("translator should receive");
        tx.send(FrontendEvent::Log {
            level: tracing::Level::INFO,
            target: "naaf_core::step".to_string(),
            message: "workflow visible".to_string(),
        })
        .expect("translator should receive");
        tx.send(FrontendEvent::Quit)
            .expect("translator should receive quit");

        translator.await.expect("translator task should join");
        let snapshot = state.snapshot();
        assert_eq!(snapshot.history.len(), 2);
        assert!(matches!(
            snapshot.history.front().map(|entry| &entry.event),
            Some(UiEvent::Log { message, .. }) if message == "visible"
        ));
        assert!(matches!(
            snapshot.history.back().map(|entry| &entry.event),
            Some(UiEvent::Log { message, .. }) if message == "workflow visible"
        ));
    }

    #[tokio::test]
    async fn translator_sets_pending_prompt() {
        let state = Arc::new(UiState::new());
        let (tx, rx) = mpsc::unbounded_channel();
        let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();
        let translator = spawn_event_translator(rx, state.clone());

        tx.send(FrontendEvent::HumanPrompt {
            question: "Continue?".to_string(),
            choices: vec!["Yes".to_string()],
            reply: reply_tx,
        })
        .expect("translator should receive");
        tx.send(FrontendEvent::Quit)
            .expect("translator should receive quit");

        translator.await.expect("translator task should join");
        let snapshot = state.snapshot();
        assert_eq!(
            snapshot
                .pending_prompt
                .as_ref()
                .map(|p| p.question.as_str()),
            Some("Continue?")
        );
    }

    #[tokio::test]
    async fn translator_records_assistant_stream() {
        let state = Arc::new(UiState::new());
        let (tx, rx) = mpsc::unbounded_channel();
        let translator = spawn_event_translator(rx, state.clone());

        tx.send(FrontendEvent::AssistantReasoningDelta {
            delta: "Plan ".to_string(),
        })
        .expect("translator should receive");
        tx.send(FrontendEvent::AssistantReasoningDelta {
            delta: "first".to_string(),
        })
        .expect("translator should receive");
        tx.send(FrontendEvent::AssistantMessageDelta {
            delta: "Done".to_string(),
        })
        .expect("translator should receive");
        tx.send(FrontendEvent::AssistantResponseCompleted {
            message: Some("Done".to_string()),
        })
        .expect("translator should receive");
        tx.send(FrontendEvent::Quit)
            .expect("translator should receive quit");

        translator.await.expect("translator task should join");
        let snapshot = state.snapshot();
        assert_eq!(snapshot.conversation.len(), 2);
        assert!(matches!(
            snapshot.conversation.front(),
            Some(crate::liveview::ConversationEntry::AssistantReasoning {
                text,
                complete: true
            }) if text == "Plan first"
        ));
        assert!(matches!(
            snapshot.conversation.back(),
            Some(crate::liveview::ConversationEntry::AssistantMessage {
                text,
                complete: true
            }) if text == "Done"
        ));
    }
}
