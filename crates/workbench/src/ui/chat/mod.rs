use dioxus::{fullstack::WebSocketOptions, prelude::*};

use crate::api::chat::{
    ChatClientMessage, ChatServerMessage, SYSTEM_LANE_ID, TranscriptItem, TranscriptItemKind,
    connect_chat, load_transcript,
};

#[css_module("/src/ui/chat/style.css")]
struct ChatStyles;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatMessageKind {
    Message,
    Log,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChatTranscriptItem {
    id: String,
    speaker: String,
    content: String,
    kind: ChatMessageKind,
}

impl From<TranscriptItem> for ChatTranscriptItem {
    fn from(item: TranscriptItem) -> Self {
        Self {
            id: item.id,
            speaker: item.speaker,
            content: item.content,
            kind: match item.kind {
                TranscriptItemKind::Message => ChatMessageKind::Message,
                TranscriptItemKind::Log => ChatMessageKind::Log,
                TranscriptItemKind::Error => ChatMessageKind::Error,
            },
        }
    }
}

#[component]
pub(crate) fn ChatWorkbench(
    selected_project_id: Signal<Option<String>>,
    selected_lane_id: Signal<Option<String>>,
    selected_lane_status: Signal<Option<String>>,
    lanes_revision: Signal<u64>,
) -> Element {
    let mut messages = use_signal(Vec::<ChatTranscriptItem>::new);
    let draft = use_signal(String::new);
    let next_client_message_id = use_signal(|| 0u64);
    let mut websocket = dioxus::fullstack::use_websocket(|| connect_chat(WebSocketOptions::new()));

    use_effect(move || {
        let lane_id = selected_lane_id();
        let project_id = selected_project_id();
        if let Some(project_id) = project_id {
            let requested_project_id = project_id.clone();
            let requested_lane_id = lane_id.clone();
            spawn(async move {
                match load_transcript(project_id, lane_id).await {
                    Ok(items)
                        if selected_project_id().as_deref()
                            == Some(requested_project_id.as_str())
                            && selected_lane_id() == requested_lane_id =>
                    {
                        messages.set(items.into_iter().map(ChatTranscriptItem::from).collect())
                    }
                    Err(error)
                        if selected_project_id().as_deref()
                            == Some(requested_project_id.as_str())
                            && selected_lane_id() == requested_lane_id =>
                    {
                        messages.set(vec![ChatTranscriptItem {
                            id: "transcript-load-error".to_string(),
                            speaker: "System".to_string(),
                            content: format!("Could not load transcript: {error}"),
                            kind: ChatMessageKind::Error,
                        }])
                    }
                    _ => {}
                }
            });
        } else {
            messages.set(Vec::new());
        }
    });

    use_future(move || async move {
        loop {
            match websocket.recv().await {
                Ok(message) => push_server_message(
                    messages,
                    selected_project_id(),
                    selected_lane_id(),
                    lanes_revision,
                    message,
                ),
                Err(error) => {
                    messages.write().push(ChatTranscriptItem {
                        id: "chat-connection-error".to_string(),
                        speaker: "System".to_string(),
                        content: format!("Chat connection closed: {error}"),
                        kind: ChatMessageKind::Error,
                    });
                    break;
                }
            }
        }
    });

    rsx! {
        section { class: ChatStyles::dx_chat_shell, aria_label: "Workbench chat",
            ConversationContainer { messages: messages.read().clone() }
            {chat_composer(selected_project_id, selected_lane_id, selected_lane_status, draft, next_client_message_id, messages, websocket)}
        }
    }
}

#[component]
fn ChatRow(message: ChatTranscriptItem) -> Element {
    let speaker = message.speaker.to_uppercase();
    let speaker_class = match message.speaker.as_str() {
        "You" => ChatStyles::dx_chat_speaker_me.to_string(),
        _ => ChatStyles::dx_chat_speaker_system.to_string(),
    };
    let body_class = match message.kind {
        ChatMessageKind::Message => ChatStyles::dx_chat_message.to_string(),
        ChatMessageKind::Log => format!(
            "{} {}",
            ChatStyles::dx_chat_message,
            ChatStyles::dx_chat_log
        ),
        ChatMessageKind::Error => format!(
            "{} {}",
            ChatStyles::dx_chat_message,
            ChatStyles::dx_chat_error
        ),
    };

    rsx! {
        article { class: ChatStyles::dx_chat_row,
            div { class: format!("{} {}", ChatStyles::dx_chat_gutter_name, speaker_class), "{speaker} >" }
            div { class: body_class,
                p { "{message.content}" }
            }
        }
    }
}

#[component]
fn ConversationContainer(messages: Vec<ChatTranscriptItem>) -> Element {
    rsx! {
        div {
            class: ChatStyles::dx_chat_conversation,
            role: "log",
            aria_label: "Conversation",
            aria_live: "polite",

            if messages.is_empty() {
                div { class: ChatStyles::dx_chat_empty,
                    "This lane is blank. Start a conversation or switch lanes from the sidebar."
                }
            }
            for message in messages {
                ChatRow { key: "{message.id}", message }
            }
        }
    }
}

fn append_assistant_delta(
    mut messages: Signal<Vec<ChatTranscriptItem>>,
    message_id: &str,
    delta: &str,
) {
    let mut messages = messages.write();
    append_assistant_delta_to_items(&mut messages, message_id, delta);
}

fn append_assistant_delta_to_items(
    messages: &mut Vec<ChatTranscriptItem>,
    message_id: &str,
    delta: &str,
) {
    if let Some(item) = messages.iter_mut().find(|item| item.id == message_id) {
        item.content.push_str(delta);
    } else {
        messages.push(ChatTranscriptItem {
            id: message_id.to_string(),
            speaker: "Assistant".to_string(),
            content: delta.to_string(),
            kind: ChatMessageKind::Message,
        });
    }
}

fn chat_composer(
    selected_project_id: Signal<Option<String>>,
    selected_lane_id: Signal<Option<String>>,
    selected_lane_status: Signal<Option<String>>,
    mut draft: Signal<String>,
    next_client_message_id: Signal<u64>,
    messages: Signal<Vec<ChatTranscriptItem>>,
    websocket: dioxus::fullstack::UseWebsocket<ChatClientMessage, ChatServerMessage>,
) -> Element {
    let accepts_input = selected_lane_status().as_deref() == Some("active");
    let placeholder = if accepts_input {
        "Type your message here..."
    } else if selected_lane_id().as_deref() == Some(SYSTEM_LANE_ID) {
        "System lane is read-only..."
    } else if selected_lane_status().as_deref() == Some("archived") {
        "Archived lanes are read-only..."
    } else {
        "Create or select a lane first..."
    };
    let disabled = !accepts_input;

    rsx! {
        form {
            class: ChatStyles::dx_chat_composer,
            onsubmit: move |event| {
                event.prevent_default();
                submit_chat_message(selected_project_id(), selected_lane_id(), selected_lane_status(), draft, next_client_message_id, messages, websocket);
            },
            textarea {
                aria_label: "Compose a message",
                placeholder,
                value: "{draft}",
                autofocus: true,
                disabled,
                oninput: move |event| draft.set(event.value()),
                onkeydown: move |event| {
                    if event.key() == Key::Enter && event.modifiers().meta() {
                        event.prevent_default();
                        submit_chat_message(selected_project_id(), selected_lane_id(), selected_lane_status(), draft, next_client_message_id, messages, websocket);
                    }
                },
            }
            div { class: ChatStyles::dx_chat_submit_hint,
                strong { "Press" }
                " "
                code { "⌘ + Return" }
                " "
                strong { "to submit" }
            }
        }
    }
}

fn push_server_message(
    mut messages: Signal<Vec<ChatTranscriptItem>>,
    selected_project_id: Option<String>,
    selected_lane_id: Option<String>,
    mut lanes_revision: Signal<u64>,
    message: ChatServerMessage,
) {
    let transcript_item = match message {
        ChatServerMessage::Connected { session_id } => ChatTranscriptItem {
            id: session_id.clone(),
            speaker: "System".to_string(),
            content: format!("Connected to {session_id}."),
            kind: ChatMessageKind::Log,
        },
        ChatServerMessage::UserMessageAccepted {
            lane_id,
            message_id,
            content,
            ..
        } => {
            if selected_lane_id.as_deref() != Some(lane_id.as_str()) {
                return;
            }
            ChatTranscriptItem {
                id: message_id,
                speaker: "You".to_string(),
                content,
                kind: ChatMessageKind::Message,
            }
        }
        ChatServerMessage::AssistantStreamStarted {
            lane_id,
            message_id,
            ..
        } => {
            if selected_lane_id.as_deref() != Some(lane_id.as_str()) {
                return;
            }
            ChatTranscriptItem {
                id: message_id,
                speaker: "Assistant".to_string(),
                content: String::new(),
                kind: ChatMessageKind::Message,
            }
        }
        ChatServerMessage::AssistantStreamDelta {
            lane_id,
            message_id,
            delta,
        } => {
            if selected_lane_id.as_deref() != Some(lane_id.as_str()) {
                return;
            }
            append_assistant_delta(messages, &message_id, &delta);
            return;
        }
        ChatServerMessage::AssistantStreamCompleted {
            lane_id,
            message_id,
            content,
            ..
        } => {
            if selected_lane_id.as_deref() != Some(lane_id.as_str()) {
                return;
            }
            upsert_assistant_message(messages, &message_id, content);
            return;
        }
        ChatServerMessage::AssistantStreamFailed {
            lane_id,
            message_id,
            reason,
            ..
        } => {
            if selected_lane_id.as_deref() != Some(lane_id.as_str()) {
                return;
            }
            ChatTranscriptItem {
                id: format!("assistant-failed-{message_id}"),
                speaker: "System".to_string(),
                content: reason,
                kind: ChatMessageKind::Error,
            }
        }
        ChatServerMessage::Cancelled {
            assistant_message_id,
        } => ChatTranscriptItem {
            id: format!("cancelled-{assistant_message_id}"),
            speaker: "System".to_string(),
            content: format!("Cancelled {assistant_message_id}."),
            kind: ChatMessageKind::Log,
        },
        ChatServerMessage::Pong { .. } => return,
        ChatServerMessage::ProjectionChanged { project_id } => {
            if selected_project_id.as_deref() == Some(project_id.as_str()) {
                lanes_revision.set(lanes_revision() + 1);
            }
            return;
        }
        ChatServerMessage::Error { message } => ChatTranscriptItem {
            id: format!("chat-error-{}", messages.read().len()),
            speaker: "System".to_string(),
            content: message,
            kind: ChatMessageKind::Error,
        },
    };

    let duplicate = messages
        .read()
        .iter()
        .any(|item| item.id == transcript_item.id);
    if !duplicate {
        messages.write().push(transcript_item);
    }
}

fn submit_chat_message(
    selected_project_id: Option<String>,
    selected_lane_id: Option<String>,
    selected_lane_status: Option<String>,
    mut draft: Signal<String>,
    mut next_client_message_id: Signal<u64>,
    mut messages: Signal<Vec<ChatTranscriptItem>>,
    websocket: dioxus::fullstack::UseWebsocket<ChatClientMessage, ChatServerMessage>,
) {
    let content = draft().trim().to_string();
    if content.is_empty() {
        return;
    }

    let Some(lane_id) = selected_lane_id else {
        messages.write().push(ChatTranscriptItem {
            id: "missing-lane-error".to_string(),
            speaker: "System".to_string(),
            content: "Create or select a lane before sending a message.".to_string(),
            kind: ChatMessageKind::Error,
        });
        return;
    };

    if selected_lane_status.as_deref() != Some("active") || lane_id == SYSTEM_LANE_ID {
        messages.write().push(ChatTranscriptItem {
            id: "read-only-lane-error".to_string(),
            speaker: "System".to_string(),
            content: "Select an active lane before sending a message.".to_string(),
            kind: ChatMessageKind::Error,
        });
        return;
    }

    let Some(project_id) = selected_project_id else {
        messages.write().push(ChatTranscriptItem {
            id: "missing-project-error".to_string(),
            speaker: "System".to_string(),
            content: "Select a project before sending a message.".to_string(),
            kind: ChatMessageKind::Error,
        });
        return;
    };

    let id = next_client_message_id() + 1;
    next_client_message_id.set(id);
    draft.set(String::new());

    spawn(async move {
        let result = websocket
            .send(ChatClientMessage::SendMessage {
                project_id,
                lane_id: Some(lane_id),
                client_message_id: Some(format!("client-message-{id}")),
                content,
            })
            .await;

        if let Err(error) = result {
            messages.write().push(ChatTranscriptItem {
                id: format!("send-error-{id}"),
                speaker: "System".to_string(),
                content: format!("Could not send message: {error}"),
                kind: ChatMessageKind::Error,
            });
        }
    });
}

fn upsert_assistant_message(
    mut messages: Signal<Vec<ChatTranscriptItem>>,
    message_id: &str,
    content: String,
) {
    let mut messages = messages.write();
    upsert_assistant_message_in_items(&mut messages, message_id, content);
}

fn upsert_assistant_message_in_items(
    messages: &mut Vec<ChatTranscriptItem>,
    message_id: &str,
    content: String,
) {
    if let Some(item) = messages.iter_mut().find(|item| item.id == message_id) {
        item.content = content;
    } else {
        messages.push(ChatTranscriptItem {
            id: message_id.to_string(),
            speaker: "Assistant".to_string(),
            content,
            kind: ChatMessageKind::Message,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_deltas_merge_into_one_row() {
        let mut messages = Vec::new();
        append_assistant_delta_to_items(&mut messages, "assistant-1", "Hel");
        append_assistant_delta_to_items(&mut messages, "assistant-1", "lo");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, "assistant-1");
        assert_eq!(messages[0].speaker, "Assistant");
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[0].kind, ChatMessageKind::Message);
    }

    #[test]
    fn assistant_completion_updates_existing_row_without_duplicate() {
        let mut messages = vec![ChatTranscriptItem {
            id: "assistant-1".to_string(),
            speaker: "Assistant".to_string(),
            content: "partial".to_string(),
            kind: ChatMessageKind::Message,
        }];

        upsert_assistant_message_in_items(&mut messages, "assistant-1", "complete".to_string());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "complete");
    }
}
