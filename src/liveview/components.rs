use std::{collections::VecDeque, sync::Arc};

use dioxus::prelude::*;

use crate::liveview::{
    BuildJobSnapshot, ComposerMode, ConversationEntry, PendingPromptSnapshot,
    ProjectWorkerSnapshot, RunSummary, UiEvent, UiEventEntry, UiState,
};

#[derive(Props, Clone)]
pub(super) struct RootAppProps {
    pub ui_state: Arc<UiState>,
}

#[derive(Props, Clone)]
struct ProjectSwitcherProps {
    ui_state: Arc<UiState>,
    projects: Vec<crate::project::ProjectConfig>,
    active_project_id: crate::project::ProjectId,
}

#[derive(Props, Clone)]
struct RegisterProjectFormProps {
    ui_state: Arc<UiState>,
}

#[derive(Props, Clone, PartialEq)]
struct QueuePanelProps {
    queue: Vec<BuildJobSnapshot>,
    worker_summary: Vec<ProjectWorkerSnapshot>,
}

#[derive(Props, Clone)]
struct ComposerProps {
    ui_state: Arc<UiState>,
    mode: ComposerMode,
    pending_prompt: Option<PendingPromptSnapshot>,
}

#[derive(Props, Clone, PartialEq)]
struct RawLogsDisclosureProps {
    history: VecDeque<UiEventEntry>,
}

impl PartialEq for RootAppProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ui_state, &other.ui_state)
    }
}

impl PartialEq for ProjectSwitcherProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ui_state, &other.ui_state)
            && self.projects == other.projects
            && self.active_project_id == other.active_project_id
    }
}

impl PartialEq for RegisterProjectFormProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ui_state, &other.ui_state)
    }
}

impl PartialEq for ComposerProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ui_state, &other.ui_state)
            && self.mode == other.mode
            && self.pending_prompt == other.pending_prompt
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

    rsx! {
        div { class: "mmat-root",
            div { class: "mmat-shell",
                div { class: "mmat-header",
                    pre { class: "mmat-logo", "aria-hidden": "true", "|\\/| |\\/|  /\\  T\n|  | |  | /--\\ |" }
                    ProjectSwitcher {
                        ui_state: props.ui_state.clone(),
                        projects: snapshot_value.projects.clone(),
                        active_project_id: snapshot_value.active_project.id.clone(),
                    }
                }
                div { class: "mmat-content",
                    div { class: "mmat-project-bar",
                        div { class: "active-project",
                            span { class: "project-label", "project" }
                            span { class: "project-name", "{snapshot_value.active_project.name}" }
                            span { class: "project-root", "{snapshot_value.active_project.root.display()}" }
                        }
                        RegisterProjectForm { ui_state: props.ui_state.clone() }
                    }
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
                    QueuePanel {
                        queue: snapshot_value.queue.clone(),
                        worker_summary: snapshot_value.worker_summary.clone(),
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

#[allow(non_snake_case)]
fn Composer(props: ComposerProps) -> Element {
    let mut input = use_signal(String::new);
    let is_working = matches!(props.mode, ComposerMode::Working);
    let placeholder = match props.mode {
        ComposerMode::InitialPrompt => "Describe what you want to build...",
        ComposerMode::Reply => "Type your reply...",
        ComposerMode::Working => "Working... You can draft the next message here.",
    };

    let key_submit_state = props.ui_state.clone();
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
                onkeydown: move |event| {
                    let modifiers = event.modifiers();
                    let should_submit = event.key() == Key::Enter && (modifiers.meta() || modifiers.ctrl());

                    if !should_submit || is_working {
                        return;
                    }

                    event.prevent_default();
                    submit_composer_input(&mut input, &key_submit_state);
                },
            }
        }
        div { class: "composer-hint", "Cmd+Enter to submit" }
    }
}

#[allow(non_snake_case)]
fn ProjectSwitcher(props: ProjectSwitcherProps) -> Element {
    rsx! {
        nav { class: "project-switcher",
            for project in props.projects {
                {
                    let project_id = project.id.clone();
                    let state = props.ui_state.clone();
                    let class_name = if project.id == props.active_project_id {
                        "project-switch active"
                    } else {
                        "project-switch"
                    };
                    rsx! {
                        button {
                            class: "{class_name}",
                            r#type: "button",
                            onclick: move |_| {
                                state.switch_project(project_id.clone());
                            },
                            "{project.name}"
                        }
                    }
                }
            }
        }
    }
}

#[allow(non_snake_case)]
fn QueuePanel(props: QueuePanelProps) -> Element {
    rsx! {
        div { class: "queue-panel",
            div { class: "queue-active",
                span { class: "queue-title", "queue" }
                if props.queue.is_empty() {
                    span { class: "queue-empty", "empty" }
                }
                for job in props.queue {
                    div { class: "queue-job",
                        span { class: "queue-status {job.status}", "{job.status}" }
                        span { class: "queue-prompt", "{job.prompt}" }
                        if let Some(error) = &job.error {
                            span { class: "queue-error", "{error}" }
                        }
                    }
                }
            }
            if !props.worker_summary.is_empty() {
                div { class: "queue-global",
                    for worker in props.worker_summary {
                        div { class: "worker-summary",
                            span { class: "worker-name", "{worker.project_name}" }
                            span { class: "worker-count", "p {worker.pending}" }
                            span { class: "worker-count", "r {worker.running}" }
                            span { class: "worker-count failed", "f {worker.failed}" }
                        }
                    }
                }
            }
        }
    }
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

#[allow(non_snake_case)]
fn RegisterProjectForm(props: RegisterProjectFormProps) -> Element {
    let mut name = use_signal(String::new);
    let mut root = use_signal(String::new);
    let form_state = props.ui_state.clone();

    rsx! {
        div { class: "project-register",
            input {
                class: "project-input",
                value: "{name}",
                placeholder: "Name",
                oninput: move |event| name.set(event.value()),
            }
            input {
                class: "project-input root",
                value: "{root}",
                placeholder: "Repository root",
                oninput: move |event| root.set(event.value()),
            }
            button {
                class: "project-add",
                r#type: "button",
                onclick: move |_| {
                    let submitted_name = name.read().trim().to_string();
                    let submitted_root = root.read().trim().to_string();
                    if submitted_name.is_empty() || submitted_root.is_empty() {
                        return;
                    }
                    if form_state.register_project(submitted_name, submitted_root).is_ok() {
                        name.set(String::new());
                        root.set(String::new());
                    }
                },
                "+"
            }
        }
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

fn render_conversation_entry(index: usize, entry: &ConversationEntry) -> Element {
    match entry {
        ConversationEntry::UserMessage { text } => rsx! {
            div { key: "conv-{index}", class: "conversation-entry user",
                span { class: "entry-role", "user" }
                span { class: "entry-body", "{text}" }
            }
        },
        ConversationEntry::AssistantQuestion { question } => rsx! {
            div { key: "conv-{index}", class: "conversation-entry question",
                span { class: "entry-role", "agent" }
                span { class: "entry-body", "{question}" }
            }
        },
        ConversationEntry::AssistantReasoning { text, complete } => rsx! {
            div {
                key: "conv-{index}",
                class: if *complete { "conversation-entry reasoning" } else { "conversation-entry reasoning pending" },
                span { class: "entry-role", if *complete { "trace" } else { "trace*" } }
                span { class: "entry-body", "{text}" }
            }
        },
        ConversationEntry::AssistantMessage { text, .. } => rsx! {
            div { key: "conv-{index}", class: "conversation-entry assistant",
                span { class: "entry-role", "thinking" }
                span { class: "entry-body", "{text}" }
            }
        },
    }
}

fn submit_composer_input(input: &mut Signal<String>, ui_state: &Arc<UiState>) {
    let submitted = input.read().trim().to_string();
    if submitted.is_empty() {
        return;
    }

    if ui_state.send_initial_input(submitted.clone()) || ui_state.send_pending_prompt(submitted) {
        input.set(String::new());
    }
}
