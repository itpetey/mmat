use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::http::StatusCode;
use dioxus::prelude::*;
use dioxus_core::VirtualDom;
use dioxus_liveview::LiveviewRouter;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::info;

use crate::ws::event::FrontendEvent;
use crate::ws::ui_state::{ComposerMode, ConversationEntry, UiState};

pub type EventSender = mpsc::UnboundedSender<FrontendEvent>;
pub type InstructionReceiver = oneshot::Receiver<String>;

const DEFAULT_ADDR: &str = "127.0.0.1:8080";
const APP_STYLES: &str = r#"
:root {
    color-scheme: dark;
    font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
}

* { margin: 0; padding: 0; box-sizing: border-box; }

html, body {
    min-height: 100%;
    background: #0b0f13;
    color: #e4e9f0;
}

#main {
    min-height: 100vh;
}

.mmat-root {
    min-height: 100vh;
    padding: 1rem;
}

.mmat-shell {
    max-width: 980px;
    min-height: calc(100vh - 2rem);
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid #232b35;
    border-radius: 18px;
    background: #11161c;
}

.mmat-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
    padding: 1rem 1.25rem;
    border-bottom: 1px solid #232b35;
    background: #11161c;
}

.mmat-brand {
    display: flex;
    align-items: flex-start;
    gap: 0.9rem;
}

.mmat-logo {
    margin: 0;
    padding-top: 0.05rem;
    color: #b8c2cf;
    font-family: 'SF Mono', 'Fira Code', 'Roboto Mono', monospace;
    font-size: 0.68rem;
    line-height: 1.15;
    letter-spacing: 0.02em;
    white-space: pre;
}

.mmat-brand-copy {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
}

.mmat-title {
    font-size: 0.98rem;
    font-weight: 650;
    letter-spacing: 0.02em;
    color: #f2f5f8;
}

.mmat-subtitle {
    font-size: 0.82rem;
    color: #94a0af;
}

.mmat-header-meta {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    flex-wrap: wrap;
}

.header-badge {
    color: #c7d0dc;
    font-size: 0.76rem;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    white-space: nowrap;
}

.header-badge.subtle {
    color: #748091;
}

.mmat-content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
}

.mmat-conversation {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 1.25rem 1.25rem 0;
    display: flex;
    flex-direction: column;
    gap: 0.8rem;
    overflow-anchor: auto;
}

.conversation-entry {
    max-width: min(84%, 720px);
    padding: 0.85rem 0.95rem;
    border: 1px solid #25303b;
    border-radius: 12px;
    background: #151b22;
    color: #d6dde7;
    font-size: 0.94rem;
    line-height: 1.55;
    white-space: pre-wrap;
    word-break: break-word;
}

.conversation-entry.user {
    align-self: flex-end;
    border-bottom-right-radius: 4px;
    background: #1b2430;
    color: #eef3f9;
}

.conversation-entry.assistant {
    align-self: flex-start;
    border-bottom-left-radius: 4px;
}

.conversation-entry.reasoning {
    align-self: flex-start;
    border-bottom-left-radius: 4px;
    border-left: 2px solid #6f8197;
    background: #12181f;
    color: #bcc7d5;
}

.conversation-entry.reasoning.pending {
    border-style: dashed;
}

.conversation-entry.question {
    align-self: flex-start;
    border-bottom-left-radius: 4px;
    background: #131c25;
    color: #d9e2ed;
}

.conversation-entry.status,
.conversation-entry.connecting {
    align-self: center;
    max-width: 100%;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: #7f8b9b;
    font-size: 0.79rem;
}

.reasoning-label {
    margin-bottom: 0.35rem;
    color: #8d9aab;
    font-size: 0.72rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
}

.mmat-composer {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 1rem 1.25rem 1.25rem;
    border-top: 1px solid #232b35;
    background: #11161c;
}

.composer-choices {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
}

.composer-choice-btn {
    padding: 0.5rem 0.75rem;
    border: 1px solid #2a3440;
    border-radius: 999px;
    background: #141a21;
    color: #cbd4df;
    font-size: 0.85rem;
    cursor: pointer;
}

.composer-choice-btn:hover {
    border-color: #3a4654;
    background: #171f28;
}

.composer-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.75rem;
    align-items: start;
}

.composer-textarea {
    width: 100%;
    min-height: 4.5rem;
    padding: 0.9rem 1rem;
    border: 1px solid #2a3440;
    border-radius: 12px;
    background: #0e1318;
    color: #edf2f7;
    font: inherit;
    line-height: 1.5;
    outline: none;
    resize: none;
    transition: border-color 0.14s ease, background 0.14s ease;
}

.composer-textarea::placeholder {
    color: #778395;
}

.composer-textarea:focus {
    border-color: #465364;
    background: #10161c;
}

.composer-textarea:disabled {
    opacity: 0.65;
    cursor: not-allowed;
}

.composer-btn {
    min-width: 112px;
    padding: 0.9rem 1.1rem;
    border: 1px solid #2a3440;
    border-radius: 12px;
    font-size: 0.94rem;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
}

.composer-btn.primary {
    background: #e6ebf1;
    border-color: #e6ebf1;
    color: #0b1015;
}

.composer-btn.primary:hover:not(:disabled) {
    background: #f1f5f9;
    border-color: #f1f5f9;
}

.composer-btn:disabled {
    background: #171d24;
    border-color: #2a3440;
    color: #6f7b8c;
    cursor: not-allowed;
}

.composer-hint {
    font-size: 0.76rem;
    color: #748091;
}

.raw-logs-toggle {
    width: fit-content;
    padding: 0.45rem 0.75rem;
    border: 1px solid #2a3440;
    border-radius: 999px;
    background: transparent;
    color: #9eabbc;
    font-size: 0.79rem;
    cursor: pointer;
    text-align: left;
}

.raw-logs-toggle {
    list-style: none;
}

.raw-logs-toggle::-webkit-details-marker {
    display: none;
}

.raw-logs-label-open {
    display: none;
}

details[open] .raw-logs-label-open {
    display: inline;
}

details[open] .raw-logs-label-closed {
    display: none;
}

.raw-logs-toggle:hover {
    border-color: #3a4654;
    color: #dbe3ec;
}

.raw-logs-container {
    margin-top: 0.25rem;
    max-height: 300px;
    overflow-y: auto;
    overflow-anchor: auto;
    padding: 0.9rem 1rem;
    border: 1px solid #232b35;
    border-radius: 12px;
    background: #0d1217;
    font-family: 'SF Mono', 'Fira Code', 'Roboto Mono', monospace;
    font-size: 0.8rem;
    line-height: 1.45;
}

.raw-log-entry {
    padding: 0.15rem 0;
    white-space: pre-wrap;
    word-break: break-word;
}

.raw-log-entry.info { color: #c9d1d9; }
.raw-log-entry.warn { color: #d4b26a; }
.raw-log-entry.error { color: #ec8f8f; }
.raw-log-entry.status { color: #8b949e; }

@media (max-width: 720px) {
    .mmat-root {
        padding: 0.75rem;
    }

    .mmat-shell {
        min-height: calc(100vh - 1.5rem);
        border-radius: 14px;
    }

    .mmat-header {
        flex-direction: column;
        align-items: flex-start;
        padding: 0.95rem 1rem;
    }

    .mmat-header-meta {
        gap: 0.55rem;
    }

    .mmat-conversation {
        padding: 1rem 1rem 0;
    }

    .conversation-entry {
        max-width: 100%;
    }

    .mmat-composer {
        padding: 0.95rem 1rem 1rem;
    }

    .composer-row {
        grid-template-columns: 1fr;
    }

    .composer-btn {
        width: 100%;
    }
}
"#;

pub struct WsHandle {
    shutdown_tx: watch::Sender<bool>,
    join_handle: tokio::task::JoinHandle<Result<(), WsError>>,
}

pub struct WsReadyHandle {
    shutdown_tx: watch::Sender<bool>,
    join_handle: tokio::task::JoinHandle<Result<(), WsError>>,
    ready_rx: oneshot::Receiver<Result<(), std::io::Error>>,
}

pub struct WsAppBuilder {
    addr: SocketAddr,
    ui_state: Arc<UiState>,
}

#[derive(Props, Clone)]
struct RootAppProps {
    ui_state: Arc<UiState>,
}

#[derive(Debug, thiserror::Error)]
pub enum WsError {
    #[error("bind failed: {0}")]
    Bind(std::io::Error),

    #[error("server failed: {0}")]
    Serve(std::io::Error),

    #[error("websocket task failed: {0}")]
    Task(String),
}

impl Default for WsAppBuilder {
    fn default() -> Self {
        Self {
            addr: DEFAULT_ADDR
                .parse()
                .expect("default socket address should parse"),
            ui_state: Arc::new(UiState::new()),
        }
    }
}

impl PartialEq for RootAppProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ui_state, &other.ui_state)
    }
}

impl WsAppBuilder {
    pub fn addr(mut self, addr: SocketAddr) -> Self {
        self.addr = addr;
        self
    }

    pub fn with_ui_state(mut self, ui_state: Arc<UiState>) -> Self {
        self.ui_state = ui_state;
        self
    }

    pub fn spawn_with_input(
        self,
    ) -> Result<
        (
            EventSender,
            WsReadyHandle,
            InstructionReceiver,
            mpsc::UnboundedReceiver<FrontendEvent>,
        ),
        WsError,
    > {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (instruction_tx, instruction_rx) = oneshot::channel();
        let handle = spawn_server_with_input(self.addr, self.ui_state, instruction_tx)?;
        Ok((event_tx, handle, instruction_rx, event_rx))
    }
}

impl WsReadyHandle {
    pub async fn wait_for_ready(self) -> Result<WsHandle, WsError> {
        let ready_result = self
            .ready_rx
            .await
            .map_err(|_| WsError::Task("server shut down before binding".into()))?;
        ready_result.map_err(WsError::Bind)?;
        Ok(WsHandle {
            shutdown_tx: self.shutdown_tx,
            join_handle: self.join_handle,
        })
    }
}

impl WsHandle {
    pub async fn shutdown(self) -> Result<(), WsError> {
        let _ = self.shutdown_tx.send(true);
        self.join_handle
            .await
            .map_err(|error| WsError::Task(error.to_string()))?
    }
}

#[allow(non_snake_case)]
fn RootApp(props: RootAppProps) -> Element {
    let state = props.ui_state.clone();
    let snapshot = state.snapshot();

    let header_badge = header_badge_text(&snapshot.composer_mode, snapshot.run_summary.as_ref());

    let composer_key = format!(
        "{}-{}-{}-{}",
        snapshot.conversation.len(),
        composer_mode_key(&snapshot.composer_mode),
        usize::from(snapshot.pending_prompt.is_some()),
        snapshot
            .run_summary
            .as_ref()
            .map(|summary| format!("{}-{}", summary.status, summary.current_stage))
            .unwrap_or_default()
    );

    rsx! {
        div { class: "mmat-root",
            div { class: "mmat-shell",
                AppHeader { badge_text: header_badge }
                div { class: "mmat-content",
                    div { id: "mmat-conversation", class: "mmat-conversation",
                        for (index , entry) in snapshot.conversation.iter().enumerate() {
                            {render_conversation_entry(index, entry)}
                        }
                        if let Some(prompt) = &snapshot.pending_prompt {
                            if !has_trailing_question(&snapshot.conversation, &prompt.question) {
                                div { class: "conversation-entry question", "{prompt.question}" }
                            }
                        }
                        if matches!(snapshot.composer_mode, ComposerMode::Working) {
                            if let Some(summary) = &snapshot.run_summary {
                                div { class: "conversation-entry status", "{format_run_summary(summary)}" }
                            }
                        }
                    }
                    div { class: "mmat-composer",
                        Composer {
                            key: "{composer_key}",
                            ui_state: props.ui_state.clone(),
                            mode: snapshot.composer_mode.clone(),
                            choices: snapshot.pending_prompt.as_ref().and_then(|p| p.choices.clone()),
                        }
                        RawLogsDisclosure { history: snapshot.history.clone() }
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct AppHeaderProps {
    badge_text: String,
}

#[allow(non_snake_case)]
fn AppHeader(props: AppHeaderProps) -> Element {
    rsx! {
        div { class: "mmat-header",
            div { class: "mmat-brand",
                pre { class: "mmat-logo", "aria-hidden": "true", "|\\/| |\\/|  /\\  T\n|  | |  | /--\\ |" }
            }
            div { class: "mmat-header-meta",
                div { class: "header-badge", "{props.badge_text}" }
            }
        }
    }
}

#[allow(non_snake_case)]
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
                class: if *complete {
                    "conversation-entry reasoning"
                } else {
                    "conversation-entry reasoning pending"
                },
                div { class: "reasoning-label", if *complete { "Reasoning" } else { "Reasoning..." } }
                "{text}"
            }
        },
        ConversationEntry::AssistantMessage { text, .. } => rsx! {
            div { key: "conv-{index}", class: "conversation-entry assistant", "{text}" }
        },
    }
}

fn has_trailing_question(
    conversation: &std::collections::VecDeque<ConversationEntry>,
    question: &str,
) -> bool {
    matches!(
        conversation.back(),
        Some(ConversationEntry::AssistantQuestion {
            question: existing_question,
        }) if existing_question == question
    )
}

fn composer_mode_key(mode: &ComposerMode) -> &'static str {
    match mode {
        ComposerMode::InitialPrompt => "initial",
        ComposerMode::Reply => "reply",
        ComposerMode::Working => "working",
    }
}

fn header_badge_text(mode: &ComposerMode, summary: Option<&crate::models::RunSummary>) -> String {
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

fn format_run_summary(summary: &crate::models::RunSummary) -> String {
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

#[derive(Props, Clone)]
struct ComposerProps {
    ui_state: Arc<UiState>,
    mode: ComposerMode,
    choices: Option<Vec<String>>,
}

impl PartialEq for ComposerProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ui_state, &other.ui_state)
            && self.mode == other.mode
            && self.choices == other.choices
    }
}

#[allow(non_snake_case)]
fn Composer(props: ComposerProps) -> Element {
    let mut input = use_signal(String::new);
    let mode = props.mode.clone();
    let is_working = matches!(mode, ComposerMode::Working);
    let textarea_value = if is_working {
        String::new()
    } else {
        input.read().clone()
    };

    let mut prev_mode = use_signal(|| mode.clone());
    let mut clear_input = input;
    let mode_for_effect = mode.clone();
    use_effect(move || {
        if is_working && !clear_input.read().is_empty() {
            clear_input.set(String::new());
        }
        if prev_mode() != mode_for_effect && !clear_input.read().is_empty() {
            clear_input.set(String::new());
        }
        prev_mode.set(mode_for_effect.clone());
    });

    let (btn_label, btn_disabled) = match &mode {
        ComposerMode::InitialPrompt => ("Start", false),
        ComposerMode::Reply => ("Reply", false),
        ComposerMode::Working => ("Working...", true),
    };

    let submit_click = "return window.mmatSubmitComposer(this);";
    let keydown_shortcut = "return window.mmatHandleComposerKeydown(event, this);";
    let submit_choice = "return window.mmatSubmitChoice(this);";

    rsx! {
        if let Some(choices) = &props.choices {
            if !choices.is_empty() {
                div { class: "composer-choices",
                    for choice in choices {
                        button {
                            class: "composer-choice-btn",
                            r#type: "button",
                            "data-choice": "{choice}",
                            "onclick": submit_choice,
                            "{choice}"
                        }
                    }
                }
            }
        }
        div { class: "composer-row",
            textarea {
                class: "composer-textarea",
                value: "{textarea_value}",
                oninput: move |e| input.set(e.value()),
                "onkeydown": keydown_shortcut,
                placeholder: match &mode {
                    ComposerMode::InitialPrompt => "Describe what you want to build...",
                    ComposerMode::Reply => "Type your reply...",
                    ComposerMode::Working => "Working... You can draft the next message here.",
                },
                rows: "2",
            }
            button {
                class: "composer-btn primary",
                r#type: "button",
                "onclick": submit_click,
                disabled: btn_disabled,
                "{btn_label}"
            }
        }
        div {
            class: "composer-hint",
            "Cmd+Enter to submit"
        }
    }
}

#[derive(Props, Clone)]
struct RawLogsDisclosureProps {
    history: std::collections::VecDeque<crate::ws::ui_state::UiEventEntry>,
}

impl PartialEq for RawLogsDisclosureProps {
    fn eq(&self, other: &Self) -> bool {
        self.history == other.history
    }
}

#[allow(non_snake_case)]
fn RawLogsDisclosure(props: RawLogsDisclosureProps) -> Element {
    if props.history.is_empty() {
        return rsx! {};
    }

    rsx! {
        details {
            summary { class: "raw-logs-toggle",
                span { class: "raw-logs-label-closed", "Show raw logs" }
                span { class: "raw-logs-label-open", "Hide raw logs" }
            }
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

fn log_level_class(event: &crate::ws::UiEvent) -> &'static str {
    match event {
        crate::ws::UiEvent::Log { level, .. } => match level.to_lowercase().as_str() {
            "warn" | "warning" => "warn",
            "error" => "error",
            _ => "info",
        },
        crate::ws::UiEvent::StepStarted { .. }
        | crate::ws::UiEvent::StepCompleted { .. }
        | crate::ws::UiEvent::StepFailed { .. }
        | crate::ws::UiEvent::ComponentStarted { .. }
        | crate::ws::UiEvent::ComponentCompleted { .. }
        | crate::ws::UiEvent::ComponentFailed { .. } => "status",
    }
}

fn format_event(event: &crate::ws::UiEvent) -> String {
    match event {
        crate::ws::UiEvent::Log { level, message } => format!("[{level}] {message}"),
        crate::ws::UiEvent::StepStarted { task_label, .. } => {
            format!("▶ {task_label}")
        }
        crate::ws::UiEvent::StepCompleted {
            task_label,
            attempts,
            ..
        } => {
            format!("✔ {task_label} ({attempts} attempts)")
        }
        crate::ws::UiEvent::StepFailed {
            task_label, stage, ..
        } => {
            format!("✘ {task_label} ({stage})")
        }
        crate::ws::UiEvent::ComponentStarted { component, name } => {
            format!("[{component}] started: {name}")
        }
        crate::ws::UiEvent::ComponentCompleted { component, name } => {
            format!("[{component}] completed: {name}")
        }
        crate::ws::UiEvent::ComponentFailed { component, name } => {
            format!("[{component}] failed: {name}")
        }
    }
}

async fn run_server(
    addr: SocketAddr,
    ui_state: Arc<UiState>,
    _shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    ready_tx: oneshot::Sender<Result<(), std::io::Error>>,
) -> Result<(), WsError> {
    let title = "MMAT";
    let index_html = axum::response::Html(format!(
        r#"<!DOCTYPE html>
<html>
    <head>
        <title>{title}</title>
        <style>{APP_STYLES}</style>
    </head>
    <body>
        <div id="main" class="mmat-root">
            <div class="mmat-shell">
                <div class="mmat-header">
                    <div class="mmat-brand">
                        <pre class="mmat-logo" aria-hidden="true">|\/| |\/|  /\  T
|  | |  | /--\ |</pre>
                    </div>
                    <div class="mmat-header-meta">
                        <div id="header-badge" class="header-badge">Connecting...</div>
                    </div>
                </div>
                <div class="mmat-content">
                    <div id="mmat-conversation" class="mmat-conversation">
                        <div class="conversation-entry connecting">Connecting...</div>
                    </div>
                    <div class="mmat-composer">
                        <div id="composer-choices" class="composer-choices" style="display:none"></div>
                        <div class="composer-row">
                            <textarea id="composer-textarea" class="composer-textarea" rows="2" placeholder="Describe what you want to build..."></textarea>
                            <button id="composer-submit" class="composer-btn primary" type="button">Start</button>
                        </div>
                        <div class="composer-hint">Cmd+Enter to submit</div>
                        <details id="raw-logs-disclosure" style="display:none">
                            <summary class="raw-logs-toggle">
                                <span class="raw-logs-label-closed">Show raw logs</span>
                                <span class="raw-logs-label-open">Hide raw logs</span>
                            </summary>
                            <div id="raw-logs-container" class="raw-logs-container"></div>
                        </details>
                    </div>
                </div>
            </div>
        </div>
    </body>
    <script>
    (function() {{
        const SCROLL_THRESHOLD = 80;
        const SNAPSHOT_INTERVAL_MS = 50;
        let autoScrollEnabled = true;
        let conversationEl = null;
        let textareaEl = null;
        let submitButtonEl = null;
        let choicesEl = null;
        let headerBadgeEl = null;
        let rawLogsDisclosureEl = null;
        let rawLogsContainerEl = null;
        let lastMode = null;
        let pollInFlight = false;

        function isNearBottom(el) {{
            return el.scrollHeight - el.scrollTop - el.clientHeight < SCROLL_THRESHOLD;
        }}

        function scrollToBottom(el) {{
            el.scrollTop = el.scrollHeight;
        }}

        function variantName(value) {{
            if (!value || typeof value !== 'object') {{
                return null;
            }}

            const keys = Object.keys(value);
            return keys.length > 0 ? keys[0] : null;
        }}

        function variantData(value) {{
            const name = variantName(value);
            return name ? value[name] : null;
        }}

        function escapeHtml(text) {{
            return String(text)
                .replace(/&/g, '&amp;')
                .replace(/</g, '&lt;')
                .replace(/>/g, '&gt;')
                .replace(/"/g, '&quot;')
                .replace(/'/g, '&#39;');
        }}

        function headerBadgeText(snapshot) {{
            const summary = snapshot.run_summary;
            if (summary) {{
                switch (summary.status) {{
                    case 'running':
                        return 'Running: ' + summary.current_stage.replaceAll('_', ' ');
                    case 'awaiting_clarification':
                        return 'Awaiting clarification';
                    case 'awaiting_approval':
                        return 'Awaiting proposal approval';
                    case 'awaiting_contract_approval':
                        return 'Awaiting contract approval';
                    case 'revising':
                        return 'Revising: ' + summary.current_stage.replaceAll('_', ' ');
                    default:
                        return summary.status.replaceAll('_', ' ');
                }}
            }}

            switch (snapshot.composer_mode) {{
                case 'InitialPrompt':
                    return 'Ready for a new run';
                case 'Reply':
                    return 'Awaiting your reply';
                default:
                    return 'Working';
            }}
        }}

        function formatRunSummary(summary) {{
            const stage = summary.current_stage.replaceAll('_', ' ');
            switch (summary.status) {{
                case 'awaiting_clarification':
                    return 'Waiting for clarification during ' + stage + '.';
                case 'awaiting_approval':
                    return 'Waiting for proposal approval.';
                case 'awaiting_contract_approval':
                    return 'Waiting for contract approval.';
                case 'revising':
                    return 'Revising after feedback in ' + stage + '.';
                case 'running':
                    return 'Working on ' + stage + '.';
                default:
                    return summary.status + ' (' + stage + ')';
            }}
        }}

        function formatEvent(event) {{
            const kind = variantName(event);
            const data = variantData(event) || {{}};
            switch (kind) {{
                case 'Log':
                    return '[' + data.level + '] ' + data.message;
                case 'StepStarted':
                    return '> ' + data.task_label;
                case 'StepCompleted':
                    return 'ok ' + data.task_label + ' (' + data.attempts + ' attempts)';
                case 'StepFailed':
                    return 'x ' + data.task_label + ' (' + data.stage + ')';
                case 'ComponentStarted':
                    return '[' + data.component + '] started: ' + data.name;
                case 'ComponentCompleted':
                    return '[' + data.component + '] completed: ' + data.name;
                case 'ComponentFailed':
                    return '[' + data.component + '] failed: ' + data.name;
                default:
                    return '';
            }}
        }}

        function logLevelClass(event) {{
            const kind = variantName(event);
            const data = variantData(event) || {{}};
            if (kind === 'Log') {{
                const level = String(data.level || '').toLowerCase();
                if (level === 'warn' || level === 'warning') {{
                    return 'warn';
                }}
                if (level === 'error') {{
                    return 'error';
                }}
                return 'info';
            }}
            return 'status';
        }}

        function hasTrailingQuestion(conversation, question) {{
            if (!conversation.length) {{
                return false;
            }}

            const last = conversation[conversation.length - 1];
            return variantName(last) === 'AssistantQuestion' && (variantData(last) || {{}}).question === question;
        }}

        function renderConversation(snapshot) {{
            const wasNearBottom = conversationEl && isNearBottom(conversationEl);
            const entries = [];

            for (const entry of snapshot.conversation) {{
                const kind = variantName(entry);
                const data = variantData(entry) || {{}};
                if (kind === 'UserMessage') {{
                    entries.push('<div class="conversation-entry user">' + escapeHtml(data.text || '') + '</div>');
                }} else if (kind === 'AssistantQuestion') {{
                    entries.push('<div class="conversation-entry question">' + escapeHtml(data.question || '') + '</div>');
                }} else if (kind === 'AssistantReasoning') {{
                    const cls = data.complete ? 'conversation-entry reasoning' : 'conversation-entry reasoning pending';
                    const label = data.complete ? 'Reasoning' : 'Reasoning...';
                    entries.push('<div class="' + cls + '"><div class="reasoning-label">' + label + '</div>' + escapeHtml(data.text || '') + '</div>');
                }} else if (kind === 'AssistantMessage') {{
                    entries.push('<div class="conversation-entry assistant">' + escapeHtml(data.text || '') + '</div>');
                }}
            }}

            if (snapshot.pending_prompt && !hasTrailingQuestion(snapshot.conversation, snapshot.pending_prompt.question)) {{
                entries.push('<div class="conversation-entry question">' + escapeHtml(snapshot.pending_prompt.question) + '</div>');
            }}

            if (snapshot.composer_mode === 'Working' && snapshot.run_summary) {{
                entries.push('<div class="conversation-entry status">' + escapeHtml(formatRunSummary(snapshot.run_summary)) + '</div>');
            }}

            conversationEl.innerHTML = entries.join('');
            if (autoScrollEnabled && wasNearBottom) {{
                requestAnimationFrame(function() {{
                    scrollToBottom(conversationEl);
                }});
            }}
        }}

        function renderChoices(snapshot) {{
            const choices = snapshot.pending_prompt && snapshot.pending_prompt.choices ? snapshot.pending_prompt.choices : [];
            if (!choices.length) {{
                choicesEl.style.display = 'none';
                choicesEl.innerHTML = '';
                return;
            }}

            choicesEl.style.display = 'flex';
            choicesEl.innerHTML = choices.map(function(choice) {{
                return '<button class="composer-choice-btn" type="button" data-choice="' + escapeHtml(choice) + '">' + escapeHtml(choice) + '</button>';
            }}).join('');
        }}

        function renderLogs(snapshot) {{
            if (!snapshot.history.length) {{
                rawLogsDisclosureEl.style.display = 'none';
                rawLogsContainerEl.innerHTML = '';
                return;
            }}

            rawLogsDisclosureEl.style.display = 'block';
            rawLogsContainerEl.innerHTML = snapshot.history.map(function(entry) {{
                return '<div class="raw-log-entry ' + logLevelClass(entry.event) + '">' + escapeHtml(formatEvent(entry.event)) + '</div>';
            }}).join('');
        }}

        function applySnapshot(snapshot) {{
            headerBadgeEl.textContent = headerBadgeText(snapshot);
            renderConversation(snapshot);
            renderChoices(snapshot);
            renderLogs(snapshot);

            const mode = snapshot.composer_mode;
            const isWorking = mode === 'Working';
            const modeChanged = lastMode !== null && lastMode !== mode;

            if (isWorking || modeChanged) {{
                textareaEl.value = '';
            }}

            if (mode === 'InitialPrompt') {{
                submitButtonEl.textContent = 'Start';
                textareaEl.placeholder = 'Describe what you want to build...';
            }} else if (mode === 'Reply') {{
                submitButtonEl.textContent = 'Reply';
                textareaEl.placeholder = 'Type your reply...';
            }} else {{
                submitButtonEl.textContent = 'Working...';
                textareaEl.placeholder = 'Working... You can draft the next message here.';
            }}

            submitButtonEl.disabled = isWorking;
            lastMode = mode;
        }}

        async function loadSnapshot() {{
            if (pollInFlight) {{
                return;
            }}

            pollInFlight = true;
            try {{
                const response = await fetch('/snapshot', {{ cache: 'no-store' }});
                if (!response.ok) {{
                    return;
                }}

                const snapshot = await response.json();
                applySnapshot(snapshot);
            }} catch (_error) {{
            }} finally {{
                pollInFlight = false;
            }}
        }}

        async function submitText(text) {{
            if (!text) {{
                return false;
            }}

            submitButtonEl.disabled = true;
            submitButtonEl.textContent = 'Working...';

            try {{
                const response = await fetch('/submit', {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'text/plain;charset=UTF-8' }},
                    body: text,
                }});

                if (!response.ok) {{
                    submitButtonEl.disabled = false;
                    return false;
                }}

                textareaEl.value = '';
                void loadSnapshot();
                return false;
            }} catch (_error) {{
                submitButtonEl.disabled = false;
                return false;
            }}
        }}

        async function submitComposer() {{
            if (!textareaEl || !submitButtonEl || submitButtonEl.disabled) {{
                return false;
            }}

            return submitText(textareaEl.value);
        }}

        window.mmatSubmitComposer = function(element) {{
            void submitComposer();
            return false;
        }};

        window.mmatSubmitChoice = function(element) {{
            const choice = element.getAttribute('data-choice');
            if (!choice) {{
                return false;
            }}

            void submitText(choice);
            return false;
        }};

        window.mmatHandleComposerKeydown = function(event, textarea) {{
            const isEnter = event.key === 'Enter' || event.key === 'Return' || event.keyCode === 13 || event.which === 13;
            if (!isEnter || event.isComposing || !(event.metaKey || event.ctrlKey)) {{
                return true;
            }}

            event.preventDefault();
            void submitComposer();
            return false;
        }};

        function initUi() {{
            conversationEl = document.getElementById('mmat-conversation');
            textareaEl = document.getElementById('composer-textarea');
            submitButtonEl = document.getElementById('composer-submit');
            choicesEl = document.getElementById('composer-choices');
            headerBadgeEl = document.getElementById('header-badge');
            rawLogsDisclosureEl = document.getElementById('raw-logs-disclosure');
            rawLogsContainerEl = document.getElementById('raw-logs-container');

            submitButtonEl.addEventListener('click', function() {{
                void submitComposer();
            }});

            textareaEl.addEventListener('keydown', function(event) {{
                void window.mmatHandleComposerKeydown(event, textareaEl);
            }});

            choicesEl.addEventListener('click', function(event) {{
                const button = event.target.closest('.composer-choice-btn');
                if (button) {{
                    void window.mmatSubmitChoice(button);
                }}
            }});

            conversationEl.addEventListener('scroll', function() {{
                if (isNearBottom(conversationEl)) {{
                    autoScrollEnabled = true;
                }} else {{
                    autoScrollEnabled = false;
                }}
            }});

            void loadSnapshot();
            window.setInterval(loadSnapshot, SNAPSHOT_INTERVAL_MS);
        }}

        if (document.readyState === 'loading') {{
            document.addEventListener('DOMContentLoaded', initUi);
        }} else {{
            initUi();
        }}
    }})();
    </script>
    </html>"#
    ));

    let liveview_state = ui_state.clone();
    let submit_state = ui_state.clone();
    let snapshot_state = ui_state.clone();

    let app = Router::create_default_liveview_router()
        .with_virtual_dom("/", move || {
            let state = liveview_state.clone();
            VirtualDom::new_with_props(RootApp, RootAppProps { ui_state: state })
        })
        .route(
            "/submit",
            axum::routing::post({
                let ui_state = submit_state.clone();
                move |body: String| {
                    let ui_state = ui_state.clone();
                    async move {
                        if body.is_empty() {
                            return StatusCode::BAD_REQUEST;
                        }

                        let submitted = if ui_state.pending_initial_input.lock().is_some() {
                            ui_state.send_initial_input(body)
                        } else {
                            ui_state.send_pending_prompt(body)
                        };

                        if submitted {
                            StatusCode::NO_CONTENT
                        } else {
                            StatusCode::CONFLICT
                        }
                    }
                }
            }),
        )
        .route(
            "/snapshot",
            axum::routing::get({
                let ui_state = snapshot_state.clone();
                move || {
                    let ui_state = ui_state.clone();
                    async move { Json(ui_state.snapshot()) }
                }
            }),
        )
        .route(
            "/",
            axum::routing::get(move || async move { index_html.clone() }),
        );

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            let kind = error.kind();
            let _ = ready_tx.send(Err(std::io::Error::new(kind, error.to_string())));
            return Err(WsError::Bind(error));
        }
    };

    let _ = ready_tx.send(Ok(()));

    info!(target: "mmat::ws", "LiveView front end listening on http://{addr}");

    let server = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(wait_for_shutdown(shutdown_rx));
    server.await.map_err(WsError::Serve)
}

fn spawn_server(addr: SocketAddr, ui_state: Arc<UiState>) -> Result<WsReadyHandle, WsError> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (ready_tx, ready_rx) = oneshot::channel();
    let join_handle = tokio::spawn(run_server(
        addr,
        ui_state,
        shutdown_tx.clone(),
        shutdown_rx,
        ready_tx,
    ));

    Ok(WsReadyHandle {
        shutdown_tx,
        join_handle,
        ready_rx,
    })
}

fn spawn_server_with_input(
    addr: SocketAddr,
    ui_state: Arc<UiState>,
    instruction_tx: oneshot::Sender<String>,
) -> Result<WsReadyHandle, WsError> {
    {
        let mut pending = ui_state.pending_initial_input.lock();
        *pending = Some(instruction_tx);
    }

    spawn_server(addr, ui_state)
}

async fn wait_for_shutdown(mut shutdown_rx: watch::Receiver<bool>) {
    while !*shutdown_rx.borrow() {
        if shutdown_rx.changed().await.is_err() {
            break;
        }
    }
}
