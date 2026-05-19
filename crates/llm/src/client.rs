//! LLM client trait and OpenAI-compatible implementation.

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::Client;
use tokio::sync::mpsc;

use crate::{
    error::{LlmError, Result},
    message::{CompletionRequest, CompletionResponse, CompletionStreamChunk},
};

/// Default base URL for OpenAI-compatible chat completions.
pub const DEFAULT_CHAT_BASE_URL: &str = "https://api.openai.com/v1";

/// A client capable of sending chat completions to an LLM provider.
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a completion request and return the full response.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
}

/// Configuration for connecting to an OpenAI-compatible API.
#[derive(Clone, Debug)]
pub struct OpenAiConfig {
    /// The API key used for authentication.
    pub api_key: String,
    /// The base URL of the API endpoint.
    pub base_url: String,
    /// The HTTP request timeout.
    pub timeout: Duration,
}

/// Builder for constructing an [`OpenAiConfig`].
#[derive(Default)]
pub struct OpenAiConfigBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    timeout: Option<Duration>,
}

/// An OpenAI-compatible client using reqwest for HTTP transport.
#[derive(Clone, Debug)]
pub struct OpenAiClient {
    config: OpenAiConfig,
    http: Client,
}

impl OpenAiConfig {
    /// Create a new builder for constructing an [`OpenAiConfig`].
    pub fn builder() -> OpenAiConfigBuilder {
        OpenAiConfigBuilder::default()
    }
}

impl OpenAiConfigBuilder {
    /// Set the API key used for authentication.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Set the base URL of the API endpoint.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set the HTTP request timeout.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Consume the builder and produce an [`OpenAiConfig`].
    ///
    /// Defaults to [`DEFAULT_CHAT_BASE_URL`] for the base URL and 60 seconds
    /// for the timeout if not set.
    pub fn build(self) -> OpenAiConfig {
        OpenAiConfig {
            api_key: self.api_key.unwrap_or_default(),
            base_url: self
                .base_url
                .unwrap_or_else(|| DEFAULT_CHAT_BASE_URL.into()),
            timeout: self.timeout.unwrap_or_else(|| Duration::from_secs(60)),
        }
    }
}

impl OpenAiClient {
    /// Create a new [`OpenAiClient`] from the given configuration.
    pub fn new(config: OpenAiConfig) -> Result<Self> {
        let http = Client::builder().timeout(config.timeout).build()?;
        Ok(Self { config, http })
    }

    /// Send a completion request and return a streaming receiver of response chunks.
    ///
    /// The returned [`mpsc::Receiver`] yields [`Result`]`<`[`CompletionStreamChunk`]`>` items
    /// as they arrive from the API.
    pub async fn complete_streaming(
        &self,
        request: CompletionRequest,
    ) -> Result<mpsc::Receiver<Result<CompletionStreamChunk>>> {
        let mut req = request;
        req.stream = Some(true);
        req.stream_options = Some(crate::message::StreamOptions {
            include_usage: true,
        });

        let body = serde_json::to_value(&req)?;
        let response = self
            .http
            .post(format!("{}/chat/completions", self.config.base_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status,
                message: text,
            });
        }

        let (tx, rx) = mpsc::channel(128);
        let mut stream = response.bytes_stream();

        tokio::spawn(async move {
            let mut buffer = String::new();
            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(Err(LlmError::Http(e))).await;
                        break;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(pos) = buffer.find('\n') {
                    let line = buffer.drain(..=pos).collect::<String>();
                    let line = line.trim();
                    if line.is_empty() || line == "data: [DONE]" {
                        continue;
                    }
                    let payload = line.strip_prefix("data: ").unwrap_or(line);
                    if payload == "[DONE]" {
                        continue;
                    }
                    match serde_json::from_str::<CompletionStreamChunk>(payload) {
                        Ok(chunk) => {
                            if tx.send(Ok(chunk)).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            if tx.send(Err(LlmError::Json(e))).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(rx)
    }
}

#[async_trait::async_trait]
impl LlmClient for OpenAiClient {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let body = serde_json::to_value(&request)?;
        let response = self
            .http
            .post(format!("{}/chat/completions", self.config.base_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status,
                message: text,
            });
        }

        let json = response.json().await?;
        Ok(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_builder_defaults() {
        let config = OpenAiConfig::builder().api_key("sk-test").build();
        assert_eq!(config.api_key, "sk-test");
        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(config.timeout, Duration::from_secs(60));
    }

    #[test]
    fn config_builder_custom_base_url() {
        let config = OpenAiConfig::builder()
            .api_key("sk-test")
            .base_url("https://api.example.com/v1")
            .build();
        assert_eq!(config.base_url, "https://api.example.com/v1");
    }

    #[test]
    fn config_builder_custom_timeout() {
        let config = OpenAiConfig::builder()
            .api_key("sk-test")
            .timeout(Duration::from_secs(30))
            .build();
        assert_eq!(config.timeout, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn complete_sends_correct_request() {
        use crate::message::{CompletionRequest, Message};
        use wiremock::matchers::{body_partial_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer sk-test"))
            .and(header("Content-Type", "application/json"))
            .and(body_partial_json(serde_json::json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hello"}]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "resp_1",
                "object": "chat.completion",
                "created": 1234567890,
                "model": "gpt-4",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "hi"},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
            })))
            .mount(&mock_server)
            .await;

        let config = OpenAiConfig::builder()
            .api_key("sk-test")
            .base_url(mock_server.uri())
            .build();
        let client = OpenAiClient::new(config).unwrap();

        let result = client
            .complete(CompletionRequest::new(
                "gpt-4",
                vec![Message::user("hello")],
            ))
            .await
            .unwrap();

        assert_eq!(result.id, "resp_1");
        assert_eq!(result.usage.total_tokens, 8);
    }

    #[tokio::test]
    async fn complete_handles_api_error() {
        use crate::message::{CompletionRequest, Message};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {"message": "Invalid API key"}
            })))
            .mount(&mock_server)
            .await;

        let config = OpenAiConfig::builder()
            .api_key("bad-key")
            .base_url(mock_server.uri())
            .build();
        let client = OpenAiClient::new(config).unwrap();

        let result = client
            .complete(CompletionRequest::new(
                "gpt-4",
                vec![Message::user("hello")],
            ))
            .await;

        assert!(result.is_err());
    }
}
