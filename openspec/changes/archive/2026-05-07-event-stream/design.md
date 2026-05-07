## Context

MMAT is being rewritten as a persistent, event-driven engineering organisation. The old codebase used NAAF's Step/Pipeline model with prompt-based LLM agent projections. The new architecture replaces this with structured cognitive events flowing through a central bus, durable memory, and role-based actors. The event stream crate is the foundation — without it, no role can communicate, no memory can be written, and no governance can operate.

The three crates in this changeset are all infrastructure with zero role-specific logic. They exist to be consumed by every subsequent changeset (memory-core, coordinator, roles).

## Goals / Non-Goals

**Goals:**
- Define the complete `SemanticEvent` enum covering all cognitive events the organisation will generate
- Provide a `tokio::broadcast`-based publish-subscribe event bus for intra-process distribution
- Provide a durable, append-only SQLite event log with replay capability
- Migrate the OpenAI-compatible LLM client from `naaf-llm` as a pure library (no agent projections)
- Migrate shell command execution from `naaf-process` as a pure library (no agent projections)
- Establish the Cargo workspace structure

**Non-Goals:**
- Role implementations (Intent Lead, Worker, etc.) — these are separate changesets
- Memory storage or retrieval — `memory-core` changeset
- Coordination, scheduling, or escalation logic — `coordinator` changeset
- Project orchestration (directory creation, repo init) — separate `project` crate
- Networking, IPC, or distributed event streaming — v1 is single-process
- Human interaction (`HumanIO`) — replaced by Intent Lead role consuming events

## Decisions

### Decision 1: `SemanticEvent` as a flat enum, not a trait hierarchy

**Chosen**: Single `#[derive(Clone, Debug, Serialize, Deserialize)] pub enum SemanticEvent` with one variant per event type.

**Rationale**: The event stream must be serializable for the event store, matchable for subscription filtering, and simple for `tokio::broadcast` (which requires `Clone`). A trait-based event system with `Any` downcasting would lose type safety and make replay from SQLite impossible. The flat enum is verbose but correct — it's the system's schema, not convenience code.

**Alternative considered**: Trait `Event` with `as_any()` downcasting. Rejected because it requires runtime type checking, prevents deserialization from the store, and makes subscription filtering complex.

### Decision 2: `tokio::broadcast` for the event bus (not `tokio::mpsc` or external broker)

**Chosen**: `tokio::broadcast::Sender<Arc<SemanticEvent>>` wrapped in an `EventBus` struct.

**Rationale**: Broadcast channels support multiple consumers (every role subscribes), bounded buffers with lag handling, and are well-tested. `mpsc` would require manually fanning out to each subscriber. An external broker (NATS, Redpanda) adds operational complexity this early — the `EventBus` trait abstraction allows swapping later.

**Trade-off**: `broadcast` drops slow consumers. We mitigate by using `Arc<SemanticEvent>` (cheap clones) and the durable event store as backpressure (consumers can replay from SQLite if they miss events).

### Decision 3: Event store as SQLite append-only log, not Kafka or file-based

**Chosen**: SQLite with a single `events` table keyed by monotonically increasing `EventId`. Each row stores the event variant discriminant and full JSON payload.

**Rationale**: SQLite is already a dependency (Qdrant metadata), requires zero operational setup, and supports efficient replay via `WHERE event_id > ?`. A file-based log would require custom indexing. An external broker would require running infrastructure.

**Schema**:
```sql
CREATE TABLE events (
    event_id TEXT PRIMARY KEY,   -- uuid::Uuid v4 as string
    rowid INTEGER NOT NULL,      -- autoincrement for efficient range scans
    variant TEXT NOT NULL,       -- discriminant e.g. "TaskAssigned"
    payload TEXT NOT NULL,       -- JSON serialisation
    timestamp_ns INTEGER NOT NULL,
    source_agent TEXT NOT NULL   -- RoleId serialised as string
);
CREATE UNIQUE INDEX idx_events_rowid ON events(rowid);
CREATE INDEX idx_events_variant ON events(variant);
```

### Decision 4: LLM crate as pure client library (no `LlmAgent` projections)

**Chosen**: Extract `OpenAiClient`, `Executor`, `Tool`, `ToolRegistry`, and message types from `naaf-llm`. Remove `LlmAgent`, `LlmTask`, `LlmCheck`, `LlmMaterialiser`, `LlmRepairPlanner`, `HumanIO`.

**Rationale**: In the new architecture, each role owns its LLM invocation strategy directly. The old `LlmAgent` projection system (`.task(closure)`, `.check(closure)`) was designed for NAAF's Step/Pipeline model. Roles are actors that receive contracts and emit events — they don't need adapter projections.

**What stays**: `OpenAiClient` (HTTP to OpenAI-compatible endpoints), `CompletionRequest`/`CompletionResponse`, `Message` (system/user/assistant/tool), streaming support, `Executor` (model→tool→result loop), `Tool` trait, `ToolRegistry`, `ToolSpec`, `ToolCall`.

**What goes**: `LlmAgent`, `LlmTask`, `LlmCheck`, `LlmMaterialiser`, `LlmRepairPlanner`, `HumanIO`, `HumanQuestion`, `HumanAnswer`, `MessageSource`, `OpenAiStreamObserver`, adaptor projections.

### Decision 5: Process crate as pure library (no `ProcessAgent` projections)

**Chosen**: Extract `ProcessCommand`, `ProcessOutput`, and the `tokio::process::Command` wrapper from `naaf-process`. Remove `ProcessAgent`, `ProcessTask`, `ProcessCheck`, `ProcessMaterialiser`, `ProcessRepairPlanner`.

**Rationale**: Same as LLM — roles invoke shell commands directly rather than through adapter projections.

### Decision 6: In-process only — no IPC, no networking

**Chosen**: All communication happens within a single `tokio` runtime. The event bus uses `tokio::broadcast`. No gRPC, no HTTP servers, no Unix sockets.

**Rationale**: The organisation simulator runs as one process. All roles are tasks within that process. Distribution is a future concern. This simplifies the architecture enormously and lets us focus on correctness.

## Risks / Trade-offs

- **[Risk] Broadcast channel overflow under high event volume** → Mitigation: Use `Arc<SemanticEvent>` for cheap cloning; set a generous buffer size; consumers that lag can replay from the SQLite event store.
- **[Risk] SQLite write contention on the event store** → Mitigation: All writes happen through the event bus (single publisher pattern); SQLite WAL mode for concurrent readers.
- **[Risk] Flat enum becomes unwieldy as event types grow** → Mitigation: Events are grouped by domain in the enum definition; a code-generation step could be added later to derive subscription filtering and serde from a schema file.
- **[Risk] Removing `HumanIO` before building Intent Lead** → Mitigation: Intent Lead is changeset 6 (understanding), not far off. Human interaction is temporarily unavailable between this changeset and that one.
- **[Trade-off] Monolithic crate grouping** → The three crates (`event-stream`, `llm`, `process`) are independent and could be three separate changesets. They're grouped because they share zero inter-dependencies and all need to exist before any role can operate. Splitting would create changeset coordination overhead with no benefit.

## Resolved Questions

- **EventId**: `uuid::Uuid` (v4). Enables distributed event IDs later. Range queries on the event store will use a separate monotonically increasing `rowid` column for efficient scanning, with `event_id` as the UUID primary key.
- **Batch writes**: Start with synchronous writes. Add batching behind a flag if throughput becomes an issue.
- **Crate location**: `llm` and `process` crates live in the MMAT workspace under `crates/`. Keeps development simple. Extraction to a separate repo can happen later if reuse warrants it.
