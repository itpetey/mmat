use tokio::sync::mpsc;

use crate::ws::event::FrontendEvent;
use crate::ws::ui_state::{PendingPrompt, UiEvent, UiState};

pub fn spawn_event_translator(
    mut event_rx: mpsc::UnboundedReceiver<FrontendEvent>,
    ui_state: std::sync::Arc<UiState>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                FrontendEvent::StepStarted {
                    task_name,
                    task_label,
                } => {
                    if task_name == "planning" {
                        ui_state.set_planning_started();
                    }
                    ui_state.push_event(UiEvent::StepStarted {
                        task_name,
                        task_label,
                    });
                }
                FrontendEvent::StepCompleted {
                    task_name,
                    task_label,
                    attempts,
                } => {
                    ui_state.push_event(UiEvent::StepCompleted {
                        task_name,
                        task_label,
                        attempts,
                    });
                }
                FrontendEvent::StepFailed {
                    task_name,
                    task_label,
                    stage,
                } => {
                    ui_state.push_event(UiEvent::StepFailed {
                        task_name,
                        task_label,
                        stage,
                    });
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
                    ui_state.push_event(UiEvent::Log {
                        level: level.to_string(),
                        target,
                        message,
                    });
                }
                FrontendEvent::HumanPrompt {
                    question,
                    choices,
                    reply,
                } => {
                    ui_state.set_pending_prompt(Some(PendingPrompt {
                        question,
                        choices: Some(choices),
                        reply,
                    }));
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
