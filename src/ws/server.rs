use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
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
    let tick = use_signal(|| 0u64);

    let state_for_hook = state.clone();
    use_hook(move || {
        let state = state_for_hook.clone();
        let mut tick = tick;
        spawn(async move {
            let mut rx = state.subscribe();
            while rx.changed().await.is_ok() {
                tick += 1;
            }
        });
    });

    let snapshot = use_resource(move || {
        let state = state.clone();
        let _t = tick();
        async move { state.snapshot() }
    });

    let snapshot = match snapshot() {
        Some(s) => s,
        None => {
            return rsx! {
                div { class: "mmat-root",
                    div { class: "mmat-conversation",
                        div { class: "conversation-entry connecting", "Connecting..." }
                    }
                }
            };
        }
    };

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
        document::Style {
            "
            * {{ margin: 0; padding: 0; box-sizing: border-box; }}
            html, body {{ height: 100%; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #1a1a2e; color: #e0e0e0; }}
            .mmat-root {{ display: flex; flex-direction: column; height: 100vh; max-width: 900px; margin: 0 auto; }}
            .mmat-conversation {{ flex: 1; overflow-y: auto; padding: 1rem; display: flex; flex-direction: column; gap: 0.75rem; }}
            .conversation-entry {{ padding: 0.75rem 1rem; border-radius: 8px; font-size: 0.95rem; line-height: 1.5; max-width: 85%; white-space: pre-wrap; }}
            .conversation-entry.user {{ align-self: flex-end; background: #5a5aff; color: #fff; border-bottom-right-radius: 2px; }}
            .conversation-entry.assistant {{ align-self: flex-start; background: #2a2a4a; color: #c0c0d0; border-bottom-left-radius: 2px; }}
            .conversation-entry.question {{ align-self: flex-start; background: #1e3a5f; color: #a0d0ff; border-bottom-left-radius: 2px; }}
            .conversation-entry.status {{ align-self: center; background: transparent; color: #808090; font-size: 0.8rem; padding: 0.25rem 0.5rem; max-width: 100%; }}
            .conversation-entry.connecting {{ align-self: center; background: transparent; color: #808090; font-size: 0.8rem; }}
            .mmat-composer {{ border-top: 1px solid #2a2a4a; padding: 1rem; background: #16213e; display: flex; flex-direction: column; gap: 0.5rem; }}
            .composer-row {{ display: flex; gap: 0.5rem; align-items: flex-end; }}
            .composer-textarea {{ flex: 1; padding: 0.75rem 1rem; border: 1px solid #3a3a5a; border-radius: 6px; background: #0f3460; color: #e0e0e0; font-size: 0.95rem; outline: none; font-family: inherit; resize: none; }}
            .composer-textarea:focus {{ border-color: #5a5aff; }}
            .composer-textarea:disabled {{ opacity: 0.5; cursor: not-allowed; }}
            .composer-btn {{ padding: 0.6rem 1.5rem; border: none; border-radius: 6px; font-size: 0.95rem; font-weight: 600; cursor: pointer; white-space: nowrap; }}
            .composer-btn.primary {{ background: #5a5aff; color: #fff; }}
            .composer-btn.primary:hover:not(:disabled) {{ background: #4a4aee; }}
            .composer-btn:disabled {{ background: #3a3a5a; color: #808090; cursor: not-allowed; }}
            .composer-choices {{ display: flex; flex-wrap: wrap; gap: 0.5rem; }}
            .composer-choice-btn {{ padding: 0.5rem 1rem; border: 1px solid #3a3a5a; border-radius: 6px; background: #0f3460; color: #e0e0e0; font-size: 0.875rem; cursor: pointer; }}
            .composer-choice-btn:hover {{ background: #1a4a70; border-color: #5a5aff; }}
            .raw-logs-toggle {{ padding: 0.5rem 1rem; background: transparent; border: 1px solid #3a3a5a; border-radius: 6px; color: #808090; font-size: 0.8rem; cursor: pointer; text-align: left; }}
            .raw-logs-toggle:hover {{ border-color: #5a5aff; color: #a0a0b0; }}
            .raw-logs-container {{ padding: 0.75rem 1rem; background: #0d1117; border-radius: 6px; font-family: 'SF Mono', 'Fira Code', monospace; font-size: 0.8rem; line-height: 1.4; max-height: 300px; overflow-y: auto; }}
            .raw-log-entry {{ padding: 0.15rem 0; white-space: pre-wrap; word-break: break-word; }}
            .raw-log-entry.info {{ color: #c9d1d9; }}
            .raw-log-entry.warn {{ color: #d29922; }}
            .raw-log-entry.error {{ color: #f85149; }}
            .raw-log-entry.status {{ color: #8b949e; }}
            .composer-hint {{ font-size: 0.75rem; color: #606070; }}
            "
        }
        div { class: "mmat-root",
            div { class: "mmat-conversation",
                for entry in snapshot.conversation.iter() {
                    {render_conversation_entry(entry)}
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

#[allow(non_snake_case)]
fn render_conversation_entry(entry: &ConversationEntry) -> Element {
    match entry {
        ConversationEntry::UserMessage { text } => rsx! {
            div { class: "conversation-entry user", "{text}" }
        },
        ConversationEntry::AssistantQuestion { question } => rsx! {
            div { class: "conversation-entry question", "{question}" }
        },
        ConversationEntry::AssistantMessage { text } => rsx! {
            div { class: "conversation-entry assistant", "{text}" }
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
    let reset_nonce = use_signal(|| 0u64);
    let ui_state = props.ui_state.clone();
    let mode = props.mode.clone();
    let is_working = matches!(mode, ComposerMode::Working);
    let textarea_key = format!("{}-{}", composer_mode_key(&mode), reset_nonce());
    let textarea_value = if is_working {
        String::new()
    } else {
        input.read().clone()
    };

    use_effect(move || {
        if is_working && !input.read().is_empty() {
            input.set(String::new());
        }
    });

    let (btn_label, btn_disabled) = match &mode {
        ComposerMode::InitialPrompt => ("Start", false),
        ComposerMode::Reply => ("Reply", false),
        ComposerMode::Working => ("Working...", true),
    };

    let keydown_mode = mode.clone();
    let keydown_ui_state = ui_state.clone();
    let mut keydown_input = input;
    let mut keydown_reset_nonce = reset_nonce;
    let click_mode = mode.clone();
    let click_ui_state = ui_state.clone();
    let mut click_input = input;
    let mut click_reset_nonce = reset_nonce;

    rsx! {
        if let Some(choices) = &props.choices {
            if !choices.is_empty() {
                div { class: "composer-choices",
                    for choice in choices {
                        button {
                            class: "composer-choice-btn",
                            onclick: {
                                let ui_state = props.ui_state.clone();
                                let choice = choice.clone();
                                move |_| {
                                    ui_state.send_pending_prompt(choice.clone());
                                }
                            },
                            "{choice}"
                        }
                    }
                }
            }
        }
        div { class: "composer-row",
            textarea {
                key: "{textarea_key}",
                class: "composer-textarea",
                value: "{textarea_value}",
                oninput: move |e| input.set(e.value()),
                placeholder: match &mode {
                    ComposerMode::InitialPrompt => "Describe what you want to build...",
                    ComposerMode::Reply => "Type your reply...",
                    ComposerMode::Working => "Working... You can draft the next message here.",
                },
                rows: "2",
                onkeydown: move |e| {
                    if e.key() == Key::Enter
                        && e.modifiers().shift()
                        && matches!(keydown_mode, ComposerMode::InitialPrompt | ComposerMode::Reply)
                    {
                        e.prevent_default();
                        let text = keydown_input.read().clone();
                        if text.is_empty() {
                            return;
                        }
                        keydown_input.set(String::new());
                        let sent = match &keydown_mode {
                            ComposerMode::InitialPrompt => {
                                keydown_ui_state.send_initial_input(text.clone())
                            }
                            ComposerMode::Reply => {
                                keydown_ui_state.send_pending_prompt(text.clone())
                            }
                            ComposerMode::Working => false,
                        };
                        if !sent {
                            keydown_input.set(text);
                        } else {
                            keydown_reset_nonce += 1;
                        }
                    }
                },
            }
            button {
                class: "composer-btn primary",
                onclick: move |_| {
                    let text = click_input.read().clone();
                    if text.is_empty() {
                        return;
                    }
                    click_input.set(String::new());
                    let sent = match &click_mode {
                        ComposerMode::InitialPrompt => {
                            click_ui_state.send_initial_input(text.clone())
                        }
                        ComposerMode::Reply => {
                            click_ui_state.send_pending_prompt(text.clone())
                        }
                        ComposerMode::Working => false,
                    };
                    if !sent {
                        click_input.set(text);
                    } else {
                        click_reset_nonce += 1;
                    }
                },
                disabled: btn_disabled,
                "{btn_label}"
            }
        }
        div { class: "composer-hint", "Shift+Enter submits, Enter adds a newline" }
    }
}

#[derive(Props, Clone)]
struct RawLogsDisclosureProps {
    history: std::collections::VecDeque<crate::ws::UiEvent>,
}

impl PartialEq for RawLogsDisclosureProps {
    fn eq(&self, other: &Self) -> bool {
        self.history == other.history
    }
}

#[allow(non_snake_case)]
fn RawLogsDisclosure(props: RawLogsDisclosureProps) -> Element {
    let mut expanded = use_signal(|| false);

    if props.history.is_empty() {
        return rsx! {};
    }

    rsx! {
        div {
            button {
                class: "raw-logs-toggle",
                onclick: move |_| expanded.toggle(),
                if expanded() {
                    "Hide raw logs ({props.history.len()})"
                } else {
                    "Show raw logs ({props.history.len()})"
                }
            }
            if expanded() {
                div { class: "raw-logs-container",
                    for event in props.history.iter() {
                        div { class: "raw-log-entry {log_level_class(event)}",
                            "{format_event(event)}"
                        }
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
    let glue = dioxus_liveview::interpreter_glue("/ws");
    let title = "MMAT";
    let index_html = axum::response::Html(format!(
        r#"<!DOCTYPE html>
<html>
    <head><title>{title}</title></head>
    <body><div id="main"></div></body>
    {glue}
</html>"#
    ));

    let app = Router::create_default_liveview_router()
        .with_virtual_dom("/", move || {
            let state = ui_state.clone();
            VirtualDom::new_with_props(RootApp, RootAppProps { ui_state: state })
        })
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
