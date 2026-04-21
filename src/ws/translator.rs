use tokio::sync::mpsc;

use serde_json::Value;

use crate::ws::event::FrontendEvent;
use crate::ws::ui_state::{PendingPrompt, UiEvent, UiState};

pub fn spawn_event_translator(
    mut event_rx: mpsc::UnboundedReceiver<FrontendEvent>,
    ui_state: std::sync::Arc<UiState>,
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
                FrontendEvent::Log { level, message, .. } => {
                    if let Some(content) = extract_assistant_content_from_log(&message) {
                        ui_state.record_assistant_message(content);
                    }
                    ui_state.push_event(UiEvent::Log {
                        level: level.to_string(),
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

fn extract_assistant_content_from_log(message: &str) -> Option<String> {
    let payload = message.strip_prefix("Generated prediction:")?.trim();
    let value: Value = serde_json::from_str(payload).ok()?;
    let content = value
        .get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?
        .as_str()?
        .trim();

    if content.is_empty() || looks_like_structured_payload(content) {
        return None;
    }

    Some(content.to_string())
}

fn looks_like_structured_payload(content: &str) -> bool {
    let trimmed = content.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

#[cfg(test)]
mod tests {
    use super::extract_assistant_content_from_log;

    #[test]
    fn extracts_human_visible_content_from_generated_prediction_log() {
        let message = r#"Generated prediction: {
  "choices": [
    {
      "message": {
        "content": "\n\nHere is the explanation.",
        "tool_calls": [
          { "function": { "name": "ask_user", "arguments": "{}" } }
        ]
      }
    }
  ]
}"#;

        assert_eq!(
            extract_assistant_content_from_log(message).as_deref(),
            Some("Here is the explanation.")
        );
    }

    #[test]
    fn ignores_blank_generated_prediction_content() {
        let message = r#"Generated prediction: {
  "choices": [
    {
      "message": {
        "content": "\n\n"
      }
    }
  ]
}"#;

        assert!(extract_assistant_content_from_log(message).is_none());
    }

    #[test]
    fn ignores_structured_json_generated_prediction_content() {
        let message = r#"Generated prediction: {
  "choices": [
    {
      "message": {
        "content": "{\"ready_for_solution\":false}"
      }
    }
  ]
}"#;

        assert!(extract_assistant_content_from_log(message).is_none());
    }
}
