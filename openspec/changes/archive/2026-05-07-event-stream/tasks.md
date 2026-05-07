## 1. Workspace Setup

- [x] 1.1 Create root `Cargo.toml` with `[workspace]` (resolver 3, edition 2024, members: `crates/event-stream`, `crates/llm`, `crates/process`)
- [x] 1.2 Move existing `rustfmt.toml` to root, verify `reorder_imports = true`
- [x] 1.3 Scaffold `crates/event-stream/Cargo.toml` with dependencies (serde, serde_json, tokio with broadcast+sync, rusqlite with bundled, uuid with serde+v4, thiserror, tracing, parking_lot)
- [x] 1.4 Scaffold `crates/llm/Cargo.toml` with dependencies (serde, serde_json, tokio, reqwest with json+default-tls, thiserror, tracing, uuid)
- [x] 1.5 Scaffold `crates/process/Cargo.toml` with dependencies (serde, serde_json, tokio with process, thiserror)
- [x] 1.6 Create placeholder `src/lib.rs` for each crate

## 2. LLM Crate — Message Types

- [x] 2.1 Port `Message` enum (System, User, Assistant, Tool) and associated structs from `naaf-llm`
- [x] 2.2 Port `ToolCall` and `ToolCallFunction` structs with correct serde field naming
- [x] 2.3 Port `CompletionRequest` (model, messages, tools, tool_choice, temperature, max_tokens) and `CompletionResponse` with usage stats
- [x] 2.4 Port streaming delta types (`ChoiceDelta`, `StreamChoice`, `CompletionStreamChunk`, `StreamUsage`)
- [x] 2.5 Write serde round-trip tests for all message and completion types

## 3. LLM Crate — OpenAI Client

- [x] 3.1 Port `OpenAiConfig` (api_key, base_url) with builder pattern
- [x] 3.2 Port `LlmClient` trait with `complete()` method returning `CompletionResponse`
- [x] 3.3 Port `OpenAiClient` implementing `LlmClient` with `reqwest` HTTP calls
- [x] 3.4 Port streaming client: `complete_streaming()` method returning a channel receiver of stream chunks
- [x] 3.5 Port `LlmError` error type covering HTTP, JSON, and API errors
- [x] 3.6 Remove all NAAF-specific code: `LlmAgent`, `HumanIO`, `OpenAiStreamObserver`, `MessageSource`, adaptor projections

## 4. LLM Crate — Tool System

- [x] 4.1 Port `ToolSpec` struct (name, description, input_schema as `serde_json::Value`)
- [x] 4.2 Port `Tool` trait with `spec()` and `call()` methods, generic over `Runtime` and `Error`
- [x] 4.3 Port `ToolRegistry` with `register()`, `tool_specs()`, and duplicate-name rejection
- [x] 4.4 Port `Executor` with `ExecutorConfig` (max_turns, max_tokens) and the model-tool-call-result loop
- [x] 4.5 Port `ExecutorError` variants (Client, TurnLimitExceeded, TokenLimitExceeded, Tool, Parse)

## 5. Process Crate

- [x] 5.1 Port `ProcessCommand` struct with shell command, working directory, and env vars
- [x] 5.2 Port `ProcessOutput` struct (stdout bytes, stderr bytes, exit_code) with `Serialize`/`Deserialize`
- [x] 5.3 Implement `ProcessCommand::execute()` using `tokio::process::Command`
- [x] 5.4 Implement `ProcessOutput::stdout_str()` and `ProcessOutput::stderr_str()` UTF-8 convenience methods
- [x] 5.5 Remove all NAAF-specific code: `ProcessAgent`, `ProcessTask`, `ProcessCheck`, adaptor projections

## 6. Event Stream Crate — Core Types

- [x] 6.1 Define `EventId` as `uuid::Uuid` newtype with `Serialize`, `Deserialize`, `Copy`, `Clone`, `Display`, `From<Uuid>`
- [x] 6.2 Define `RoleId` as a String-based identifier for source agents
- [x] 6.3 Define `SemanticEvent` enum with all 16+ variants specified in `semantic-event-types` spec, each carrying `EventId`, `source_agent`, and `timestamp_ns`
- [x] 6.4 Implement `SemanticEvent::new(variant_data)` constructor that auto-generates `EventId` and `timestamp_ns`
- [x] 6.5 Define supporting types: `EvidenceRef`, `TaskContract`, `ReviewFinding`, `EscalationSeverity`, `ArtefactRef`
- [x] 6.6 Implement `Serialize`/`Deserialize` for `SemanticEvent` with variant discrimination (tag field or externally tagged)

## 7. Event Stream Crate — Event Bus

- [x] 7.1 Implement `EventBus` wrapping `tokio::broadcast::Sender<Arc<SemanticEvent>>`
- [x] 7.2 Implement `EventBus::new(capacity)` constructor
- [x] 7.3 Implement `EventBus::publish(&self, event: SemanticEvent)` — wraps in `Arc`, sends to broadcast, triggers store write
- [x] 7.4 Implement `EventBus::subscribe(&self, filter: &[EventType])` returning a filtered `EventReceiver`
- [x] 7.5 Implement `EventReceiver` wrapping `tokio::broadcast::Receiver` with variant-based filtering
- [x] 7.6 Implement `EventReceiver::recv()` returning `Result<Arc<SemanticEvent>, RecvError>` with `Lagging(n)` handling

## 8. Event Stream Crate — Event Store

- [x] 8.1 Implement `EventStore` struct with `rusqlite::Connection`
- [x] 8.2 Implement `EventStore::open(path)` — opens or creates database, runs schema migration, sets WAL mode
- [x] 8.3 Implement schema migration: CREATE TABLE events with columns (event_id, variant, payload, timestamp_ns, source_agent) and variant index
- [x] 8.4 Implement `EventStore::insert(&self, event: &SemanticEvent)` — serializes to JSON, inserts row, returns `EventId`
- [x] 8.5 Implement `EventStore::replay(&self, after_row: i64, before_row: Option<i64>)` — queries by row range, deserializes, returns `Vec<SemanticEvent>`
- [x] 8.6 Implement `EventStore::query_by_variant(&self, variant: &str, after_row: Option<i64>, before_row: Option<i64>)`
- [x] 8.7 Implement `EventStore::latest_row(&self)` — returns `Option<i64>`
- [x] 8.8 Implement `EventStoreError` with `From<rusqlite::Error>` and `From<serde_json::Error>`

## 9. Integration

- [x] 9.1 Wire `EventBus::publish()` to call `EventStore::insert()` synchronously (single publisher → single writer pattern)
- [x] 9.2 Write integration test: publish event → subscriber receives → event is in store → replay returns it
- [x] 9.3 Write integration test: multiple subscribers with different filters
- [x] 9.4 Write integration test: subscriber replays from store after lag

## 10. Validation

- [x] 10.1 `cargo fmt --all` passes
- [x] 10.2 `cargo clippy -- -D warnings` passes on all crates
- [x] 10.3 `cargo test` passes all tests including doc tests
