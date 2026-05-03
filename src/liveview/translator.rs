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
            if matches!(event, FrontendEvent::Quit) {
                break;
            }
            match event {
                FrontendEvent::ProjectScoped { project_id, event } => {
                    dispatch_event(Some(project_id), *event, ui_state.as_ref());
                }
                event => dispatch_event(None, event, ui_state.as_ref()),
            }
        }
    })
}

fn dispatch_event(
    project_id: Option<crate::project::ProjectId>,
    event: FrontendEvent,
    ui_state: &UiState,
) {
    match event {
        FrontendEvent::StepStarted { task_label } => {
            push_event(
                ui_state,
                project_id.as_ref(),
                UiEvent::StepStarted { task_label },
            );
        }
        FrontendEvent::StepCompleted {
            task_label,
            attempts,
        } => {
            push_event(
                ui_state,
                project_id.as_ref(),
                UiEvent::StepCompleted {
                    task_label,
                    attempts,
                },
            );
        }
        FrontendEvent::StepFailed { task_label, stage } => {
            push_event(
                ui_state,
                project_id.as_ref(),
                UiEvent::StepFailed { task_label, stage },
            );
        }
        FrontendEvent::ComponentStarted { component, name } => {
            push_event(
                ui_state,
                project_id.as_ref(),
                UiEvent::ComponentStarted { component, name },
            );
        }
        FrontendEvent::ComponentCompleted { component, name } => {
            push_event(
                ui_state,
                project_id.as_ref(),
                UiEvent::ComponentCompleted { component, name },
            );
        }
        FrontendEvent::ComponentFailed { component, name } => {
            push_event(
                ui_state,
                project_id.as_ref(),
                UiEvent::ComponentFailed { component, name },
            );
        }
        FrontendEvent::Log {
            level,
            target,
            message,
        } => {
            if should_store_raw_log(&target, level) {
                push_event(
                    ui_state,
                    project_id.as_ref(),
                    UiEvent::Log {
                        level: level.to_string(),
                        message,
                    },
                );
            }
        }
        FrontendEvent::AssistantReasoningDelta { delta } => {
            if let Some(project_id) = &project_id {
                ui_state.record_project_assistant_reasoning_delta(project_id, &delta);
            } else {
                ui_state.record_assistant_reasoning_delta(&delta);
            }
        }
        FrontendEvent::AssistantMessageDelta { delta } => {
            if let Some(project_id) = &project_id {
                ui_state.record_project_assistant_message_delta(project_id, &delta);
            } else {
                ui_state.record_assistant_message_delta(&delta);
            }
        }
        FrontendEvent::AssistantResponseCompleted { message } => {
            if let Some(message) = message {
                if let Some(project_id) = &project_id {
                    ui_state.record_project_assistant_message(project_id, message);
                } else {
                    ui_state.record_assistant_message(message);
                }
            }
            if let Some(project_id) = &project_id {
                ui_state.finish_project_assistant_reasoning(project_id);
            } else {
                ui_state.finish_assistant_reasoning();
            }
        }
        FrontendEvent::DomainNodeAssistantMessageDelta { node_id, delta } => {
            if let Ok(node_id) = node_id.parse() {
                if let Some(project_id) = &project_id {
                    ui_state
                        .record_project_domain_assistant_message_delta(project_id, node_id, &delta);
                } else {
                    let active_project = ui_state.active_project();
                    ui_state.record_project_domain_assistant_message_delta(
                        &active_project.id,
                        node_id,
                        &delta,
                    );
                }
            }
        }
        FrontendEvent::DomainNodeAssistantReasoningDelta { node_id, delta } => {
            if let Ok(node_id) = node_id.parse() {
                if let Some(project_id) = &project_id {
                    ui_state.record_project_domain_assistant_reasoning_delta(
                        project_id, node_id, &delta,
                    );
                } else {
                    let active_project = ui_state.active_project();
                    ui_state.record_project_domain_assistant_reasoning_delta(
                        &active_project.id,
                        node_id,
                        &delta,
                    );
                }
            }
        }
        FrontendEvent::DomainNodeAssistantResponseCompleted { node_id, message } => {
            if let Ok(node_id) = node_id.parse() {
                if let Some(message) = message {
                    if let Some(project_id) = &project_id {
                        ui_state
                            .record_project_domain_assistant_message(project_id, node_id, message);
                    } else {
                        let active_project = ui_state.active_project();
                        ui_state.record_project_domain_assistant_message(
                            &active_project.id,
                            node_id,
                            message,
                        );
                    }
                }
                if let Some(project_id) = &project_id {
                    ui_state.finish_project_domain_assistant_reasoning(project_id, node_id);
                } else {
                    let active_project = ui_state.active_project();
                    ui_state.finish_project_domain_assistant_reasoning(&active_project.id, node_id);
                }
            }
        }
        FrontendEvent::ToolCallStarted { name, arguments } => {
            if let Some(project_id) = &project_id {
                ui_state.record_project_tool_use(project_id, name, arguments);
            } else {
                ui_state.record_tool_use(name, arguments);
            }
        }
        FrontendEvent::HumanPrompt {
            question,
            choices,
            reply,
        } => {
            let prompt = Some(PendingPrompt {
                question,
                choices: (!choices.is_empty()).then_some(choices),
                reply,
            });
            if let Some(project_id) = &project_id {
                ui_state.set_project_pending_prompt(project_id, prompt);
            } else {
                ui_state.set_pending_prompt(prompt);
            }
        }
        FrontendEvent::RunSummary(summary) => {
            ui_state.set_run_summary(RunSummary::from(summary));
        }
        FrontendEvent::DomainNodePhaseChanged { node_id, phase } => {
            if let Ok(node_id) = node_id.parse() {
                if let Some(project_id) = &project_id {
                    ui_state.set_project_domain_node_phase(project_id, node_id, phase);
                } else {
                    let active_project = ui_state.active_project();
                    ui_state.set_project_domain_node_phase(&active_project.id, node_id, phase);
                }
            }
        }
        FrontendEvent::Quit => {}
        FrontendEvent::StepAttemptStarted { .. }
        | FrontendEvent::StepAttemptValidated { .. }
        | FrontendEvent::StepRepairStarted { .. }
        | FrontendEvent::StepRejected { .. }
        | FrontendEvent::ProjectScoped { .. }
        | FrontendEvent::DomainTreeUpdated
        | FrontendEvent::BackflowStarted { .. }
        | FrontendEvent::BackflowCascade { .. }
        | FrontendEvent::BackflowResolved { .. }
        | FrontendEvent::BackflowHalting { .. }
        | FrontendEvent::DeliveryGraphUpdated
        | FrontendEvent::DeliveryBatchStarted { .. }
        | FrontendEvent::DeliveryBatchCompleted { .. } => {}
    }
}

fn push_event(ui_state: &UiState, project_id: Option<&crate::project::ProjectId>, event: UiEvent) {
    if let Some(project_id) = project_id {
        ui_state.push_project_event(project_id, event);
    } else {
        ui_state.push_event(event);
    }
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
