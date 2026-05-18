use dioxus::{
    fullstack::{WebSocketOptions, Websocket},
    prelude::*,
};
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
use std::sync::OnceLock;

#[cfg(feature = "server")]
static WORKBENCH_BUS: OnceLock<mmat_event_stream::event_bus::EventBus> = OnceLock::new();

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
    /// Reports that assistant streaming is not yet connected to an LLM runtime.
    AssistantStreamUnavailable {
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
    let active = mmat_db::load_lanes_by_status(&mut connection, &project_id, "active")
        .await
        .map_err(|error| ServerFnError::new(format!("could not load active lanes: {error}")))?
        .into_iter()
        .map(lane_from_row)
        .collect();
    let archived = mmat_db::load_lanes_by_status(&mut connection, &project_id, "archived")
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
    let lane_id = mmat_db::new_lane_id();
    let lane = mmat_db::models::NewLane {
        id: lane_id.clone(),
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
    let event = lane_created_event(&lane_id, &title, &project_id);
    let lane = mmat_db::create_lane_with_event(&mut connection, lane, event.clone())
        .await
        .map_err(|error| ServerFnError::new(format!("could not create lane: {error}")))?;
    let _ = workbench_bus().publish(event);

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
    let lane = mmat_db::archive_lane_with_event(
        &mut connection,
        &lane.id,
        mmat_db::now_timestamp_string(),
        event.clone(),
    )
    .await
    .map_err(|error| ServerFnError::new(format!("could not archive lane: {error}")))?;
    let _ = workbench_bus().publish(event);
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
    let events = mmat_db::replay_events(&mut connection, 0, None)
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
    let session_id = next_id("chat-session");
    let mut workbench_events = workbench_bus().subscribe(&[]);

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
                        if handle_user_message(&mut socket, project_id, lane_id, client_message_id, content)
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    ChatClientMessage::Cancel {
                        assistant_message_id,
                    } => {
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

                if let Some(message) = chat_server_message_from_event(event.as_ref())
                    && socket.send(message).await.is_err()
                {
                    return;
                }
            }
        }
    }
}

#[cfg(feature = "server")]
async fn handle_user_message(
    socket: &mut dioxus::fullstack::TypedWebsocket<ChatClientMessage, ChatServerMessage>,
    project_id: String,
    lane_id: Option<String>,
    client_message_id: Option<String>,
    content: String,
) -> Result<(), dioxus::fullstack::WebsocketError> {
    use mmat_event_stream::event::{EventContext, RoleId, SemanticEvent};

    let content = content.trim().to_string();
    if content.is_empty() {
        socket
            .send(ChatServerMessage::Error {
                message: "Message content is required.".to_string(),
            })
            .await?;
        return Ok(());
    }

    let Some(lane_id) = lane_id.filter(|id| id != SYSTEM_LANE_ID) else {
        socket
            .send(ChatServerMessage::Error {
                message: "Select or create a lane before sending a message.".to_string(),
            })
            .await?;
        return Ok(());
    };

    let pool = match super::db().await {
        Ok(pool) => pool,
        Err(error) => {
            socket
                .send(ChatServerMessage::Error {
                    message: format!("Could not open database pool: {error}"),
                })
                .await?;
            return Ok(());
        }
    };
    let mut connection = match pool.get().await {
        Ok(connection) => connection,
        Err(error) => {
            socket
                .send(ChatServerMessage::Error {
                    message: format!("Could not open database connection: {error}"),
                })
                .await?;
            return Ok(());
        }
    };

    if let Err(error) = validate_lane(&mut connection, &project_id, &lane_id, true).await {
        socket
            .send(ChatServerMessage::Error {
                message: error.to_string(),
            })
            .await?;
        return Ok(());
    }

    let event = SemanticEvent::new_human_feedback_received(RoleId::new("human"), &content)
        .with_context(
            EventContext::new(
                "default-organisation",
                "default-workspace",
                project_id,
                "default-run",
            )
            .with_lane_id(lane_id.clone()),
        );
    let message_id = event.event_id().to_string();
    if let Err(error) = mmat_db::append_event(&mut connection, &event).await {
        socket
            .send(ChatServerMessage::Error {
                message: format!("Could not persist message: {error}"),
            })
            .await?;
        return Ok(());
    }
    let _ = workbench_bus().publish(event);

    socket
        .send(ChatServerMessage::UserMessageAccepted {
            lane_id: lane_id.clone(),
            client_message_id,
            message_id: message_id.clone(),
            content,
            timestamp_ms: now_ms(),
        })
        .await?;

    socket
        .send(ChatServerMessage::AssistantStreamUnavailable {
            lane_id,
            message_id: next_id("assistant-message"),
            reply_to_message_id: message_id,
            reason: "LLM streaming is not connected to the workbench runtime yet.".to_string(),
        })
        .await
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
fn workbench_bus() -> &'static mmat_event_stream::event_bus::EventBus {
    WORKBENCH_BUS.get_or_init(|| mmat_event_stream::event_bus::EventBus::new(1024))
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
    let lane = mmat_db::get_lane(connection, lane_id)
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
    let exists = mmat_db::project_exists(connection, project_id)
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
        id: lane.id,
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
            id: id.to_string(),
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
        let projection = lane_projection_from_rows(
            vec![lane_from_row(lane_row("active-lane", "active"))],
            vec![lane_from_row(lane_row("archived-lane", "archived"))],
        );

        assert_eq!(projection.active.len(), 1);
        assert_eq!(projection.active[0].id, "active-lane");
        assert_eq!(projection.archived.len(), 1);
        assert_eq!(projection.archived[0].id, "archived-lane");
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
        let lane = lane_from_row(lane_row("blank-lane", "active"));
        let projection = lane_projection_from_rows(vec![lane], Vec::new());
        let transcript = Vec::<SemanticEvent>::new()
            .iter()
            .filter(|event| transcript_matches_lane(event, Some("blank-lane")))
            .filter_map(transcript_item_from_event)
            .collect::<Vec<_>>();

        assert_eq!(projection.active.len(), 1);
        assert_eq!(projection.active[0].id, "blank-lane");
        assert!(transcript.is_empty());
    }
}
