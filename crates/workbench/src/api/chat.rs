use dioxus::{
    fullstack::{WebSocketOptions, Websocket},
    prelude::*,
};
use serde::{Deserialize, Serialize};

/// Messages accepted from the browser chat client over the workbench WebSocket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatClientMessage {
    /// Submit a user-authored message for backend processing.
    SendMessage {
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
        client_message_id: Option<String>,
        message_id: String,
        content: String,
        timestamp_ms: u64,
    },
    /// Reports that assistant streaming is not yet connected to an LLM runtime.
    AssistantStreamUnavailable {
        message_id: String,
        reply_to_message_id: String,
        reason: String,
    },
    /// Confirms that the backend received a cancellation request.
    Cancelled { assistant_message_id: String },
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

#[cfg(feature = "server")]
async fn handle_chat_socket(
    mut socket: dioxus::fullstack::TypedWebsocket<ChatClientMessage, ChatServerMessage>,
) {
    let session_id = next_id("chat-session");

    if socket
        .send(ChatServerMessage::Connected { session_id })
        .await
        .is_err()
    {
        return;
    }

    loop {
        let message = match socket.recv().await {
            Ok(message) => message,
            Err(_) => return,
        };

        match message {
            ChatClientMessage::SendMessage {
                client_message_id,
                content,
            } => {
                if handle_user_message(&mut socket, client_message_id, content)
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
}

#[cfg(feature = "server")]
async fn handle_user_message(
    socket: &mut dioxus::fullstack::TypedWebsocket<ChatClientMessage, ChatServerMessage>,
    client_message_id: Option<String>,
    content: String,
) -> Result<(), dioxus::fullstack::WebsocketError> {
    let content = content.trim().to_string();

    if content.is_empty() {
        socket
            .send(ChatServerMessage::Error {
                message: "Message content is required.".to_string(),
            })
            .await?;
        return Ok(());
    }

    let message_id = next_id("user-message");
    let assistant_message_id = next_id("assistant-message");

    socket
        .send(ChatServerMessage::UserMessageAccepted {
            client_message_id,
            message_id: message_id.clone(),
            content,
            timestamp_ms: now_ms(),
        })
        .await?;

    socket
        .send(ChatServerMessage::AssistantStreamUnavailable {
            message_id: assistant_message_id,
            reply_to_message_id: message_id,
            reason: "LLM streaming is not connected to the workbench runtime yet.".to_string(),
        })
        .await
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
