//! Request and response types for OpenAI-compatible chat completions.

use serde::{Deserialize, Serialize};

/// Function call details within a [`ToolCall`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCallFunction {
    /// The name of the function to call.
    pub name: String,
    /// The JSON-encoded function arguments.
    pub arguments: String,
}

/// A tool call requested by the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// The type of call (always `"function"`).
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function call details.
    pub function: ToolCallFunction,
}

/// A chat message in one of the four standard roles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    /// A system message that provides instructions to the model.
    System {
        /// The message content.
        content: String,
        /// Optional participant name.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// A message from the end user.
    User {
        /// The message content.
        content: String,
        /// Optional participant name.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// A response from the model.
    Assistant {
        /// The text content of the response, if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        /// Tool calls requested by the model, if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
        /// Optional participant name.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// The result of a tool invocation.
    Tool {
        /// The tool output.
        content: String,
        /// The ID of the tool call this result corresponds to.
        tool_call_id: String,
    },
}

/// Options for streaming completion requests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StreamOptions {
    /// Whether to include token usage in stream chunks.
    pub include_usage: bool,
}

/// A request for a chat completion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// The model to use (e.g. `"gpt-4"`).
    pub model: String,
    /// The conversation messages.
    pub messages: Vec<Message>,
    /// Tool definitions to make available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    /// Controls which tool (if any) the model calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    /// Sampling temperature between 0 and 2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Maximum number of tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether to stream the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Additional streaming options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
}

/// A single completion choice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    /// The index of this choice.
    pub index: u32,
    /// The message produced by the model.
    pub message: Message,
    /// The reason the model stopped generating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Token usage statistics for a completion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Number of tokens in the prompt.
    pub prompt_tokens: u32,
    /// Number of tokens in the generated completion.
    pub completion_tokens: u32,
    /// Total tokens consumed.
    pub total_tokens: u32,
}

/// A full (non-streaming) completion response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompletionResponse {
    /// Unique identifier for this completion.
    pub id: String,
    /// The object type (always `"chat.completion"`).
    pub object: String,
    /// Unix timestamp of when the completion was created.
    pub created: u64,
    /// The model used for the completion.
    pub model: String,
    /// The list of completion choices.
    pub choices: Vec<Choice>,
    /// Token usage statistics.
    pub usage: Usage,
}

/// Function call details within a streaming tool call delta.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StreamToolCallFunction {
    /// The name of the function, provided in the first chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The partial JSON-encoded function arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// A tool call delta in a streaming response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StreamToolCall {
    /// The index of the tool call within the choice.
    pub index: u32,
    /// Unique identifier for this tool call, provided in the first chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The type of call (always `"function"`).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    /// The function call details, if present in this chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<StreamToolCallFunction>,
}

/// A delta update to a choice in a streaming response.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChoiceDelta {
    /// The role of the message author (e.g. `"assistant"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// A fragment of the message content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool call fragments, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<StreamToolCall>>,
}

/// A streaming choice containing a delta.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StreamChoice {
    /// The index of this choice.
    pub index: u32,
    /// The delta content for this choice.
    pub delta: ChoiceDelta,
    /// The reason the model stopped, provided in the final chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Token usage statistics included in a stream chunk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StreamUsage {
    /// Number of tokens in the prompt.
    pub prompt_tokens: u32,
    /// Number of tokens in the generated completion.
    pub completion_tokens: u32,
    /// Total tokens consumed.
    pub total_tokens: u32,
}

/// A single chunk of a streaming completion response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompletionStreamChunk {
    /// Unique identifier for this completion.
    pub id: String,
    /// The object type (always `"chat.completion.chunk"`).
    pub object: String,
    /// Unix timestamp of when the chunk was created.
    pub created: u64,
    /// The model used for the completion.
    pub model: String,
    /// The list of choice deltas.
    pub choices: Vec<StreamChoice>,
    /// Token usage statistics, included when `stream_options.include_usage` is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<StreamUsage>,
}

impl Message {
    /// Create a [`Message::System`] with the given content.
    pub fn system(content: impl Into<String>) -> Self {
        Self::System {
            content: content.into(),
            name: None,
        }
    }

    /// Create a [`Message::User`] with the given content.
    pub fn user(content: impl Into<String>) -> Self {
        Self::User {
            content: content.into(),
            name: None,
        }
    }

    /// Create a [`Message::Assistant`] with the given content.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::Assistant {
            content: Some(content.into()),
            tool_calls: None,
            name: None,
        }
    }

    /// Create a [`Message::Tool`] with the given content and tool call ID.
    pub fn tool(content: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self::Tool {
            content: content.into(),
            tool_call_id: tool_call_id.into(),
        }
    }
}

impl CompletionRequest {
    /// Create a new completion request with the given model and messages.
    ///
    /// All optional fields are initialised to [`None`].
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            tools: None,
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            stream: None,
            stream_options: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_serialises() {
        let msg = Message::user("Hello");
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"role":"user","content":"Hello"}"#);
    }

    #[test]
    fn assistant_message_with_tool_calls_serialises() {
        let msg = Message::Assistant {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: ToolCallFunction {
                    name: "do_thing".into(),
                    arguments: r#"{"x":1}"#.into(),
                },
            }]),
            name: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let expected = r#"{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"do_thing","arguments":"{\"x\":1}"}}]}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn message_round_trip() {
        let msgs = vec![
            Message::system("You are a helper"),
            Message::user("Hello"),
            Message::assistant("Hi there"),
            Message::tool("result", "call_1"),
        ];
        for msg in msgs {
            let json = serde_json::to_string(&msg).unwrap();
            let back: Message = serde_json::from_str(&json).unwrap();
            assert_eq!(msg, back);
        }
    }

    #[test]
    fn completion_request_round_trip() {
        let req = CompletionRequest::new("gpt-4", vec![Message::user("Hello")]);
        let json = serde_json::to_string(&req).unwrap();
        let back: CompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn completion_response_round_trip() {
        let resp = CompletionResponse {
            id: "resp_1".into(),
            object: "chat.completion".into(),
            created: 1234567890,
            model: "gpt-4".into(),
            choices: vec![Choice {
                index: 0,
                message: Message::assistant("Hello"),
                finish_reason: Some("stop".into()),
            }],
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: CompletionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn stream_chunk_round_trip() {
        let chunk = CompletionStreamChunk {
            id: "chunk_1".into(),
            object: "chat.completion.chunk".into(),
            created: 1234567890,
            model: "gpt-4".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: ChoiceDelta {
                    role: Some("assistant".into()),
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: CompletionStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, back);
    }

    #[test]
    fn stream_chunk_with_tool_call_delta_round_trip() {
        let chunk = CompletionStreamChunk {
            id: "chunk_1".into(),
            object: "chat.completion.chunk".into(),
            created: 1234567890,
            model: "gpt-4".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: ChoiceDelta {
                    role: Some("assistant".into()),
                    content: None,
                    tool_calls: Some(vec![StreamToolCall {
                        index: 0,
                        id: Some("call_1".into()),
                        call_type: Some("function".into()),
                        function: Some(StreamToolCallFunction {
                            name: Some("do_thing".into()),
                            arguments: Some(r#"{"x":1"#.into()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: CompletionStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, back);
    }
}
