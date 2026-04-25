use std::{collections::VecDeque, sync::Arc};

use dioxus::prelude::*;

use crate::liveview::{
    ComposerMode, ConversationEntry, PendingPromptSnapshot, RunSummary, UiEvent, UiEventEntry,
    UiState,
};

#[derive(Props, Clone)]
pub(super) struct RootAppProps {
    pub ui_state: Arc<UiState>,
}

impl PartialEq for RootAppProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ui_state, &other.ui_state)
    }
}

#[allow(non_snake_case)]
pub(super) fn RootApp(props: RootAppProps) -> Element {
    let mut snapshot = use_signal(|| props.ui_state.snapshot());
    let state_for_future = props.ui_state.clone();

    use_future(move || {
        let state = state_for_future.clone();
        async move {
            let mut version_rx = state.subscribe();
            while version_rx.changed().await.is_ok() {
                snapshot.set(state.snapshot());
            }
        }
    });

    let snapshot_value = snapshot.read().clone();
    let header_badge = header_badge_text(
        &snapshot_value.composer_mode,
        snapshot_value.run_summary.as_ref(),
    );

    rsx! {
        div { class: "mmat-root",
            div { class: "mmat-shell",
                div { class: "mmat-header",
                    pre { class: "mmat-logo", "aria-hidden": "true", "|\\/| |\\/|  /\\  T\n|  | |  | /--\\ |" }
                    div { class: "header-badge", "{header_badge}" }
                }
                div { class: "mmat-content",
                    div { class: "mmat-conversation",
                        if snapshot_value.conversation.is_empty() && matches!(snapshot_value.composer_mode, ComposerMode::InitialPrompt) {
                            div { class: "conversation-entry connecting", "Ready for a new run." }
                        }
                        for (index, entry) in snapshot_value.conversation.iter().enumerate() {
                            {render_conversation_entry(index, entry)}
                        }
                        if matches!(snapshot_value.composer_mode, ComposerMode::Working) {
                            if let Some(summary) = &snapshot_value.run_summary {
                                div { class: "conversation-entry status", "{format_run_summary(summary)}" }
                            }
                        }
                    }
                    div { class: "mmat-composer",
                        Composer {
                            ui_state: props.ui_state.clone(),
                            mode: snapshot_value.composer_mode.clone(),
                            pending_prompt: snapshot_value.pending_prompt.clone(),
                        }
                        RawLogsDisclosure { history: snapshot_value.history.clone() }
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone)]
struct ComposerProps {
    ui_state: Arc<UiState>,
    mode: ComposerMode,
    pending_prompt: Option<PendingPromptSnapshot>,
}

impl PartialEq for ComposerProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ui_state, &other.ui_state)
            && self.mode == other.mode
            && self.pending_prompt == other.pending_prompt
    }
}

#[allow(non_snake_case)]
fn Composer(props: ComposerProps) -> Element {
    let mut input = use_signal(String::new);
    let is_working = matches!(props.mode, ComposerMode::Working);
    let button_label = match props.mode {
        ComposerMode::InitialPrompt => "Start",
        ComposerMode::Reply => "Reply",
        ComposerMode::Working => "Working...",
    };
    let placeholder = match props.mode {
        ComposerMode::InitialPrompt => "Describe what you want to build...",
        ComposerMode::Reply => "Type your reply...",
        ComposerMode::Working => "Working... You can draft the next message here.",
    };

    let submit_state = props.ui_state.clone();
    let choices = props
        .pending_prompt
        .as_ref()
        .and_then(|prompt| prompt.choices.clone())
        .unwrap_or_default();

    rsx! {
        if !choices.is_empty() {
            div { class: "composer-choices",
                for choice in choices {
                    {
                        let choice_for_click = choice.clone();
                        let choice_state = props.ui_state.clone();
                        rsx! {
                            button {
                                class: "composer-choice-btn",
                                r#type: "button",
                                disabled: is_working,
                                onclick: move |_| {
                                    choice_state.send_pending_prompt(choice_for_click.clone());
                                },
                                "{choice}"
                            }
                        }
                    }
                }
            }
        }
        div { class: "composer-row",
            textarea {
                class: "composer-textarea",
                value: "{input}",
                disabled: is_working,
                placeholder: "{placeholder}",
                rows: "2",
                oninput: move |event| input.set(event.value()),
            }
            button {
                class: "composer-btn",
                r#type: "button",
                disabled: is_working,
                onclick: move |_| {
                    let submitted = input.read().trim().to_string();
                    if submitted.is_empty() {
                        return;
                    }

                    if submit_state.send_initial_input(submitted.clone())
                        || submit_state.send_pending_prompt(submitted)
                    {
                        input.set(String::new());
                    }
                },
                "{button_label}"
            }
        }
        div { class: "composer-hint", "Cmd+Enter to submit" }
    }
}

#[derive(Props, Clone, PartialEq)]
struct RawLogsDisclosureProps {
    history: VecDeque<UiEventEntry>,
}

#[allow(non_snake_case)]
fn RawLogsDisclosure(props: RawLogsDisclosureProps) -> Element {
    if props.history.is_empty() {
        return rsx! {};
    }

    rsx! {
        details {
            summary { class: "raw-logs-toggle", "Raw logs" }
            div { class: "raw-logs-container",
                for entry in props.history.iter() {
                    div {
                        key: "{entry.id}",
                        class: "raw-log-entry {log_level_class(&entry.event)}",
                        "{format_event(&entry.event)}"
                    }
                }
            }
        }
    }
}

fn render_conversation_entry(index: usize, entry: &ConversationEntry) -> Element {
    match entry {
        ConversationEntry::UserMessage { text } => rsx! {
            div { key: "conv-{index}", class: "conversation-entry user", "{text}" }
        },
        ConversationEntry::AssistantQuestion { question } => rsx! {
            div { key: "conv-{index}", class: "conversation-entry question", "{question}" }
        },
        ConversationEntry::AssistantReasoning { text, complete } => rsx! {
            div {
                key: "conv-{index}",
                class: if *complete { "conversation-entry reasoning" } else { "conversation-entry reasoning pending" },
                div { class: "reasoning-label", if *complete { "Reasoning" } else { "Reasoning..." } }
                "{text}"
            }
        },
        ConversationEntry::AssistantMessage { text, .. } => rsx! {
            div { key: "conv-{index}", class: "conversation-entry assistant", "{text}" }
        },
    }
}

fn header_badge_text(mode: &ComposerMode, summary: Option<&RunSummary>) -> String {
    if let Some(summary) = summary {
        return match summary.status.as_str() {
            "running" => format!("Running: {}", summary.current_stage.replace('_', " ")),
            "awaiting_clarification" => "Awaiting clarification".to_string(),
            "awaiting_approval" => "Awaiting proposal approval".to_string(),
            "awaiting_contract_approval" => "Awaiting contract approval".to_string(),
            "revising" => format!("Revising: {}", summary.current_stage.replace('_', " ")),
            _ => summary.status.replace('_', " "),
        };
    }

    match mode {
        ComposerMode::InitialPrompt => "Ready for a new run".to_string(),
        ComposerMode::Reply => "Awaiting your reply".to_string(),
        ComposerMode::Working => "Working".to_string(),
    }
}

fn format_run_summary(summary: &RunSummary) -> String {
    let stage = summary.current_stage.replace('_', " ");
    match summary.status.as_str() {
        "awaiting_clarification" => format!("Waiting for clarification during {stage}."),
        "awaiting_approval" => "Waiting for proposal approval.".to_string(),
        "awaiting_contract_approval" => "Waiting for contract approval.".to_string(),
        "revising" => format!("Revising after feedback in {stage}."),
        "running" => format!("Working on {stage}."),
        other => format!("{other} ({stage})"),
    }
}

fn log_level_class(event: &UiEvent) -> &'static str {
    match event {
        UiEvent::Log { level, .. } => match level.to_lowercase().as_str() {
            "warn" | "warning" => "warn",
            "error" => "error",
            _ => "info",
        },
        UiEvent::StepStarted { .. }
        | UiEvent::StepCompleted { .. }
        | UiEvent::StepFailed { .. }
        | UiEvent::ComponentStarted { .. }
        | UiEvent::ComponentCompleted { .. }
        | UiEvent::ComponentFailed { .. } => "status",
    }
}

fn format_event(event: &UiEvent) -> String {
    match event {
        UiEvent::Log { level, message } => format!("[{level}] {message}"),
        UiEvent::StepStarted { task_label } => format!("> {task_label}"),
        UiEvent::StepCompleted {
            task_label,
            attempts,
        } => format!("ok {task_label} ({attempts} attempts)"),
        UiEvent::StepFailed { task_label, stage } => format!("x {task_label} ({stage})"),
        UiEvent::ComponentStarted { component, name } => {
            format!("[{component}] started: {name}")
        }
        UiEvent::ComponentCompleted { component, name } => {
            format!("[{component}] completed: {name}")
        }
        UiEvent::ComponentFailed { component, name } => {
            format!("[{component}] failed: {name}")
        }
    }
}
