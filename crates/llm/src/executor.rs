use serde_json::Value;
use thiserror::Error;

use crate::client::LlmClient;
use crate::message::{CompletionRequest, Message};
use crate::tool::ToolRegistry;

pub type Result<T, E> = std::result::Result<T, ExecutorError<E>>;

#[derive(Clone, Debug)]
pub struct ExecutorConfig {
    pub max_turns: usize,
    pub max_tokens: Option<u32>,
}

#[derive(Error, Debug)]
pub enum ExecutorError<E> {
    #[error("Client error: {0}")]
    Client(String),

    #[error("Turn limit exceeded")]
    TurnLimitExceeded,

    #[error("Token limit exceeded: used {used}, budget {budget}")]
    TokenLimitExceeded { used: u32, budget: u32 },

    #[error("Tool error: {0}")]
    Tool(E),

    #[error("Parse error: {0}")]
    Parse(String),
}

pub struct Executor;

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_turns: 10,
            max_tokens: None,
        }
    }
}

impl Executor {
    pub async fn run<Runtime, Error, Client>(
        client: &Client,
        registry: &ToolRegistry<Runtime, Error>,
        config: &ExecutorConfig,
        runtime: &Runtime,
        mut request: CompletionRequest,
    ) -> Result<Message, Error>
    where
        Client: LlmClient + ?Sized,
        Error: std::fmt::Display,
    {
        let tools = registry.openai_tools();
        if !tools.is_empty() {
            request.tools = Some(tools);
        }

        let mut total_tokens: u32 = 0;

        for turn in 0..config.max_turns {
            let response = client
                .complete(request.clone())
                .await
                .map_err(|e| ExecutorError::Client(e.to_string()))?;

            total_tokens += response.usage.total_tokens;

            if let Some(budget) = config.max_tokens
                && total_tokens > budget
            {
                return Err(ExecutorError::TokenLimitExceeded {
                    used: total_tokens,
                    budget,
                });
            }

            let choice = response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| ExecutorError::Client("No choices in response".into()))?;

            match choice.message {
                Message::Assistant {
                    content: Some(content),
                    tool_calls: None,
                    ..
                } => return Ok(Message::assistant(content)),

                Message::Assistant {
                    tool_calls: Some(ref calls),
                    ..
                } => {
                    request.messages.push(choice.message.clone());
                    for call in calls {
                        let tool = registry.get(&call.function.name).ok_or_else(|| {
                            ExecutorError::Parse(format!("Tool not found: {}", call.function.name))
                        })?;
                        let args: Value = serde_json::from_str(&call.function.arguments)
                            .map_err(|e| ExecutorError::Parse(e.to_string()))?;
                        let result = tool
                            .call(runtime, args)
                            .await
                            .map_err(ExecutorError::Tool)?;
                        request
                            .messages
                            .push(Message::tool(result.to_string(), call.id.clone()));
                    }
                }

                other => return Ok(other),
            }

            if turn == config.max_turns - 1 {
                return Err(ExecutorError::TurnLimitExceeded);
            }
        }

        Err(ExecutorError::TurnLimitExceeded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::LlmClient;
    use crate::error::Result as LlmResult;
    use crate::message::{
        CompletionRequest, CompletionResponse, Message, ToolCall, ToolCallFunction, Usage,
    };
    use crate::tool::{Tool, ToolRegistry, ToolSpec};
    use serde_json::Value;
    use std::result::Result;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use thiserror::Error;

    #[derive(Error, Debug)]
    #[error("tool error: {0}")]
    struct TestError(String);

    struct MockClient {
        responses: std::sync::Mutex<Vec<CompletionResponse>>,
        call_count: AtomicUsize,
    }

    impl MockClient {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for MockClient {
        async fn complete(&self, _request: CompletionRequest) -> LlmResult<CompletionResponse> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            let responses = self.responses.lock().unwrap();
            Ok(responses[idx].clone())
        }
    }

    struct EchoTool;

    #[async_trait::async_trait]
    impl Tool<(), TestError> for EchoTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "echo".into(),
                description: "Echo input".into(),
                input_schema: serde_json::json!({}),
            }
        }
        async fn call(&self, _runtime: &(), arguments: Value) -> Result<Value, TestError> {
            Ok(arguments)
        }
    }

    fn assistant_text(text: &str) -> CompletionResponse {
        CompletionResponse {
            id: "r".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "gpt-4".into(),
            choices: vec![crate::message::Choice {
                index: 0,
                message: Message::assistant(text),
                finish_reason: Some("stop".into()),
            }],
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
        }
    }

    fn assistant_with_tool_call(id: &str, name: &str, args: &str) -> CompletionResponse {
        CompletionResponse {
            id: "r".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "gpt-4".into(),
            choices: vec![crate::message::Choice {
                index: 0,
                message: Message::Assistant {
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: id.into(),
                        call_type: "function".into(),
                        function: ToolCallFunction {
                            name: name.into(),
                            arguments: args.into(),
                        },
                    }]),
                    name: None,
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
        }
    }

    #[tokio::test]
    async fn executor_returns_text_without_tool_calls() {
        let client = MockClient::new(vec![assistant_text("hello")]);
        let registry = ToolRegistry::<(), TestError>::new();
        let config = ExecutorConfig::default();

        let result = Executor::run(
            &client,
            &registry,
            &config,
            &(),
            CompletionRequest::new("gpt-4", vec![Message::user("hi")]),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn executor_handles_single_tool_call() {
        let client = MockClient::new(vec![
            assistant_with_tool_call("call_1", "echo", r#"{"msg":"hi"}"#),
            assistant_text("done"),
        ]);
        let mut registry = ToolRegistry::<(), TestError>::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let config = ExecutorConfig::default();

        let result = Executor::run(
            &client,
            &registry,
            &config,
            &(),
            CompletionRequest::new("gpt-4", vec![Message::user("hi")]),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(client.call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn executor_enforces_turn_limit() {
        let client = MockClient::new(vec![
            assistant_with_tool_call("call_1", "echo", "{}"),
            assistant_with_tool_call("call_2", "echo", "{}"),
            assistant_with_tool_call("call_3", "echo", "{}"),
        ]);
        let mut registry = ToolRegistry::<(), TestError>::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let config = ExecutorConfig {
            max_turns: 2,
            max_tokens: None,
        };

        let result = Executor::run(
            &client,
            &registry,
            &config,
            &(),
            CompletionRequest::new("gpt-4", vec![Message::user("hi")]),
        )
        .await;

        assert!(matches!(result, Err(ExecutorError::TurnLimitExceeded)));
    }

    #[tokio::test]
    async fn executor_enforces_token_budget() {
        let mut resp = assistant_text("hello");
        resp.usage = Usage {
            prompt_tokens: 100,
            completion_tokens: 100,
            total_tokens: 200,
        };
        let client = MockClient::new(vec![resp]);
        let registry = ToolRegistry::<(), TestError>::new();
        let config = ExecutorConfig {
            max_turns: 10,
            max_tokens: Some(150),
        };

        let result = Executor::run(
            &client,
            &registry,
            &config,
            &(),
            CompletionRequest::new("gpt-4", vec![Message::user("hi")]),
        )
        .await;

        assert!(matches!(
            result,
            Err(ExecutorError::TokenLimitExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn executor_accumulates_token_usage() {
        let mut resp1 = assistant_with_tool_call("call_1", "echo", "{}");
        resp1.usage = Usage {
            prompt_tokens: 50,
            completion_tokens: 50,
            total_tokens: 100,
        };
        let mut resp2 = assistant_text("done");
        resp2.usage = Usage {
            prompt_tokens: 60,
            completion_tokens: 40,
            total_tokens: 100,
        };
        let client = MockClient::new(vec![resp1, resp2]);
        let mut registry = ToolRegistry::<(), TestError>::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let config = ExecutorConfig {
            max_turns: 10,
            max_tokens: Some(150),
        };

        let result = Executor::run(
            &client,
            &registry,
            &config,
            &(),
            CompletionRequest::new("gpt-4", vec![Message::user("hi")]),
        )
        .await;

        assert!(matches!(
            result,
            Err(ExecutorError::TokenLimitExceeded {
                used: 200,
                budget: 150
            })
        ));
    }
}
