## Why

The rewrite of MMAT replaces a fixed NAAF-based pipeline with a persistent, event-driven engineering organisation where autonomous roles coordinate through a shared cognitive substrate. The event stream is the foundational layer — every role, every memory write, every governance decision flows through it. Without a structured, append-only, durable event system, the entire architecture collapses into the same opaque prompt-chaining that the rewrite exists to escape.

## What Changes

- **New: `event-stream` crate** — Defines the full `SemanticEvent` enum covering all cognitive events the organisation generates (tool executions, claims, decisions, memory proposals, task assignments, reviews, escalations). Provides a `tokio::broadcast`-based `EventBus` for intra-process distribution and a SQLite-backed `EventStore` for durable append-only logging with replay capability.
- **New: `llm` crate** — Migrates the OpenAI-compatible client, streaming, tool-calling executor loop, and `Tool`/`ToolRegistry` traits from `naaf-llm`. **BREAKING**: The `LlmAgent` projection system (which maps a single agent into Task/Check/Materialiser/Repair roles) is removed — role behaviour is now owned by each role actor, not by LLM adapters. The `HumanIO` trait is not migrated; human interaction is handled by the Intent Lead role consuming events.
- **New: `process` crate** — Migrates shell-command execution (`ProcessCommand`, `ProcessOutput`) from `naaf-process`. The `ProcessAgent` adapter system is removed for the same reason as `LlmAgent`.
- **New: Cargo workspace** — Establishes the root workspace `Cargo.toml` with `[workspace]` members for these three crates, plus scaffolding for future crates.

## Capabilities

### New Capabilities

- `semantic-event-types`: All structured event variants (tool executions, claims, decisions, memory proposals, task lifecycle, reviews, escalations) with typed payloads, unique IDs, timestamps, and source agent attribution.
- `event-bus`: Intra-process publish-subscribe event distribution via `tokio::broadcast` with topic-based subscription filtering by event variant.
- `event-store`: Append-only durable event log backed by SQLite. Every event is written exactly once at publish time. Supports full replay for recovery and audit.
- `llm-client`: OpenAI-compatible chat completions client with streaming, configurable base URL and API key, model selection, and structured message types.
- `tool-execution`: Tool-calling executor loop that mediates the model-tool-call-tool-result cycle with configurable turn limits and token budgets. Tool registry with spec-based registration.
- `process-command`: Shell command execution adapter producing structured outputs (stdout, stderr, exit code) suitable for tool-calling and evidence logging.

### Modified Capabilities

None — this is a greenfield workspace with no existing specs.

## Impact

- **New crates**: `event-stream`, `llm`, `process` in `crates/`
- **Workspace root**: New `Cargo.toml` with `[workspace]` resolver 3, edition 2024
- **Dependencies**: `serde`, `serde_json`, `tokio`, `rusqlite`, `uuid`, `thiserror`, `tracing`, `reqwest`, `parking_lot`
- **NAAF replacement**: The `naaf-llm` and `naaf-process` crates are functionally replaced. The `naaf-core` traits (Task, Check, Step, Pipeline) are conceptually replaced by role actors and the coordinator (future changesets).
- **No user-facing changes yet** — these crates are pure library infrastructure consumed by subsequent changesets.
