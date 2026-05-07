## ADDED Requirements

### Requirement: LLM client supports OpenAI-compatible chat completions
The system SHALL provide an `LlmClient` trait and an `OpenAiClient` implementation that sends chat completion requests to an OpenAI-compatible HTTP endpoint. The client MUST support configurable base URL and API key. Requests MUST include a model name, a list of messages (system, user, assistant, tool), and optional tool specifications.

#### Scenario: Successful completion with text response
- **WHEN** `OpenAiClient::complete()` is called with a `CompletionRequest` containing a user message
- **THEN** the returned `CompletionResponse` MUST contain an assistant message with text content
- **AND** the response MUST include usage statistics (tokens consumed)

#### Scenario: Client uses configured base URL
- **WHEN** `OpenAiClient` is constructed with `base_url = "https://api.example.com/v1"`
- **THEN** completion requests MUST be sent to `https://api.example.com/v1/chat/completions`

#### Scenario: Client includes API key in requests
- **WHEN** `OpenAiClient` is constructed with `api_key = "sk-xxx"`
- **THEN** all HTTP requests MUST include an `Authorization: Bearer sk-xxx` header

### Requirement: LLM client supports streaming responses
The system SHALL support streaming chat completions where the response is delivered incrementally via a channel. The streaming API MUST deliver content deltas and final usage statistics.

#### Scenario: Streaming completion delivers incremental content
- **WHEN** `OpenAiClient::complete_streaming()` is called
- **THEN** content deltas MUST be delivered via the stream receiver as they arrive
- **AND** the final stream message MUST include usage statistics

### Requirement: Messages model the OpenAI message schema
The system SHALL provide `Message` and related types (`SystemMessage`, `UserMessage`, `AssistantMessage`, `ToolMessage`, `ToolCall`) that serialize to the OpenAI-compatible JSON format.

#### Scenario: User message serializes correctly
- **WHEN** `Message::user("Hello")` is serialized to JSON
- **THEN** it MUST produce `{"role": "user", "content": "Hello"}`

#### Scenario: Assistant message with tool calls serializes correctly
- **WHEN** an `AssistantMessage` with `tool_calls` is serialized to JSON
- **THEN** the JSON MUST include a `"tool_calls"` array with each call's `id`, `type`, and `function` fields
