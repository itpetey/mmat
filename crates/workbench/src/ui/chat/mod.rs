use dioxus::{fullstack::WebSocketOptions, prelude::*};

use crate::api::chat::{ChatClientMessage, ChatServerMessage, connect_chat};

#[css_module("/src/ui/chat/style.css")]
struct ChatStyles;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatSpeaker {
    Me,
    System,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChatTranscriptItem {
    id: String,
    speaker: ChatSpeaker,
    content: String,
    kind: ChatMessageKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatMessageKind {
    Message,
    Log,
    Error,
}

#[component]
pub(crate) fn ChatWorkbench() -> Element {
    let mut messages = use_signal(Vec::<ChatTranscriptItem>::new);
    let draft = use_signal(String::new);
    let next_client_message_id = use_signal(|| 0u64);
    let mut websocket = dioxus::fullstack::use_websocket(|| connect_chat(WebSocketOptions::new()));

    use_future(move || async move {
        loop {
            match websocket.recv().await {
                Ok(message) => push_server_message(messages, message),
                Err(error) => {
                    messages.write().push(ChatTranscriptItem {
                        id: "chat-connection-error".to_string(),
                        speaker: ChatSpeaker::System,
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
            {chat_composer(draft, next_client_message_id, messages, websocket)}
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
                    "Start a conversation. Messages will stream here as the workbench backend responds."
                }
            }
            for message in messages {
                ChatRow { key: "{message.id}", message }
            }
        }
    }
}

#[component]
fn ChatRow(message: ChatTranscriptItem) -> Element {
    let speaker = match message.speaker {
        ChatSpeaker::Me => "ME",
        ChatSpeaker::System => "SYSTEM",
    };
    let speaker_class = match message.speaker {
        ChatSpeaker::System => ChatStyles::dx_chat_speaker_system.to_string(),
        ChatSpeaker::Me => ChatStyles::dx_chat_speaker_me.to_string(),
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

fn chat_composer(
    mut draft: Signal<String>,
    next_client_message_id: Signal<u64>,
    messages: Signal<Vec<ChatTranscriptItem>>,
    websocket: dioxus::fullstack::UseWebsocket<ChatClientMessage, ChatServerMessage>,
) -> Element {
    rsx! {
        form {
            class: ChatStyles::dx_chat_composer,
            onsubmit: move |event| {
                event.prevent_default();
                submit_chat_message(draft, next_client_message_id, messages, websocket);
            },
            textarea {
                aria_label: "Compose a message",
                placeholder: "Type your message here...",
                value: "{draft}",
                oninput: move |event| draft.set(event.value()),
                onkeydown: move |event| {
                    if event.key() == Key::Enter && event.modifiers().meta() {
                        event.prevent_default();
                        submit_chat_message(draft, next_client_message_id, messages, websocket);
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

fn submit_chat_message(
    mut draft: Signal<String>,
    mut next_client_message_id: Signal<u64>,
    mut messages: Signal<Vec<ChatTranscriptItem>>,
    websocket: dioxus::fullstack::UseWebsocket<ChatClientMessage, ChatServerMessage>,
) {
    let content = draft().trim().to_string();
    if content.is_empty() {
        return;
    }

    let id = next_client_message_id() + 1;
    next_client_message_id.set(id);
    draft.set(String::new());

    spawn(async move {
        let result = websocket
            .send(ChatClientMessage::SendMessage {
                client_message_id: Some(format!("client-message-{id}")),
                content,
            })
            .await;

        if let Err(error) = result {
            messages.write().push(ChatTranscriptItem {
                id: format!("send-error-{id}"),
                speaker: ChatSpeaker::System,
                content: format!("Could not send message: {error}"),
                kind: ChatMessageKind::Error,
            });
        }
    });
}

fn push_server_message(mut messages: Signal<Vec<ChatTranscriptItem>>, message: ChatServerMessage) {
    let transcript_item = match message {
        ChatServerMessage::Connected { session_id } => ChatTranscriptItem {
            id: session_id.clone(),
            speaker: ChatSpeaker::System,
            content: format!("Connected to {session_id}."),
            kind: ChatMessageKind::Log,
        },
        ChatServerMessage::UserMessageAccepted {
            message_id,
            content,
            ..
        } => ChatTranscriptItem {
            id: message_id,
            speaker: ChatSpeaker::Me,
            content,
            kind: ChatMessageKind::Message,
        },
        ChatServerMessage::AssistantStreamUnavailable {
            message_id, reason, ..
        } => ChatTranscriptItem {
            id: message_id,
            speaker: ChatSpeaker::System,
            content: reason,
            kind: ChatMessageKind::Log,
        },
        ChatServerMessage::Cancelled {
            assistant_message_id,
        } => ChatTranscriptItem {
            id: format!("cancelled-{assistant_message_id}"),
            speaker: ChatSpeaker::System,
            content: format!("Cancelled {assistant_message_id}."),
            kind: ChatMessageKind::Log,
        },
        ChatServerMessage::Pong { .. } => return,
        ChatServerMessage::Error { message } => ChatTranscriptItem {
            id: format!("chat-error-{}", messages.read().len()),
            speaker: ChatSpeaker::System,
            content: message,
            kind: ChatMessageKind::Error,
        },
    };

    messages.write().push(transcript_item);
}
