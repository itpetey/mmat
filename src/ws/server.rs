use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::sync::{Mutex as AsyncMutex, broadcast, mpsc, oneshot, watch};
use tracing::{info, warn};
use uuid::Uuid;

use crate::ws::event::FrontendEvent;
use crate::ws::event::{WsClientMessage, WsEvent};

const DEFAULT_ADDR: &str = "127.0.0.1:8080";
const DEFAULT_EVENT_BUFFER: usize = 256;

pub type EventSender = mpsc::UnboundedSender<FrontendEvent>;
pub type InstructionReceiver = oneshot::Receiver<String>;

pub struct WsAppBuilder {
    addr: SocketAddr,
    input_screen: Option<String>,
}

impl WsAppBuilder {
    pub fn addr(mut self, addr: SocketAddr) -> Self {
        self.addr = addr;
        self
    }

    pub fn with_input_screen(mut self, label: impl Into<String>) -> Self {
        self.input_screen = Some(label.into());
        self
    }

    #[allow(dead_code)]
    pub fn spawn(self) -> Result<(EventSender, WsHandle), WsError> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let handle = spawn_server(self.addr, self.input_screen, None, event_rx)?;
        Ok((event_tx, handle))
    }

    pub fn spawn_with_input(self) -> Result<(EventSender, WsHandle, InstructionReceiver), WsError> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (instruction_tx, instruction_rx) = oneshot::channel();
        let handle = spawn_server(self.addr, self.input_screen, Some(instruction_tx), event_rx)?;
        Ok((event_tx, handle, instruction_rx))
    }
}

impl Default for WsAppBuilder {
    fn default() -> Self {
        Self {
            addr: DEFAULT_ADDR
                .parse()
                .expect("default socket address should parse"),
            input_screen: None,
        }
    }
}

pub struct WsHandle {
    shutdown_tx: watch::Sender<bool>,
    join_handle: tokio::task::JoinHandle<Result<(), WsError>>,
}

impl WsHandle {
    pub async fn shutdown(self) -> Result<(), WsError> {
        let _ = self.shutdown_tx.send(true);
        self.join_handle
            .await
            .map_err(|error| WsError::Task(error.to_string()))?
    }
}

struct PendingPrompt {
    question: String,
    choices: Vec<String>,
    reply: oneshot::Sender<String>,
}

struct PendingInput {
    label: String,
    reply: oneshot::Sender<String>,
}

struct AppState {
    broadcaster: broadcast::Sender<WsEvent>,
    pending_prompts: HashMap<String, PendingPrompt>,
    pending_input: Option<PendingInput>,
}

#[derive(Clone)]
struct RouterState {
    shared: Arc<Mutex<AppState>>,
}

fn spawn_server(
    addr: SocketAddr,
    input_screen: Option<String>,
    instruction_tx: Option<oneshot::Sender<String>>,
    event_rx: mpsc::UnboundedReceiver<FrontendEvent>,
) -> Result<WsHandle, WsError> {
    let (broadcaster, _) = broadcast::channel(DEFAULT_EVENT_BUFFER);
    let state = Arc::new(Mutex::new(AppState {
        broadcaster,
        pending_prompts: HashMap::new(),
        pending_input: input_screen
            .zip(instruction_tx)
            .map(|(label, reply)| PendingInput { label, reply }),
    }));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let join_handle = tokio::spawn(run_server(
        addr,
        state,
        event_rx,
        shutdown_tx.clone(),
        shutdown_rx,
    ));

    Ok(WsHandle {
        shutdown_tx,
        join_handle,
    })
}

async fn run_server(
    addr: SocketAddr,
    state: Arc<Mutex<AppState>>,
    event_rx: mpsc::UnboundedReceiver<FrontendEvent>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), WsError> {
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(RouterState {
            shared: state.clone(),
        });

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(WsError::Bind)?;

    info!(target: "mmat::ws", "WebSocket front end listening on ws://{addr}/ws");

    let event_task = tokio::spawn(run_event_loop(state.clone(), event_rx, shutdown_tx.clone()));

    let server = axum::serve(listener, app).with_graceful_shutdown(wait_for_shutdown(shutdown_rx));
    let server_result = server.await.map_err(WsError::Serve);

    event_task.abort();

    server_result
}

async fn wait_for_shutdown(mut shutdown_rx: watch::Receiver<bool>) {
    while !*shutdown_rx.borrow() {
        if shutdown_rx.changed().await.is_err() {
            break;
        }
    }
}

async fn run_event_loop(
    state: Arc<Mutex<AppState>>,
    mut event_rx: mpsc::UnboundedReceiver<FrontendEvent>,
    shutdown_tx: watch::Sender<bool>,
) {
    if let Some(input_event) = current_input_event(&state) {
        broadcast_event(&state, input_event);
    }

    while let Some(event) = event_rx.recv().await {
        match event {
            FrontendEvent::HumanPrompt {
                question,
                choices,
                reply,
            } => {
                let prompt_id = Uuid::new_v4().to_string();
                {
                    let mut app = state.lock();
                    app.pending_prompts.insert(
                        prompt_id.clone(),
                        PendingPrompt {
                            question: question.clone(),
                            choices: choices.clone(),
                            reply,
                        },
                    );
                }
                broadcast_event(
                    &state,
                    WsEvent::HumanPrompt {
                        prompt_id,
                        question,
                        choices,
                    },
                );
            }
            FrontendEvent::Quit => {
                let _ = shutdown_tx.send(true);
                break;
            }
            other => {
                if let Some(event) = WsEvent::from_frontend_event(other) {
                    broadcast_event(&state, event);
                }
            }
        }
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<RouterState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state.shared))
}

async fn handle_socket(socket: WebSocket, state: Arc<Mutex<AppState>>) {
    let (writer, mut reader) = socket.split();
    let writer = Arc::new(AsyncMutex::new(writer));
    let mut rx = {
        let app = state.lock();
        app.broadcaster.subscribe()
    };

    for event in initial_events(&state) {
        if send_event(&writer, &event).await.is_err() {
            return;
        }
    }

    let mut write_task = {
        let writer = writer.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if send_event(&writer, &event).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!(target: "mmat::ws", "skipped {skipped} websocket event(s)");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    };

    let mut read_task = {
        let state = state.clone();
        let writer = writer.clone();
        tokio::spawn(async move {
            while let Some(result) = reader.next().await {
                match result {
                    Ok(Message::Text(text)) => {
                        if handle_client_message(&state, &text).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Ping(payload)) => {
                        let mut writer = writer.lock().await;
                        if writer.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(error) => {
                        warn!(target: "mmat::ws", "websocket error: {error}");
                        break;
                    }
                }
            }
        })
    };

    tokio::select! {
        _ = &mut write_task => {
            read_task.abort();
        }
        _ = &mut read_task => {
            write_task.abort();
        }
    }
}

async fn handle_client_message(state: &Arc<Mutex<AppState>>, text: &str) -> Result<(), WsError> {
    match serde_json::from_str::<WsClientMessage>(text).map_err(WsError::DecodeClientMessage)? {
        WsClientMessage::HumanPromptReply { prompt_id, reply } => {
            let pending = {
                let mut app = state.lock();
                app.pending_prompts.remove(&prompt_id)
            };

            if let Some(prompt) = pending {
                let _ = prompt.reply.send(reply);
            } else {
                warn!(target: "mmat::ws", "received reply for unknown prompt: {prompt_id}");
            }
        }
        WsClientMessage::InputReply { reply } => {
            let pending = {
                let mut app = state.lock();
                app.pending_input.take()
            };

            if let Some(input) = pending {
                let _ = input.reply.send(reply);
            } else {
                warn!(target: "mmat::ws", "received unexpected input reply");
            }
        }
    }

    Ok(())
}

fn initial_events(state: &Arc<Mutex<AppState>>) -> Vec<WsEvent> {
    let app = state.lock();
    let mut events = Vec::new();

    if let Some(input) = &app.pending_input {
        events.push(WsEvent::InputRequested {
            label: input.label.clone(),
        });
    }

    events.extend(
        app.pending_prompts
            .iter()
            .map(|(prompt_id, prompt)| WsEvent::HumanPrompt {
                prompt_id: prompt_id.clone(),
                question: prompt.question.clone(),
                choices: prompt.choices.clone(),
            }),
    );

    events
}

fn current_input_event(state: &Arc<Mutex<AppState>>) -> Option<WsEvent> {
    let app = state.lock();
    app.pending_input
        .as_ref()
        .map(|input| WsEvent::InputRequested {
            label: input.label.clone(),
        })
}

fn broadcast_event(state: &Arc<Mutex<AppState>>, event: WsEvent) {
    let broadcaster = {
        let app = state.lock();
        app.broadcaster.clone()
    };
    let _ = broadcaster.send(event);
}

async fn send_event(
    writer: &Arc<AsyncMutex<futures::stream::SplitSink<WebSocket, Message>>>,
    event: &WsEvent,
) -> Result<(), WsError> {
    let payload = serde_json::to_string(event).map_err(WsError::EncodeEvent)?;
    let mut writer = writer.lock().await;
    writer
        .send(Message::Text(payload.into()))
        .await
        .map_err(WsError::Socket)
}

#[derive(Debug, thiserror::Error)]
pub enum WsError {
    #[error("bind failed: {0}")]
    Bind(std::io::Error),

    #[error("server failed: {0}")]
    Serve(std::io::Error),

    #[error("websocket task failed: {0}")]
    Task(String),

    #[error("failed to encode websocket event: {0}")]
    EncodeEvent(serde_json::Error),

    #[error("failed to decode websocket client message: {0}")]
    DecodeClientMessage(serde_json::Error),

    #[error("websocket send failed: {0}")]
    Socket(axum::Error),
}
