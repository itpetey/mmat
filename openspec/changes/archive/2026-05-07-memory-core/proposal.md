## Why

The event stream stores raw cognitive events — but events are transient facts, not durable project reality. Without a typed semantic memory layer, every role must re-derive context from the event stream on each invocation, leading to redundant computation, context window pollution, and silent knowledge drift. The memory core transforms the event stream into a continuously curated, queryable, authority-annotated model of what the organisation knows — what is true, what was decided, what constraints exist, what changed, and what supersedes what.

## What Changes

- **New: `memory` crate** — Typed semantic memory system built on the event stream. Provides the `Memory` schema with eleven categories (Fact, Decision, Constraint, Preference, Risk, Lesson, SOP, Incident, Assumption, OpenQuestion, Relationship), each carrying scope, authority, confidence, decay policy, evidence references, and supersession chains.
- **New: Memory Store** — Dual-backend storage: SQLite for structured metadata (type, scope, authority, confidence, timestamps, relationships) and Qdrant for vector embeddings enabling semantic search. Both stores are append-only with the event store serving as the source of truth.
- **New: Attention Engine** — Subscribes to the event stream and extracts memory candidates. Scores salience, detects duplicates and near-duplicates, identifies novel information, and proposes memory writes or updates. Acts as the cognitive filter between raw events and durable memory.
- **New: Provenance Engine** — Tracks evidence chains: which claim connects to which tool execution, which memory was derived from which events, which review validated which artefact. Every memory item carries an immutable evidence lineage.
- **New: Librarian** — Memory governance role. Validates memory proposals against write gates (is it durable? grounded? scoped? invalidatable? non-duplicate? non-contradictory?). Enforces decay policies, handles supersession, maintains ontology consistency. Runs as a persistent actor consuming memory proposals from the event stream.

## Capabilities

### New Capabilities

- `memory-types`: The `Memory` struct and supporting types — `MemoryType` enum (11 variants), `MemoryScope` (ephemeral, project, organisational, world-model), `Authority` levels (compiler output down to speculative reasoning), `Confidence` (0.0–1.0), `DecayPolicy` (never, stale after N days, superseded only), `MemoryId`, and `SupersessionChain`.
- `memory-store`: Dual-backend persistence — SQLite for structured fields with indexed queries by type, scope, authority, and decay status; Qdrant for vector embeddings with semantic similarity search. Both stores are populated from accepted memory proposals in the event stream.
- `attention-engine`: Subscribes to the event stream, scores memory candidates by salience (does this matter beyond the current turn?), detects duplication against existing memory, identifies novel information, and publishes `MemoryProposed` events to the bus when a candidate passes threshold.
- `provenance-engine`: Subscribes to the event stream to build evidence chains. Links `ClaimMade` to `ToolExecuted` events by evidence references. Links `MemoryProposed` to its source events. Provides `trace_evidence(memory_id)` returning the full evidence graph.
- `librarian`: Persistent actor that consumes `MemoryProposed` events, applies write gates (durability, grounding, scope, invalidatability, duplicate detection, contradiction detection), and publishes `MemoryAccepted` or rejects with reason. Enforces decay policies by periodically scanning for stale memories and publishing `MemorySuperseded` events. Handles explicit supersession requests.

### Modified Capabilities

None — this is the second changeset in a greenfield workspace.

## Impact

- **New crate**: `crates/memory/` added to workspace members
- **Dependencies**: `event-stream` (consumes events from bus/store), `serde`, `serde_json`, `tokio`, `rusqlite` (bundled), `qdrant-client`, `uuid`, `thiserror`, `tracing`, `parking_lot`
- **Event stream integration**: The attention engine subscribes to the event bus. The librarian consumes and publishes events on the bus. The memory store reads from accepted events in the event store.
- **Qdrant dependency**: Requires a running Qdrant instance (same as the old MMAT codebase). Vector dimensions and collection configuration match the embedding model (default: `text-embedding-3-small`, 1536 dims).
- **Crate ordering**: This crate depends on `event-stream` (for `SemanticEvent`, `EventBus`, `EventStore`). It does not depend on `llm` or `process`.
