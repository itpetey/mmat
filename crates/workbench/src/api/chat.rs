use dioxus::{
    fullstack::{WebSocketOptions, Websocket},
    prelude::*,
};
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
use std::sync::OnceLock;

#[cfg(feature = "server")]
static WORKBENCH_RUNTIME: OnceLock<mmat_coordinator::WorkbenchRuntimeHandle> = OnceLock::new();
#[cfg(feature = "server")]
static WORKBENCH_RUNTIME_OWNER: OnceLock<mmat_coordinator::OrganisationRuntime> = OnceLock::new();

pub const SYSTEM_LANE_ID: &str = "__system__";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkbenchLane {
    pub id: String,
    pub title: String,
    pub status: String,
    pub system: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneProjection {
    pub active: Vec<WorkbenchLane>,
    pub archived: Vec<WorkbenchLane>,
    pub system: WorkbenchLane,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptItem {
    pub id: String,
    pub lane_id: Option<String>,
    pub speaker: String,
    pub content: String,
    pub kind: TranscriptItemKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptItemKind {
    Message,
    Log,
    Error,
}

/// Messages accepted from the browser chat client over the workbench WebSocket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatClientMessage {
    /// Submit a user-authored message for backend processing.
    SendMessage {
        project_id: String,
        lane_id: Option<String>,
        client_message_id: Option<String>,
        content: String,
    },
    /// Cancel an in-flight assistant stream.
    Cancel { assistant_message_id: String },
    /// Keepalive probe from the client.
    Ping { nonce: Option<String> },
}

/// Messages emitted by the workbench chat backend over the WebSocket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatServerMessage {
    /// Sent once the server accepts the WebSocket upgrade.
    Connected { session_id: String },
    /// Confirms that a user message was accepted by the backend connection.
    UserMessageAccepted {
        lane_id: String,
        client_message_id: Option<String>,
        message_id: String,
        content: String,
        timestamp_ms: u64,
    },
    /// Signals that an assistant stream has started.
    AssistantStreamStarted {
        lane_id: String,
        message_id: String,
        reply_to_message_id: String,
        timestamp_ms: u64,
    },
    /// Carries a live assistant content delta.
    AssistantStreamDelta {
        lane_id: String,
        message_id: String,
        delta: String,
    },
    /// Signals that an assistant stream completed and was persisted.
    AssistantStreamCompleted {
        lane_id: String,
        message_id: String,
        reply_to_message_id: String,
        content: String,
        finish_reason: String,
    },
    /// Signals that an assistant stream failed before completion.
    AssistantStreamFailed {
        lane_id: String,
        message_id: String,
        reply_to_message_id: String,
        reason: String,
    },
    /// Confirms that the backend received a cancellation request.
    Cancelled { assistant_message_id: String },
    /// Signals that lane navigation/projections changed for a project.
    ProjectionChanged { project_id: String },
    /// Keepalive response from the server.
    Pong { nonce: Option<String> },
    /// Recoverable connection-level error.
    Error { message: String },
}

/// Upgrade the chat API request into a typed WebSocket connection.
#[get("/api/chat")]
pub async fn connect_chat(
    options: WebSocketOptions,
) -> ServerFnResult<Websocket<ChatClientMessage, ChatServerMessage>> {
    Ok(options.on_upgrade(handle_chat_socket))
}

#[server]
pub async fn load_lanes(project_id: String) -> ServerFnResult<LaneProjection> {
    let mut connection = super::db()
        .await?
        .get()
        .await
        .map_err(super::db_connection_error)?;
    ensure_project_exists(&mut connection, &project_id).await?;
    let active = mmat_db::lane::load_lanes_by_status(&mut connection, &project_id, "active")
        .await
        .map_err(|error| ServerFnError::new(format!("could not load active lanes: {error}")))?
        .into_iter()
        .map(lane_from_row)
        .collect();
    let archived = mmat_db::lane::load_lanes_by_status(&mut connection, &project_id, "archived")
        .await
        .map_err(|error| ServerFnError::new(format!("could not load archived lanes: {error}")))?
        .into_iter()
        .map(lane_from_row)
        .collect();

    Ok(lane_projection_from_rows(active, archived))
}

#[server]
pub async fn create_lane(project_id: String, title: String) -> ServerFnResult<WorkbenchLane> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(ServerFnError::new("Lane title is required."));
    }

    let mut connection = super::db()
        .await?
        .get()
        .await
        .map_err(super::db_connection_error)?;
    ensure_project_exists(&mut connection, &project_id).await?;
    let now = mmat_db::now_timestamp_string();
    let lane = mmat_db::models::NewLane {
        project_id: project_id.clone(),
        title: title.clone(),
        summary: String::new(),
        status: "active".to_string(),
        creator: "human".to_string(),
        parent_lane_id: None,
        origin_event_id: None,
        origin_message_id: None,
        created_at: now.clone(),
        updated_at: now,
        archived_at: None,
    };
    let (lane, event) = mmat_db::lane::create_lane_with_event(&mut connection, lane, |lane| {
        lane_created_event(&lane.id.to_string(), &title, &project_id)
    })
    .await
    .map_err(|error| ServerFnError::new(format!("could not create lane: {error}")))?;

    runtime_handle()
        .await?
        .broadcast_persisted(event)
        .map_err(|error| ServerFnError::new(format!("could not publish lane event: {error}")))?;

    Ok(lane_from_row(lane))
}

#[server]
pub async fn archive_lane(project_id: String, lane_id: String) -> ServerFnResult<WorkbenchLane> {
    if lane_id == SYSTEM_LANE_ID {
        return Err(ServerFnError::new("The System lane cannot be archived."));
    }

    let mut connection = super::db()
        .await?
        .get()
        .await
        .map_err(super::db_connection_error)?;
    let lane = validate_lane(&mut connection, &project_id, &lane_id, false).await?;
    let event = lane_archived_event(&lane_id, &project_id);
    let lane = mmat_db::lane::archive_lane_with_event(
        &mut connection,
        &lane.id.to_string(),
        mmat_db::now_timestamp_string(),
        event.clone(),
    )
    .await
    .map_err(|error| ServerFnError::new(format!("could not archive lane: {error}")))?;
    runtime_handle()
        .await?
        .broadcast_persisted(event)
        .map_err(|error| ServerFnError::new(format!("could not publish lane event: {error}")))?;
    Ok(lane_from_row(lane))
}

#[server]
pub async fn load_transcript(
    project_id: String,
    lane_id: Option<String>,
) -> ServerFnResult<Vec<TranscriptItem>> {
    let mut connection = super::db()
        .await?
        .get()
        .await
        .map_err(super::db_connection_error)?;
    ensure_project_exists(&mut connection, &project_id).await?;
    let events = mmat_db::event::replay_events(&mut connection, 0, None)
        .await
        .map_err(|error| ServerFnError::new(format!("could not replay events: {error}")))?;

    Ok(events
        .iter()
        .filter(|event| event.context().project_id == project_id)
        .filter(|event| transcript_matches_lane(event, lane_id.as_deref()))
        .filter_map(transcript_item_from_event)
        .collect())
}

#[cfg(feature = "server")]
async fn handle_chat_socket(
    mut socket: dioxus::fullstack::TypedWebsocket<ChatClientMessage, ChatServerMessage>,
) {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    let session_id = next_id("chat-session");
    let Ok(runtime) = runtime_handle().await else {
        return;
    };
    let runtime = runtime.clone();
    let mut workbench_events = runtime.subscribe(&[]);
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<ChatServerMessage>(128);
    let (shutdown_tx, _) = tokio::sync::watch::channel(false);
    let _shutdown_guard = SocketShutdown::new(shutdown_tx.clone());
    let local_messages = Arc::new(Mutex::new(HashSet::<String>::new()));
    let cancelled_streams = Arc::new(Mutex::new(HashSet::<String>::new()));

    if socket
        .send(ChatServerMessage::Connected { session_id })
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            message = socket.recv() => {
                let message = match message {
                    Ok(message) => message,
                    Err(_) => return,
                };

                match message {
                    ChatClientMessage::SendMessage {
                        project_id,
                        lane_id,
                        client_message_id,
                        content,
                    } => {
                        let runtime = runtime.clone();
                        let out_tx = out_tx.clone();
                        let shutdown_rx = shutdown_tx.subscribe();
                        let local_messages = Arc::clone(&local_messages);
                        let cancelled_streams = Arc::clone(&cancelled_streams);
                        tokio::spawn(async move {
                            handle_user_message(
                                UserMessageContext {
                                    runtime,
                                    out_tx,
                                    shutdown_rx,
                                    local_messages,
                                    cancelled_streams,
                                },
                                project_id,
                                lane_id,
                                client_message_id,
                                content,
                            )
                            .await;
                        });
                    }
                    ChatClientMessage::Cancel {
                        assistant_message_id,
                    } => {
                        mark_cancelled_stream(&cancelled_streams, assistant_message_id.clone());
                        runtime.cancel_assistant_stream(&assistant_message_id).await;
                        if socket
                            .send(ChatServerMessage::Cancelled {
                                assistant_message_id,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    ChatClientMessage::Ping { nonce } => {
                        if socket
                            .send(ChatServerMessage::Pong { nonce })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            event = workbench_events.recv() => {
                let event = match event {
                    Ok(event) => event,
                    Err(mmat_event_stream::event_bus::RecvError::Lagged(_)) => continue,
                    Err(mmat_event_stream::event_bus::RecvError::Closed) => return,
                };

                if let Some(message) = chat_server_message_from_event(event.as_ref()) {
                    if is_suppressed_local_message(&local_messages, &message) {
                        continue;
                    }
                    if socket.send(message).await.is_err() {
                        return;
                    }
                }
            }
            outbound = out_rx.recv() => {
                let Some(outbound) = outbound else {
                    return;
                };
                if socket.send(outbound).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(feature = "server")]
struct UserMessageContext {
    runtime: mmat_coordinator::WorkbenchRuntimeHandle,
    out_tx: tokio::sync::mpsc::Sender<ChatServerMessage>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
    local_messages: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    cancelled_streams: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

#[cfg(feature = "server")]
async fn handle_user_message(
    ctx: UserMessageContext,
    project_id: String,
    lane_id: Option<String>,
    client_message_id: Option<String>,
    content: String,
) {
    use mmat_event_stream::event::{EventContext, RoleId, SemanticEvent};

    let UserMessageContext {
        runtime,
        out_tx,
        mut shutdown_rx,
        local_messages,
        cancelled_streams,
    } = ctx;

    let content = content.trim().to_string();
    if content.is_empty() {
        send_chat(
            &out_tx,
            ChatServerMessage::Error {
                message: "Message content is required.".to_string(),
            },
        )
        .await;
        return;
    }

    let Some(lane_id) = lane_id.filter(|id| id != SYSTEM_LANE_ID) else {
        send_chat(
            &out_tx,
            ChatServerMessage::Error {
                message: "Select or create a lane before sending a message.".to_string(),
            },
        )
        .await;
        return;
    };

    let pool = match super::db().await {
        Ok(pool) => pool,
        Err(error) => {
            send_chat(
                &out_tx,
                ChatServerMessage::Error {
                    message: format!("Could not open database pool: {error}"),
                },
            )
            .await;
            return;
        }
    };
    let mut connection = match pool.get().await {
        Ok(connection) => connection,
        Err(error) => {
            send_chat(
                &out_tx,
                ChatServerMessage::Error {
                    message: format!("Could not open database connection: {error}"),
                },
            )
            .await;
            return;
        }
    };

    if let Err(error) = validate_lane(&mut connection, &project_id, &lane_id, true).await {
        send_chat(
            &out_tx,
            ChatServerMessage::Error {
                message: error.to_string(),
            },
        )
        .await;
        return;
    }

    let event = SemanticEvent::new_human_feedback_received(RoleId::new("human"), &content)
        .with_context(
            EventContext::new(
                "default-organisation",
                "default-workspace",
                project_id.clone(),
                "default-run",
            )
            .with_lane_id(lane_id.clone()),
        );
    let message_id = event.event_id().to_string();
    suppress_local_message(&local_messages, message_id.clone());
    if let Err(error) = runtime.publish_durable(event).await {
        send_chat(
            &out_tx,
            ChatServerMessage::Error {
                message: format!("Could not persist message: {error}"),
            },
        )
        .await;
        return;
    }

    if !send_chat(
        &out_tx,
        ChatServerMessage::UserMessageAccepted {
            lane_id: lane_id.clone(),
            client_message_id,
            message_id: message_id.clone(),
            content: content.clone(),
            timestamp_ms: now_ms(),
        },
    )
    .await
    {
        return;
    }

    let assistant_message_id = next_id("assistant-message");
    let stream_request = mmat_coordinator::AssistantStreamRequest {
        project_id: project_id.clone(),
        lane_id: lane_id.clone(),
        assistant_message_id: assistant_message_id.clone(),
        reply_to_message_id: message_id.clone(),
        user_content: content,
    };
    let mut stream = match runtime.start_assistant_stream(stream_request).await {
        Ok(stream) => stream,
        Err(error) => {
            send_chat(
                &out_tx,
                ChatServerMessage::Error {
                    message: format!("Assistant stream could not start: {error}"),
                },
            )
            .await;
            return;
        }
    };

    if !send_chat(
        &out_tx,
        ChatServerMessage::AssistantStreamStarted {
            lane_id: lane_id.clone(),
            message_id: assistant_message_id.clone(),
            reply_to_message_id: message_id.clone(),
            timestamp_ms: now_ms(),
        },
    )
    .await
    {
        runtime.cancel_assistant_stream(&assistant_message_id).await;
        return;
    }

    let mut assistant_content = String::new();
    loop {
        let update = tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    runtime.cancel_assistant_stream(&assistant_message_id).await;
                    return;
                }
                continue;
            }
            update = stream.recv() => update,
        };

        let Some(update) = update else {
            if consume_cancelled_stream(&cancelled_streams, &assistant_message_id) {
                return;
            }
            send_chat(
                &out_tx,
                ChatServerMessage::AssistantStreamFailed {
                    lane_id: lane_id.clone(),
                    message_id: assistant_message_id.clone(),
                    reply_to_message_id: message_id.clone(),
                    reason: "Assistant stream ended before completion.".to_string(),
                },
            )
            .await;
            return;
        };

        match update {
            mmat_coordinator::AssistantStreamEvent::Delta { content } => {
                assistant_content.push_str(&content);
                if !send_chat(
                    &out_tx,
                    ChatServerMessage::AssistantStreamDelta {
                        lane_id: lane_id.clone(),
                        message_id: assistant_message_id.clone(),
                        delta: content,
                    },
                )
                .await
                {
                    runtime.cancel_assistant_stream(&assistant_message_id).await;
                    return;
                }
            }
            mmat_coordinator::AssistantStreamEvent::Finished { finish_reason } => {
                if *shutdown_rx.borrow() {
                    runtime.cancel_assistant_stream(&assistant_message_id).await;
                    return;
                }
                let event = SemanticEvent::new_assistant_message_produced(
                    RoleId::new("assistant"),
                    &assistant_message_id,
                    &message_id,
                    &assistant_content,
                    &finish_reason,
                )
                .with_context(
                    EventContext::new(
                        "default-organisation",
                        "default-workspace",
                        project_id.clone(),
                        "default-run",
                    )
                    .with_lane_id(lane_id.clone()),
                );
                suppress_local_message(&local_messages, assistant_message_id.clone());
                let publish_result = tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            runtime.cancel_assistant_stream(&assistant_message_id).await;
                            return;
                        }
                        continue;
                    }
                    result = runtime.publish_durable(event) => result,
                };
                if let Err(error) = publish_result {
                    send_chat(
                        &out_tx,
                        ChatServerMessage::AssistantStreamFailed {
                            lane_id: lane_id.clone(),
                            message_id: assistant_message_id.clone(),
                            reply_to_message_id: message_id.clone(),
                            reason: format!("Could not persist assistant message: {error}"),
                        },
                    )
                    .await;
                    return;
                }
                if !send_chat(
                    &out_tx,
                    ChatServerMessage::AssistantStreamCompleted {
                        lane_id: lane_id.clone(),
                        message_id: assistant_message_id.clone(),
                        reply_to_message_id: message_id.clone(),
                        content: assistant_content,
                        finish_reason,
                    },
                )
                .await
                {
                    return;
                }
                return;
            }
            mmat_coordinator::AssistantStreamEvent::Failed { message } => {
                send_chat(
                    &out_tx,
                    ChatServerMessage::AssistantStreamFailed {
                        lane_id: lane_id.clone(),
                        message_id: assistant_message_id.clone(),
                        reply_to_message_id: message_id.clone(),
                        reason: message,
                    },
                )
                .await;
                return;
            }
        }
    }
}

#[cfg(feature = "server")]
async fn send_chat(
    out_tx: &tokio::sync::mpsc::Sender<ChatServerMessage>,
    message: ChatServerMessage,
) -> bool {
    out_tx.send(message).await.is_ok()
}

#[cfg(feature = "server")]
struct SocketShutdown {
    tx: tokio::sync::watch::Sender<bool>,
}

#[cfg(feature = "server")]
impl SocketShutdown {
    fn new(tx: tokio::sync::watch::Sender<bool>) -> Self {
        Self { tx }
    }
}

#[cfg(feature = "server")]
impl Drop for SocketShutdown {
    fn drop(&mut self) {
        if self.tx.send(true).is_err() {
            // All stream tasks have already stopped.
        }
    }
}

#[cfg(feature = "server")]
fn suppress_local_message(
    local_messages: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    message_id: String,
) {
    if let Ok(mut local_messages) = local_messages.lock() {
        local_messages.insert(message_id);
    }
}

#[cfg(feature = "server")]
fn is_suppressed_local_message(
    local_messages: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    message: &ChatServerMessage,
) -> bool {
    let Some(message_id) = server_message_id(message) else {
        return false;
    };
    local_messages
        .lock()
        .map(|mut local_messages| local_messages.remove(message_id))
        .unwrap_or(false)
}

#[cfg(feature = "server")]
fn mark_cancelled_stream(
    cancelled_streams: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    assistant_message_id: String,
) {
    if let Ok(mut cancelled_streams) = cancelled_streams.lock() {
        cancelled_streams.insert(assistant_message_id);
    }
}

#[cfg(feature = "server")]
fn consume_cancelled_stream(
    cancelled_streams: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    assistant_message_id: &str,
) -> bool {
    cancelled_streams
        .lock()
        .map(|mut cancelled_streams| cancelled_streams.remove(assistant_message_id))
        .unwrap_or(false)
}

#[cfg(feature = "server")]
fn server_message_id(message: &ChatServerMessage) -> Option<&str> {
    match message {
        ChatServerMessage::UserMessageAccepted { message_id, .. }
        | ChatServerMessage::AssistantStreamStarted { message_id, .. }
        | ChatServerMessage::AssistantStreamDelta { message_id, .. }
        | ChatServerMessage::AssistantStreamCompleted { message_id, .. }
        | ChatServerMessage::AssistantStreamFailed { message_id, .. } => Some(message_id),
        _ => None,
    }
}

#[cfg(feature = "server")]
fn lane_created_event(
    lane_id: &str,
    title: &str,
    project_id: &str,
) -> mmat_event_stream::event::SemanticEvent {
    use mmat_event_stream::event::{EventContext, RoleId, SemanticEvent};

    SemanticEvent::new_lane_created(
        RoleId::new("human"),
        lane_id,
        title,
        "conversation",
        "",
        "",
        None,
        Vec::new(),
        None,
        None,
    )
    .with_context(EventContext::new(
        "default-organisation",
        "default-workspace",
        project_id,
        "default-run",
    ))
}

#[cfg(feature = "server")]
async fn runtime_handle() -> ServerFnResult<&'static mmat_coordinator::WorkbenchRuntimeHandle> {
    if let Some(runtime) = WORKBENCH_RUNTIME.get() {
        return Ok(runtime);
    }

    let pool = (*super::db().await?).clone();
    let runtime_owner = mmat_coordinator::OrganisationRuntime::new_workbench_boundary(
        mmat_coordinator::OrganisationConfig::new(crate::cli::pg_dsn()),
    )
    .await
    .map_err(|error| ServerFnError::new(format!("could not initialise runtime: {error}")))?;
    if WORKBENCH_RUNTIME_OWNER.set(runtime_owner).is_err() {
        // Another request initialised the shared runtime first.
    }
    let runtime_owner = WORKBENCH_RUNTIME_OWNER
        .get()
        .ok_or_else(|| ServerFnError::new("runtime owner was not initialised"))?;
    let runtime = runtime_owner.workbench_handle(pool, crate::cli::llm_config());
    if WORKBENCH_RUNTIME.set(runtime).is_err() {
        // Another request initialised the shared runtime first.
    }

    WORKBENCH_RUNTIME
        .get()
        .ok_or_else(|| ServerFnError::new("runtime handle was not initialised"))
}

#[cfg(feature = "server")]
fn lane_archived_event(lane_id: &str, project_id: &str) -> mmat_event_stream::event::SemanticEvent {
    use mmat_event_stream::event::{EventContext, RoleId, SemanticEvent};

    SemanticEvent::new_lane_archived(RoleId::new("human"), lane_id).with_context(EventContext::new(
        "default-organisation",
        "default-workspace",
        project_id,
        "default-run",
    ))
}

#[cfg(feature = "server")]
async fn validate_lane(
    connection: &mut mmat_db::AsyncPgConnection,
    project_id: &str,
    lane_id: &str,
    require_active: bool,
) -> ServerFnResult<mmat_db::models::Lane> {
    let lane = mmat_db::lane::get_lane(connection, lane_id)
        .await
        .map_err(|error| ServerFnError::new(format!("could not load lane: {error}")))?
        .ok_or_else(|| ServerFnError::new("Lane does not exist."))?;

    if lane.project_id != project_id {
        return Err(ServerFnError::new(
            "Lane does not belong to the selected project.",
        ));
    }
    if require_active && lane.status != "active" {
        return Err(ServerFnError::new(
            "Cannot send messages to an archived lane.",
        ));
    }

    Ok(lane)
}

#[cfg(feature = "server")]
async fn ensure_project_exists(
    connection: &mut mmat_db::AsyncPgConnection,
    project_id: &str,
) -> ServerFnResult<()> {
    let exists = mmat_db::project::project_exists(connection, project_id)
        .await
        .map_err(|error| ServerFnError::new(format!("could not validate project: {error}")))?;

    if exists {
        Ok(())
    } else {
        Err(ServerFnError::new("Project does not exist."))
    }
}

#[cfg(feature = "server")]
fn chat_server_message_from_event(
    event: &mmat_event_stream::event::SemanticEvent,
) -> Option<ChatServerMessage> {
    match event {
        mmat_event_stream::event::SemanticEvent::HumanFeedbackReceived {
            event_id,
            timestamp_ns,
            context,
            answer,
            ..
        } => Some(ChatServerMessage::UserMessageAccepted {
            lane_id: context.lane_id.clone()?,
            client_message_id: None,
            message_id: event_id.to_string(),
            content: answer.clone(),
            timestamp_ms: timestamp_ns / 1_000_000,
        }),
        mmat_event_stream::event::SemanticEvent::AssistantMessageProduced {
            context,
            assistant_message_id,
            reply_to_message_id,
            content,
            finish_reason,
            ..
        } => Some(ChatServerMessage::AssistantStreamCompleted {
            lane_id: context.lane_id.clone()?,
            message_id: assistant_message_id.clone(),
            reply_to_message_id: reply_to_message_id.clone(),
            content: content.clone(),
            finish_reason: finish_reason.clone(),
        }),
        mmat_event_stream::event::SemanticEvent::LaneCreated { context, .. }
        | mmat_event_stream::event::SemanticEvent::LaneArchived { context, .. } => {
            Some(ChatServerMessage::ProjectionChanged {
                project_id: context.project_id.clone(),
            })
        }
        _ => None,
    }
}

#[cfg(feature = "server")]
fn next_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{id}")
}

#[cfg(feature = "server")]
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(feature = "server")]
fn system_lane() -> WorkbenchLane {
    WorkbenchLane {
        id: SYSTEM_LANE_ID.to_string(),
        title: "System".to_string(),
        status: "system".to_string(),
        system: true,
    }
}

#[cfg(feature = "server")]
fn lane_from_row(lane: mmat_db::models::Lane) -> WorkbenchLane {
    WorkbenchLane {
        id: lane.id.to_string(),
        title: lane.title,
        status: lane.status,
        system: false,
    }
}

#[cfg(feature = "server")]
fn lane_projection_from_rows(
    active: Vec<WorkbenchLane>,
    archived: Vec<WorkbenchLane>,
) -> LaneProjection {
    LaneProjection {
        active,
        archived,
        system: system_lane(),
    }
}

#[cfg(feature = "server")]
fn transcript_matches_lane(
    event: &mmat_event_stream::event::SemanticEvent,
    lane_id: Option<&str>,
) -> bool {
    match lane_id {
        Some(SYSTEM_LANE_ID) => event.context().lane_id.is_none(),
        Some(id) => event.context().lane_id.as_deref() == Some(id),
        None => false,
    }
}

#[cfg(feature = "server")]
fn transcript_item_from_event(
    event: &mmat_event_stream::event::SemanticEvent,
) -> Option<TranscriptItem> {
    use mmat_event_stream::event::SemanticEvent;

    match event {
        SemanticEvent::HumanFeedbackReceived { answer, .. } => Some(TranscriptItem {
            id: event.event_id().to_string(),
            lane_id: event.context().lane_id.clone(),
            speaker: "You".to_string(),
            content: answer.clone(),
            kind: TranscriptItemKind::Message,
        }),
        SemanticEvent::AssistantMessageProduced { content, .. } => Some(TranscriptItem {
            id: event.event_id().to_string(),
            lane_id: event.context().lane_id.clone(),
            speaker: "Assistant".to_string(),
            content: content.clone(),
            kind: TranscriptItemKind::Message,
        }),
        SemanticEvent::HumanFeedbackRequested {
            source_agent,
            question,
            ..
        } => Some(TranscriptItem {
            id: event.event_id().to_string(),
            lane_id: event.context().lane_id.clone(),
            speaker: source_agent.to_string(),
            content: question.clone(),
            kind: TranscriptItemKind::Message,
        }),
        SemanticEvent::LaneCreated { name, lane_id, .. } => Some(TranscriptItem {
            id: event.event_id().to_string(),
            lane_id: event.context().lane_id.clone(),
            speaker: "System".to_string(),
            content: format!("Forked to lane: {name} ({lane_id})"),
            kind: TranscriptItemKind::Log,
        }),
        _ => Some(TranscriptItem {
            id: event.event_id().to_string(),
            lane_id: event.context().lane_id.clone(),
            speaker: "System".to_string(),
            content: event.variant_name().to_string(),
            kind: TranscriptItemKind::Log,
        }),
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use mmat_event_stream::event::{EventContext, RoleId, SemanticEvent};

    use super::*;

    fn lane_row(id: &str, status: &str) -> mmat_db::models::Lane {
        mmat_db::models::Lane {
            id: uuid::Uuid::parse_str(id).unwrap(),
            project_id: "project-1".to_string(),
            title: id.to_string(),
            summary: String::new(),
            status: status.to_string(),
            creator: "human".to_string(),
            parent_lane_id: None,
            origin_event_id: None,
            origin_message_id: None,
            created_at: "1".to_string(),
            updated_at: "1".to_string(),
            archived_at: None,
        }
    }

    #[test]
    fn lane_projection_groups_active_archived_and_system_lanes() {
        let active_lane_id = "00000000-0000-0000-0000-000000000001";
        let archived_lane_id = "00000000-0000-0000-0000-000000000002";
        let projection = lane_projection_from_rows(
            vec![lane_from_row(lane_row(active_lane_id, "active"))],
            vec![lane_from_row(lane_row(archived_lane_id, "archived"))],
        );

        assert_eq!(projection.active.len(), 1);
        assert_eq!(projection.active[0].id, active_lane_id);
        assert_eq!(projection.archived.len(), 1);
        assert_eq!(projection.archived[0].id, archived_lane_id);
        assert_eq!(projection.system.id, SYSTEM_LANE_ID);
        assert!(projection.system.system);
    }

    #[test]
    fn system_lane_matches_only_unscoped_events() {
        let unscoped = SemanticEvent::new_organisation_started(RoleId::new("coordinator"));
        let scoped = SemanticEvent::new_human_feedback_received(RoleId::new("human"), "hello")
            .with_context(
                EventContext::new("org", "workspace", "project-1", "run-1").with_lane_id("lane-1"),
            );

        assert!(transcript_matches_lane(&unscoped, Some(SYSTEM_LANE_ID)));
        assert!(!transcript_matches_lane(&scoped, Some(SYSTEM_LANE_ID)));
        assert!(transcript_matches_lane(&scoped, Some("lane-1")));
    }

    #[test]
    fn blank_lane_can_exist_without_transcript_items() {
        let blank_lane_id = "00000000-0000-0000-0000-000000000003";
        let lane = lane_from_row(lane_row(blank_lane_id, "active"));
        let projection = lane_projection_from_rows(vec![lane], Vec::new());
        let transcript = Vec::<SemanticEvent>::new()
            .iter()
            .filter(|event| transcript_matches_lane(event, Some(blank_lane_id)))
            .filter_map(transcript_item_from_event)
            .collect::<Vec<_>>();

        assert_eq!(projection.active.len(), 1);
        assert_eq!(projection.active[0].id, blank_lane_id);
        assert!(transcript.is_empty());
    }

    #[test]
    fn assistant_messages_project_into_matching_lane_only() {
        let assistant = SemanticEvent::new_assistant_message_produced(
            RoleId::new("assistant"),
            "assistant-message-1",
            "user-message-1",
            "Assistant reply",
            "stop",
        )
        .with_context(
            EventContext::new("org", "workspace", "project-1", "run-1").with_lane_id("lane-1"),
        );

        assert!(transcript_matches_lane(&assistant, Some("lane-1")));
        assert!(!transcript_matches_lane(&assistant, Some("lane-2")));

        let item = transcript_item_from_event(&assistant).unwrap();
        assert_eq!(item.id, assistant.event_id().to_string());
        assert_eq!(item.lane_id.as_deref(), Some("lane-1"));
        assert_eq!(item.speaker, "Assistant");
        assert_eq!(item.content, "Assistant reply");
        assert_eq!(item.kind, TranscriptItemKind::Message);
    }

    #[test]
    fn local_broadcast_suppression_consumes_only_matching_message() {
        let local_messages = std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashSet::<String>::new(),
        ));
        suppress_local_message(&local_messages, "message-1".to_string());

        let own_message = ChatServerMessage::UserMessageAccepted {
            lane_id: "lane-1".to_string(),
            client_message_id: None,
            message_id: "message-1".to_string(),
            content: "hello".to_string(),
            timestamp_ms: 1,
        };
        let other_message = ChatServerMessage::UserMessageAccepted {
            lane_id: "lane-1".to_string(),
            client_message_id: None,
            message_id: "message-2".to_string(),
            content: "hello".to_string(),
            timestamp_ms: 1,
        };

        assert!(is_suppressed_local_message(&local_messages, &own_message));
        assert!(!is_suppressed_local_message(&local_messages, &own_message));
        assert!(!is_suppressed_local_message(
            &local_messages,
            &other_message
        ));
    }

    #[test]
    fn cancelled_stream_marker_is_consumed_once() {
        let cancelled_streams = std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashSet::<String>::new(),
        ));
        mark_cancelled_stream(&cancelled_streams, "assistant-1".to_string());

        assert!(consume_cancelled_stream(&cancelled_streams, "assistant-1"));
        assert!(!consume_cancelled_stream(&cancelled_streams, "assistant-1"));
        assert!(!consume_cancelled_stream(&cancelled_streams, "assistant-2"));
    }
}
