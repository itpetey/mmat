use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use dioxus::prelude::*;
use pulldown_cmark::{Options, Parser};

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
    let mut show_reasoning = use_signal(|| false);
    let mut local_reasoning_overrides = use_signal(HashMap::<u64, bool>::new);

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
    let show_reasoning_value = *show_reasoning.read();

    rsx! {
        div { class: "mmat-root",
            div { class: "mmat-shell",
                div { class: "mmat-header",
                    pre { class: "mmat-logo", "aria-hidden": "true", "makemeathing" }
                    div { display: "flex", align_items: "center", gap: "0.5rem",
                        button {
                            class: if show_reasoning_value { "reasoning-toggle active" } else { "reasoning-toggle" },
                            r#type: "button",
                            onclick: move |_| {
                                show_reasoning.set(!show_reasoning_value);
                                local_reasoning_overrides.set(HashMap::new());
                            },
                            if show_reasoning_value { "thinking on" } else { "thinking off" }
                        }
                        ProjectSwitcher {
                            ui_state: props.ui_state.clone(),
                            projects: snapshot_value.projects.clone(),
                            active_project_id: snapshot_value.active_project.id.clone(),
                        }
                    }
                }
                div { class: "mmat-content",
                    div { class: "mmat-project-bar",
                        div { class: "active-project",
                            span { class: "project-name", "{snapshot_value.active_project.name}" }
                            span { class: "project-root", "{snapshot_value.active_project.root.display()}" }
                        }
                        RegisterProjectForm { ui_state: props.ui_state.clone() }
                    }
                    if snapshot_value.domain_tree_nodes.is_empty() {
                        // Single-column layout for projects without a domain tree.
                        div { class: "mmat-conversation",
                            if snapshot_value.conversation.is_empty() && matches!(snapshot_value.composer_mode, ComposerMode::InitialPrompt) {
                                div { class: "conversation-entry connecting", "Ready for a new run." }
                            }
                            for (index, entry) in snapshot_value.conversation.iter().enumerate() {
                                {render_conversation_entry(index, entry, show_reasoning_value, local_reasoning_overrides)}
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
                    } else {
                        // Multi-column layout for domain-mapped projects.
                        div { class: "mmat-multi-domain",
                            div { class: "mmat-domain-sidebar",
                                div { class: "domain-tree",
                                    h3 { "Domains" }
                                    for node in &snapshot_value.domain_tree_nodes {
                                        div { class: "domain-tree-node",
                                            "{node.name} ({node.status})"
                                        }
                                    }
                                }
                                if let Some(graph) = &snapshot_value.delivery_graph {
                                    div { class: "delivery-graph-mini",
                                        h3 { "Delivery" }
                                        for batch in &graph.batches {
                                            div { class: "delivery-batch",
                                                "Batch {batch.index}: {batch.nodes.len()} jobs"
                                            }
                                        }
                                    }
                                }
                            }
                            div { class: "mmat-domain-centre",
                                if snapshot_value.open_domain_tabs.is_empty() {
                                    div { class: "domain-tab-placeholder", "Select a domain to begin." }
                                } else {
                                    div { class: "domain-tab-bar",
                                        for tab_id in &snapshot_value.open_domain_tabs {
                                            div { class: "domain-tab",
                                                "{tab_id}"
                                            }
                                        }
                                    }
                                }
                                div { class: "domain-conversation domain-tab-content",
                                    if snapshot_value.conversation.is_empty() && matches!(snapshot_value.composer_mode, ComposerMode::InitialPrompt) {
                                        div { class: "conversation-entry connecting", "Ready for a new run." }
                                    }
                                    for (index, entry) in snapshot_value.conversation.iter().enumerate() {
                                        {render_conversation_entry(index, entry, show_reasoning_value, local_reasoning_overrides)}
                                    }
                                    if matches!(snapshot_value.composer_mode, ComposerMode::Working) {
                                        if let Some(summary) = &snapshot_value.run_summary {
                                            div { class: "conversation-entry status", "{format_run_summary(summary)}" }
                                        }
                                    }
                                }
                            }
                            div { class: "mmat-domain-detail",
                                if let Some(node_id) = snapshot_value.active_domain_node_id {
                                    div { class: "detail-panel",
                                        h3 { "Details" }
                                        p { "Node: {node_id}" }
                                    }
                                } else {
                                    div { class: "detail-panel-placeholder", "Select a node for details." }
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
}

#[allow(non_snake_case)]
fn Composer(props: ComposerProps) -> Element {
    let mut input = use_signal(String::new);
    let is_working = matches!(props.mode, ComposerMode::Working);

    let key_submit_state = props.ui_state.clone();
    let reset_input = input;
    use_effect(move || {
        if reset_input.read().is_empty() {
            spawn(async move {
                let _ = document::eval(
                    r#"
                    const textarea = document.querySelector('.composer-textarea');
                    if (textarea) {
                        textarea.style.height = 'auto';
                    }
                "#,
                )
                .await;
            });
        }
    });

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
                placeholder: "Cmd+Enter to submit...",
                rows: "2",
                autofocus: true,
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
        if !props.queue.is_empty() {
            div { class: "queue-panel",
                div { class: "queue-active",
                    span { class: "queue-title", "queue" }
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

fn render_conversation_entry(
    index: usize,
    entry: &ConversationEntry,
    show_reasoning: bool,
    mut local_overrides: Signal<HashMap<u64, bool>>,
) -> Element {
    match entry {
        ConversationEntry::UserMessage { text } => rsx! {
            div { key: "conv-{index}", class: "conversation-entry user",
                span { class: "entry-role", "user" }
                span { class: "entry-body", "{text}" }
            }
        },
        ConversationEntry::AssistantQuestion { question } => {
            let html = render_markdown(question);
            rsx! {
                div { key: "conv-{index}", class: "conversation-entry question",
                    span { class: "entry-role", "mmat" }
                    span { class: "entry-body markdown-body", dangerous_inner_html: "{html}" }
                }
            }
        }
        ConversationEntry::AssistantReasoning { text, complete } => {
            let entry_key = index as u64;
            let expanded = local_overrides
                .read()
                .get(&entry_key)
                .copied()
                .unwrap_or(show_reasoning);
            let role_label = if *complete { "trace" } else { "trace*" };
            rsx! {
                div {
                    key: "conv-{index}",
                    class: if *complete { "conversation-entry reasoning" } else { "conversation-entry reasoning pending" },
                    span {
                        class: "entry-role entry-role-toggle",
                        onclick: move |_| {
                            let mut overrides = local_overrides.read().clone();
                            let current = overrides.get(&entry_key).copied().unwrap_or(show_reasoning);
                            overrides.insert(entry_key, !current);
                            local_overrides.set(overrides);
                        },
                        "{role_label}"
                    }
                    if expanded {
                        span { class: "entry-body", "{text}" }
                    } else {
                        if *complete {
                            span { class: "entry-body", "..." }
                        } else {
                            span { class: "entry-body",
                                span { class: "reasoning-loading",
                                    span { class: "reasoning-loading-dot" }
                                    span { class: "reasoning-loading-dot" }
                                    span { class: "reasoning-loading-dot" }
                                }
                            }
                        }
                    }
                }
            }
        }
        ConversationEntry::AssistantMessage { text, complete: _ } => {
            let entry_key = index as u64;
            let expanded = local_overrides
                .read()
                .get(&entry_key)
                .copied()
                .unwrap_or(show_reasoning);
            let role_label = "thinking";
            rsx! {
                div { key: "conv-{index}", class: "conversation-entry assistant",
                    span {
                        class: "entry-role entry-role-toggle",
                        onclick: move |_| {
                            let mut overrides = local_overrides.read().clone();
                            let current = overrides.get(&entry_key).copied().unwrap_or(show_reasoning);
                            overrides.insert(entry_key, !current);
                            local_overrides.set(overrides);
                        },
                        "{role_label}"
                    }
                    if expanded {
                        span { class: "entry-body", "{text}" }
                    } else {
                        span { class: "entry-body", "..." }
                    }
                }
            }
        }
        ConversationEntry::ToolUse { name, arguments } => rsx! {
            div { key: "conv-{index}", class: "conversation-entry tool-use",
                span { class: "entry-role", "tool" }
                span { class: "entry-body",
                    span { class: "tool-call-name", "{name}" }
                    span { class: "tool-call-args", " {arguments}" }
                }
            }
        },
    }
}

fn render_markdown(text: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(text, options);
    let mut html_output = String::new();
    pulldown_cmark::html::push_html(&mut html_output, parser);
    html_output
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
