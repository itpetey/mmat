use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use dioxus::prelude::*;
use dioxus_core::VirtualDom;
use dioxus_liveview::LiveviewRouter;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::info;

use crate::ws::event::FrontendEvent;
use crate::ws::ui_state::UiState;

pub type EventSender = mpsc::UnboundedSender<FrontendEvent>;
pub type InstructionReceiver = oneshot::Receiver<String>;

const DEFAULT_ADDR: &str = "127.0.0.1:8080";

pub struct WsHandle {
    shutdown_tx: watch::Sender<bool>,
    join_handle: tokio::task::JoinHandle<Result<(), WsError>>,
}

pub struct WsAppBuilder {
    addr: SocketAddr,
    ui_state: Arc<UiState>,
}

#[derive(Props, Clone)]
struct RootAppProps {
    ui_state: Arc<UiState>,
}

#[derive(Props, Clone)]
struct InitialInputCardProps {
    ui_state: Arc<UiState>,
}

#[derive(Props, Clone)]
struct PromptCardProps {
    ui_state: Arc<UiState>,
    question: String,
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

impl PartialEq for InitialInputCardProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ui_state, &other.ui_state)
    }
}

impl PartialEq for PromptCardProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ui_state, &other.ui_state) && self.question == other.question
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

    #[allow(dead_code)]
    pub fn spawn(
        self,
    ) -> Result<
        (
            EventSender,
            WsHandle,
            mpsc::UnboundedReceiver<FrontendEvent>,
        ),
        WsError,
    > {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let handle = spawn_server(self.addr, self.ui_state)?;
        Ok((event_tx, handle, event_rx))
    }

    pub fn spawn_with_input(
        self,
    ) -> Result<
        (
            EventSender,
            WsHandle,
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

impl WsHandle {
    pub async fn shutdown(self) -> Result<(), WsError> {
        let _ = self.shutdown_tx.send(true);
        self.join_handle
            .await
            .map_err(|error| WsError::Task(error.to_string()))?
    }
}

#[allow(non_snake_case)]
fn InitialInputCard(props: InitialInputCardProps) -> Element {
    let mut input = use_signal(String::new);
    let ui_state1 = props.ui_state.clone();
    let ui_state2 = props.ui_state.clone();

    rsx! {
        div { class: "prompt-card",
            p { "What are we building?" }
            textarea {
                class: "prompt-input",
                value: "{input}",
                oninput: move |e| input.set(e.value()),
                placeholder: "Describe what you want to build...",
                rows: "3",
                onkeydown: move |e| {
                    if e.key() == Key::Enter && e.modifiers().shift() {
                        let text = input.read().clone();
                        if !text.is_empty() {
                            let mut pending = ui_state1.pending_initial_input.lock();
                            if let Some(sender) = pending.take() {
                                let _ = sender.send(text);
                            }
                        }
                    }
                },
            }
            button {
                class: "prompt-submit",
                onclick: move |_| {
                    let text = input.read().clone();
                    if !text.is_empty() {
                        let mut pending = ui_state2.pending_initial_input.lock();
                        if let Some(sender) = pending.take() {
                            let _ = sender.send(text);
                        }
                    }
                },
                "Start"
            }
        }
    }
}

#[allow(non_snake_case)]
fn PromptCard(props: PromptCardProps) -> Element {
    let mut input = use_signal(String::new);
    let ui_state1 = props.ui_state.clone();
    let ui_state2 = props.ui_state.clone();

    rsx! {
        div { class: "prompt-card",
            p { "{props.question}" }
            textarea {
                class: "prompt-input",
                value: "{input}",
                oninput: move |e| input.set(e.value()),
                placeholder: "Type your reply...",
                rows: "3",
                onkeydown: move |e| {
                    if e.key() == Key::Enter && e.modifiers().shift() {
                        let text = input.read().clone();
                        if !text.is_empty() {
                            let mut pending = ui_state1.pending_prompt.lock();
                            if let Some(prompt) = pending.take() {
                                let _ = prompt.reply.send(text);
                            }
                        }
                    }
                },
            }
            button {
                class: "prompt-submit",
                onclick: move |_| {
                    let text = input.read().clone();
                    if !text.is_empty() {
                        let mut pending = ui_state2.pending_prompt.lock();
                        if let Some(prompt) = pending.take() {
                            let _ = prompt.reply.send(text);
                        }
                    }
                },
                "Reply"
            }
        }
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
                    div { class: "mmat-transcript",
                        div { class: "transcript-entry", "Connecting..." }
                    }
                }
            };
        }
    };

    rsx! {
        document::Style {
            "
            * {{ margin: 0; padding: 0; box-sizing: border-box; }}
            html, body {{ height: 100%; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #1a1a2e; color: #e0e0e0; }}
            .mmat-root {{ display: flex; flex-direction: column; height: 100vh; max-width: 900px; margin: 0 auto; }}
            .mmat-transcript {{ flex: 1; overflow-y: auto; padding: 1rem; display: flex; flex-direction: column; gap: 0.25rem; }}
            .transcript-entry {{ padding: 0.35rem 0.5rem; border-radius: 4px; font-size: 0.875rem; line-height: 1.4; font-family: 'SF Mono', 'Fira Code', monospace; }}
            .mmat-composer {{ border-top: 1px solid #2a2a4a; padding: 1rem; background: #16213e; }}
            .prompt-card {{ display: flex; flex-direction: column; gap: 0.75rem; }}
            .prompt-card p {{ font-size: 1rem; line-height: 1.5; color: #c0c0d0; }}
            .prompt-input {{ width: 100%; padding: 0.75rem 1rem; border: 1px solid #3a3a5a; border-radius: 6px; background: #0f3460; color: #e0e0e0; font-size: 0.95rem; outline: none; }}
            .prompt-input:focus {{ border-color: #5a5aff; }}
            .prompt-submit {{ align-self: flex-end; padding: 0.6rem 1.5rem; border: none; border-radius: 6px; background: #5a5aff; color: #fff; font-size: 0.95rem; font-weight: 600; cursor: pointer; }}
            .prompt-submit:hover {{ background: #4a4aee; }}
            "
        }
        div { class: "mmat-root",
            div { class: "mmat-transcript",
                for event in snapshot.history.iter() {
                    div { class: "transcript-entry",
                        "{format_event(event)}"
                    }
                }
            }
            div { class: "mmat-composer",
                if snapshot.has_pending_input {
                    InitialInputCard { ui_state: props.ui_state.clone() }
                } else if let Some(prompt) = snapshot.pending_prompt {
                    PromptCard {
                        ui_state: props.ui_state.clone(),
                        question: prompt.question,
                    }
                } else {
                    div { class: "prompt-card",
                        p { "Working..." }
                    }
                }
            }
        }
    }
}

fn format_event(event: &crate::ws::UiEvent) -> String {
    match event {
        crate::ws::UiEvent::Log {
            level,
            target: _,
            message,
        } => format!("[{level}] {message}"),
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
        crate::ws::UiEvent::PlanningTriggered => "Planning started.".to_string(),
    }
}

async fn run_server(
    addr: SocketAddr,
    ui_state: Arc<UiState>,
    _shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
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

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(WsError::Bind)?;

    info!(target: "mmat::ws", "LiveView front end listening on http://{addr}");

    let server = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(wait_for_shutdown(shutdown_rx));
    server.await.map_err(WsError::Serve)
}

fn spawn_server(addr: SocketAddr, ui_state: Arc<UiState>) -> Result<WsHandle, WsError> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let join_handle = tokio::spawn(run_server(addr, ui_state, shutdown_tx.clone(), shutdown_rx));

    Ok(WsHandle {
        shutdown_tx,
        join_handle,
    })
}

fn spawn_server_with_input(
    addr: SocketAddr,
    ui_state: Arc<UiState>,
    instruction_tx: oneshot::Sender<String>,
) -> Result<WsHandle, WsError> {
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
