use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use dioxus::prelude::*;
use pulldown_cmark::{Options, Parser};

use crate::liveview::state::{
    BackflowNotificationSnapshot, DomainNodeUiSnapshot, DomainUiStateSnapshot,
};
use crate::liveview::{
    BuildJobSnapshot, ComposerMode, ConversationEntry, PendingPromptSnapshot,
    ProjectWorkerSnapshot, RunSummary, UiEvent, UiEventEntry, UiState,
};
use crate::plan::domain_map::DomainNodeId;

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

#[derive(Props, Clone)]
struct MultiDomainShellProps {
    ui_state: Arc<UiState>,
    snapshot: crate::liveview::UiSnapshot,
    show_reasoning: bool,
    local_reasoning_overrides: Signal<HashMap<u64, bool>>,
}

#[derive(Props, Clone)]
struct DomainTreeSidebarProps {
    ui_state: Arc<UiState>,
    nodes: Vec<DomainNodeUiSnapshot>,
    active_node_id: Option<DomainNodeId>,
    delivery_graph: Option<crate::deliver::DeliveryGraph>,
}

#[derive(Props, Clone)]
struct DomainTreeNodeProps {
    ui_state: Arc<UiState>,
    node: DomainNodeUiSnapshot,
    is_active: bool,
    depth: usize,
}

#[derive(Props, Clone, PartialEq)]
struct DeliveryGraphMiniProps {
    graph: Option<crate::deliver::DeliveryGraph>,
}

#[derive(Props, Clone)]
struct TabBarProps {
    ui_state: Arc<UiState>,
    tabs: Vec<DomainNodeId>,
    active_tab: Option<DomainNodeId>,
    backflow_notifications: Vec<BackflowNotificationSnapshot>,
    domain_states: std::collections::BTreeMap<DomainNodeId, DomainUiStateSnapshot>,
}

#[derive(Props, Clone)]
struct TabProps {
    ui_state: Arc<UiState>,
    tab_id: DomainNodeId,
    is_active: bool,
    has_backflow: bool,
    label: String,
}

#[derive(Props, Clone, PartialEq)]
struct BackflowBannerProps {
    notifications: Vec<BackflowNotificationSnapshot>,
}

#[derive(Props, Clone, PartialEq)]
struct PipelinePhaseIndicatorProps {
    node_id: DomainNodeId,
    phase: Option<String>,
}

#[derive(Props, Clone)]
struct RightDetailPanelProps {
    ui_state: Arc<UiState>,
    node_id: Option<DomainNodeId>,
    nodes: Vec<DomainNodeUiSnapshot>,
    domain_states: std::collections::BTreeMap<DomainNodeId, DomainUiStateSnapshot>,
    delivery_graph: Option<crate::deliver::DeliveryGraph>,
    backflow_notifications: Vec<BackflowNotificationSnapshot>,
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

impl PartialEq for MultiDomainShellProps {
    fn eq(&self, other: &Self) -> bool {
        self.snapshot == other.snapshot
            && self.show_reasoning == other.show_reasoning
            && Arc::ptr_eq(&self.ui_state, &other.ui_state)
    }
}

impl PartialEq for DomainTreeSidebarProps {
    fn eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes
            && self.active_node_id == other.active_node_id
            && self.delivery_graph == other.delivery_graph
            && Arc::ptr_eq(&self.ui_state, &other.ui_state)
    }
}

impl PartialEq for DomainTreeNodeProps {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
            && self.is_active == other.is_active
            && self.depth == other.depth
            && Arc::ptr_eq(&self.ui_state, &other.ui_state)
    }
}

impl PartialEq for TabBarProps {
    fn eq(&self, other: &Self) -> bool {
        self.tabs == other.tabs
            && self.active_tab == other.active_tab
            && self.backflow_notifications == other.backflow_notifications
            && self.domain_states == other.domain_states
            && Arc::ptr_eq(&self.ui_state, &other.ui_state)
    }
}

impl PartialEq for TabProps {
    fn eq(&self, other: &Self) -> bool {
        self.tab_id == other.tab_id
            && self.is_active == other.is_active
            && self.has_backflow == other.has_backflow
            && self.label == other.label
            && Arc::ptr_eq(&self.ui_state, &other.ui_state)
    }
}

impl PartialEq for RightDetailPanelProps {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id
            && self.nodes == other.nodes
            && self.domain_states == other.domain_states
            && self.delivery_graph == other.delivery_graph
            && self.backflow_notifications == other.backflow_notifications
            && Arc::ptr_eq(&self.ui_state, &other.ui_state)
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
                        MultiDomainShell {
                            ui_state: props.ui_state.clone(),
                            snapshot: snapshot_value,
                            show_reasoning: show_reasoning_value,
                            local_reasoning_overrides,
                        }
                    }
                }
            }
        }
    }
}

#[allow(non_snake_case)]
fn BackflowBanner(props: BackflowBannerProps) -> Element {
    let notification =
        props
            .notifications
            .first()
            .cloned()
            .unwrap_or_else(|| BackflowNotificationSnapshot {
                node_id: DomainNodeId::new(),
                severity: "minor".to_string(),
                reason: "Unknown".to_string(),
                cascade_depth: 0,
            });
    let severity_class = notification.severity.to_ascii_lowercase();

    rsx! {
        div { class: "backflow-banner {severity_class}",
            span { class: "backflow-severity", "{notification.severity}" }
            span { class: "backflow-reason", "{notification.reason}" }
            if notification.cascade_depth > 0 {
                span { class: "backflow-cascade", "Cascade depth: {notification.cascade_depth}" }
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
fn DeliveryGraphMini(props: DeliveryGraphMiniProps) -> Element {
    rsx! {
        div { class: "delivery-graph-mini",
            h3 { "Delivery" }
            if let Some(graph) = props.graph {
                for batch in graph.batches.clone() {
                    div {
                        class: if graph.active_batch_index == Some(batch.index) {
                            "delivery-batch active"
                        } else {
                            "delivery-batch"
                        },
                        "B{batch.index}"
                        for _node in batch.nodes {
                            span { class: "delivery-batch-node" }
                        }
                    }
                }
            } else {
                div { class: "delivery-graph-placeholder", "No delivery jobs yet." }
            }
        }
    }
}

#[allow(non_snake_case)]
fn DomainTreeNode(props: DomainTreeNodeProps) -> Element {
    let class = if props.is_active {
        "domain-tree-node active"
    } else {
        "domain-tree-node"
    };
    let status_class = format!("status-badge {}", status_css_class(&props.node.status));
    let node_id = props.node.node_id;
    let state = props.ui_state.clone();
    let indent = format!("padding-left: {}rem;", 0.5 + props.depth as f32 * 0.75);

    rsx! {
        div {
            class: "{class}",
            style: "{indent}",
            onclick: move |_| {
                state.set_project_active_domain_node_id(
                    &state.active_project().id,
                    Some(node_id),
                );
                let mut tabs = state
                    .snapshot()
                    .open_domain_tabs;
                if !tabs.contains(&node_id) {
                    tabs.push(node_id);
                    state.set_project_open_domain_tabs(&state.active_project().id, tabs);
                }
            },
            span { class: "{status_class}" }
            span { "{props.node.name}" }
        }
    }
}

#[allow(non_snake_case)]
fn DomainTreeSidebar(props: DomainTreeSidebarProps) -> Element {
    rsx! {
        div { class: "mmat-domain-sidebar",
            div { class: "domain-tree",
                h3 { "Domains" }
                if props.nodes.is_empty() {
                    div { class: "domain-tree-placeholder", "No domains discovered yet." }
                } else {
                    for node in props.nodes.clone() {
                        DomainTreeNode {
                            ui_state: props.ui_state.clone(),
                            node: node.clone(),
                            is_active: props.active_node_id == Some(node.node_id),
                            depth: node.depth,
                        }
                    }
                }
            }
            DeliveryGraphMini { graph: props.delivery_graph }
        }
    }
}

#[allow(non_snake_case)]
fn MultiDomainShell(props: MultiDomainShellProps) -> Element {
    let snapshot = props.snapshot;
    let active_node_id = snapshot.active_domain_node_id;
    let backflow_for_active: Vec<_> = snapshot
        .backflow_notifications
        .iter()
        .filter(|n| active_node_id.is_some_and(|id| n.node_id == id))
        .cloned()
        .collect();

    rsx! {
        div { class: "mmat-multi-domain",
            DomainTreeSidebar {
                ui_state: props.ui_state.clone(),
                nodes: snapshot.domain_tree_nodes.clone(),
                active_node_id,
                delivery_graph: snapshot.delivery_graph.clone(),
            }
            div { class: "mmat-domain-centre",
                TabBar {
                    ui_state: props.ui_state.clone(),
                    tabs: snapshot.open_domain_tabs.clone(),
                    active_tab: active_node_id,
                    backflow_notifications: snapshot.backflow_notifications.clone(),
                    domain_states: snapshot.domain_states.clone(),
                }
                if let Some(node_id) = active_node_id {
                    if !backflow_for_active.is_empty() {
                        BackflowBanner { notifications: backflow_for_active.clone() }
                    }
                    PipelinePhaseIndicator {
                        node_id,
                        phase: snapshot.domain_states.get(&node_id).and_then(|s| s.phase.clone()),
                    }
                }
                div { class: "domain-conversation domain-tab-content",
                    if snapshot.conversation.is_empty() && matches!(snapshot.composer_mode, ComposerMode::InitialPrompt) {
                        div { class: "conversation-entry connecting", "Ready for a new run." }
                    }
                    for (index, entry) in snapshot.conversation.iter().enumerate() {
                        {render_conversation_entry(index, entry, props.show_reasoning, props.local_reasoning_overrides)}
                    }
                    if matches!(snapshot.composer_mode, ComposerMode::Working) {
                        if let Some(summary) = &snapshot.run_summary {
                            div { class: "conversation-entry status", "{format_run_summary(summary)}" }
                        }
                    }
                }
            }
            RightDetailPanel {
                ui_state: props.ui_state.clone(),
                node_id: active_node_id,
                nodes: snapshot.domain_tree_nodes.clone(),
                domain_states: snapshot.domain_states.clone(),
                delivery_graph: snapshot.delivery_graph.clone(),
                backflow_notifications: snapshot.backflow_notifications.clone(),
            }
        }
        div { class: "mmat-composer",
            Composer {
                ui_state: props.ui_state.clone(),
                mode: snapshot.composer_mode.clone(),
                pending_prompt: snapshot.pending_prompt.clone(),
            }
            RawLogsDisclosure { history: snapshot.history.clone() }
        }
    }
}

#[allow(non_snake_case)]
fn PipelinePhaseIndicator(props: PipelinePhaseIndicatorProps) -> Element {
    let phases = [
        "Discovery",
        "Knowledge",
        "Solutions",
        "Architect",
        "Delivery",
    ];
    let current = props.phase.as_deref().unwrap_or("Discovery");
    let current_index = phases.iter().position(|p| *p == current).unwrap_or(0);

    rsx! {
        div { class: "pipeline-phase-indicator",
            for (index, phase) in phases.iter().enumerate() {
                if index > 0 {
                    span { class: "pipeline-separator", "→" }
                }
                span {
                    class: if index < current_index {
                        "pipeline-phase completed"
                    } else if index == current_index {
                        "pipeline-phase current"
                    } else {
                        "pipeline-phase pending"
                    },
                    "{phase}"
                }
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

#[allow(non_snake_case)]
fn RightDetailPanel(props: RightDetailPanelProps) -> Element {
    let mut collapsed = use_signal(|| false);
    let is_collapsed = *collapsed.read();
    let node_id = props.node_id;
    let backflow_count = node_id.map_or(0, |id| {
        props
            .backflow_notifications
            .iter()
            .filter(|n| n.node_id == id)
            .count()
    });

    if is_collapsed {
        return rsx! {
            div { class: "mmat-domain-detail collapsed",
                button {
                    class: "detail-panel-toggle",
                    r#type: "button",
                    onclick: move |_| collapsed.set(false),
                    "◀"
                }
            }
        };
    }

    rsx! {
        div { class: "mmat-domain-detail",
            button {
                class: "detail-panel-toggle",
                r#type: "button",
                onclick: move |_| collapsed.set(true),
                "▶"
            }
            if let Some(node_id) = node_id {
                if let Some(node) = props.nodes.iter().find(|n| n.node_id == node_id) {
                    div { class: "detail-panel",
                        h3 { "Node" }
                        div { class: "detail-row",
                            span { class: "detail-label", "Name" }
                            span { class: "detail-value", "{node.name}" }
                        }
                        div { class: "detail-row",
                            span { class: "detail-label", "Status" }
                            span { class: "detail-value", "{node.status}" }
                        }
                        div { class: "detail-row",
                            span { class: "detail-label", "Phase" }
                            span { class: "detail-value", "{node.phase}" }
                        }
                        div { class: "detail-row",
                            span { class: "detail-label", "Depth" }
                            span { class: "detail-value", "{node.depth}" }
                        }
                        if let Some(state) = props.domain_states.get(&node_id) {
                            div { class: "detail-row",
                                span { class: "detail-label", "Messages" }
                                span { class: "detail-value", "{state.conversation.len()}" }
                            }
                        }
                        if backflow_count > 0 {
                            div { class: "detail-row",
                                span { class: "detail-label", "Backflows" }
                                span { class: "detail-value", "{backflow_count}" }
                            }
                        }
                    }
                } else {
                    div { class: "detail-panel-placeholder", "Node not found." }
                }
            } else {
                div { class: "detail-panel-placeholder", "Select a node for details." }
            }
        }
    }
}

#[allow(non_snake_case)]
fn Tab(props: TabProps) -> Element {
    let mut class = if props.is_active {
        "domain-tab active".to_string()
    } else {
        "domain-tab".to_string()
    };
    if props.has_backflow {
        class.push_str(" backflow");
    }

    let tab_id = props.tab_id;
    let state_for_click = props.ui_state.clone();
    let state_for_close = props.ui_state.clone();
    let is_active = props.is_active;

    rsx! {
        div {
            class: "{class}",
            onclick: move |_| {
                let project_id = state_for_click.active_project().id;
                state_for_click.set_project_active_domain_node_id(&project_id, Some(tab_id));
            },
            "{props.label}"
            button {
                class: "domain-tab-close",
                r#type: "button",
                onclick: move |event| {
                    event.stop_propagation();
                    let project_id = state_for_close.active_project().id;
                    let mut tabs = state_for_close.snapshot().open_domain_tabs;
                    tabs.retain(|id| *id != tab_id);
                    state_for_close.set_project_open_domain_tabs(&project_id, tabs.clone());
                    if is_active && !tabs.is_empty() {
                        state_for_close.set_project_active_domain_node_id(
                            &project_id,
                            Some(tabs[tabs.len() - 1]),
                        );
                    } else if is_active {
                        state_for_close.set_project_active_domain_node_id(&project_id, None);
                    }
                },
                "x"
            }
        }
    }
}

#[allow(non_snake_case)]
fn TabBar(props: TabBarProps) -> Element {
    let backflow_node_ids: std::collections::HashSet<_> = props
        .backflow_notifications
        .iter()
        .map(|n| n.node_id)
        .collect();

    rsx! {
        if props.tabs.is_empty() {
            div { class: "domain-tab-placeholder", "Select a domain to begin." }
        } else {
            div { class: "domain-tab-bar",
                for tab_id in props.tabs.clone() {
                    Tab {
                        ui_state: props.ui_state.clone(),
                        tab_id,
                        is_active: props.active_tab == Some(tab_id),
                        has_backflow: backflow_node_ids.contains(&tab_id),
                        label: props
                            .domain_states
                            .get(&tab_id)
                            .map(|s| s.node_id.to_string())
                            .unwrap_or_else(|| tab_id.to_string()),
                    }
                }
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

fn status_css_class(status: &str) -> &'static str {
    match status {
        "discovering" => "discovering",
        "ready" => "ready",
        "knowledge_materialised" => "knowledge-materialised",
        "solutions_collected" => "solutions-collected",
        "solution_chosen" => "solution-chosen",
        "architect_complete" => "architect-complete",
        "delivering" => "delivering",
        "complete" => "complete",
        "replanning" => "replanning",
        _ => "pending",
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
