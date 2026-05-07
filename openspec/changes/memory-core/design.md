## Context

The event stream crate provides raw cognitive events (tool executions, claims, decisions, task lifecycles). But events are ephemeral — they describe what happened at a point in time. The system needs to know what is currently true, what was decided and why, what constraints apply, what risks exist, what lessons were learned, and what changed over time. This is the difference between an event log and institutional knowledge.

The conversation design establishes four memory classes: Ephemeral (minutes→days), Project (months→years), Organisational (cross-project, periodically reviewed), and Semantic World Models (relationships, causality). Each memory item carries type, scope, authority, confidence, decay policy, evidence lineage, and supersession chain.

The memory-core changeset builds this layer — the `memory` crate, its store, and the three subsystems that feed it (attention engine, provenance engine, librarian).

## Goals / Non-Goals

**Goals:**
- Define the typed `Memory` schema with all eleven categories, four scopes, and authority hierarchy
- Provide dual-backend storage: SQLite for structured fields, Qdrant for vector embeddings
- Implement the attention engine: event stream subscription → salience scoring → memory candidate extraction
- Implement the provenance engine: evidence chain tracking from event IDs
- Implement the librarian: write gates, deduplication, supersession, decay enforcement
- All three subsystems are long-lived actors consuming and publishing on the event bus

**Non-Goals:**
- Role-specific memory retrieval strategies (that's coordinator/retrieval planner)
- Human-in-the-loop memory approval (that's Intent Lead)
- Cross-project organisational memory inheritance (v1 is single-project)
- Memory compaction or archival (future optimisation)
- Custom embedding models (uses Qdrant with OpenAI-compatible embeddings)

## Decisions

### Decision 1: Memory as a flat struct, not a trait hierarchy per type

**Chosen**: Single `Memory` struct with a `memory_type: MemoryType` discriminant field, rather than `Fact`, `Decision`, `Constraint` as separate types.

**Rationale**: The store queries across all types (e.g., "retrieve all project-scoped memories with high authority"). A trait hierarchy would require type-erased storage and make SQL queries complex. The flat struct with discriminant is simpler to store, query, and serialize. Domain-specific validation (e.g., a Decision must have rationale_refs) can be enforced at construction time via builder methods or the librarian's write gates.

```rust
struct Memory {
    id: MemoryId,
    memory_type: MemoryType,
    content: String,
    embedding: Option<Vec<f32>>,
    scope: MemoryScope,
    authority: Authority,
    confidence: Confidence,
    decay_policy: DecayPolicy,
    evidence_refs: Vec<EventId>,
    supersedes: Option<MemoryId>,
    superseded_by: Option<MemoryId>,
    created_at: Timestamp,
    last_accessed_at: Timestamp,
    source_agent: RoleId,
}
```

### Decision 2: SQLite for structured fields, Qdrant for vectors

**Chosen**: SQLite stores all structured metadata (type, scope, authority, confidence, decay, relationships). Qdrant stores embeddings keyed by `MemoryId`. The two stores are read independently — structured queries go to SQLite, semantic queries go to Qdrant, and the caller joins by `MemoryId`.

**Rationale**: This mirrors the old MMAT architecture (`SqliteKnowledgeGroupStore` + Qdrant). It works. SQLite provides rich querying (WHERE type=X AND scope=Y AND authority >= Z). Qdrant provides semantic search over the content embedding. The separation avoids forcing vector search for structured queries and vice versa.

**Alternative considered**: Single Postgres store with pgvector. Rejected because Qdrant is already a dependency with better vector search performance; Postgres would add operational complexity for no benefit in v1.

### Decision 3: LLM-driven salience scoring with batching

**Chosen**: The attention engine uses an LLM to score event salience rather than a deterministic heuristic. Events are batched (configurable interval or event count) and sent to a dedicated LLM with a prompt instructing it to identify durable, consequential information. High-salience events proceed to memory proposal; low-salience events are dropped.

**Rationale**: Accuracy of what becomes institutional memory matters far more than per-event latency or LLM cost. A deterministic heuristic would miss nuanced salience (e.g., a subtle constraint discovery embedded in a tool output). The LLM can understand context that keyword matching cannot. Batching avoids per-event API call overhead while keeping latency reasonable.

**Alternative considered**: Deterministic salience scoring based on event type weights and source agent authority. Rejected because it cannot distinguish between "trivial ClaimMade about formatting" and "important ClaimMade about a discovered API constraint" — both have the same event type and source agent.

### Decision 4: Attention engine as filter, not creator

**Chosen**: The attention engine consumes raw events, scores them for salience, and proposes memory candidates. It does NOT write to the memory store directly — it publishes `MemoryProposed` events. The librarian consumes those proposals and decides durability.

**Rationale**: This separation mirrors the conversation's architecture: cognition proposes, governance accepts. The attention engine can be aggressive (low threshold) because the librarian has strict write gates. It also means the attention engine can be tuned or replaced without affecting memory integrity.

### Decision 4: Librarian as persistent actor with periodic scans

**Chosen**: The librarian subscribes to `MemoryProposed` events in real-time for write validation, AND runs periodic scans (configurable interval) for decay enforcement. It publishes `MemoryAccepted`, `MemoryRejected`, and `MemorySuperseded` events.

**Rationale**: Decay is time-based (stale after N days). Real-time event processing can't trigger decay — there's no "time passed" event. A periodic scan is the simplest correct approach. The scan queries SQLite for memories where `decay_policy = stale_after_days AND created_at + days < now()`.

### Decision 5: Provenance via event ID references, not content hashing

**Chosen**: Evidence chains use `EventId` references stored in memory items' `evidence_refs` field. `ProvenanceEngine::trace(memory_id)` walks the reference graph by querying the event store.

**Rationale**: `EventId` is a UUID referencing the single source of truth (the event store). Content hashing would require storing event contents in memory, duplicating data. ID references are lighter and more auditable — you can verify that the referenced tool execution actually happened by checking the event store.

### Decision 6: Retrieval-scoped embeddings

**Chosen**: Embeddings are computed from `content` only (the memory text), not from the full struct. The Qdrant collection is configured during crate init with the embedding model's dimensions and metric (cosine).

**Rationale**: Semantic search needs text content, not structured fields. Mixing `"authority: high, scope: project"` into the embedding would dilute semantic relevance. Structured filtering (scope, authority, type) is done via SQLite WHERE clauses, and the results are joined with vector results by `MemoryId`.

## Risks / Trade-offs

- **[Risk] LLM salience scoring increases latency and cost** → Mitigation: Events are batched (process every N seconds or M events, whichever comes first) rather than calling the LLM per event. The salience LLM uses a fast/cheap model (configurable). Low-confidence scores default to "drop" — the librarian catches false negatives later.
- **[Risk] Attention engine overproduces memory candidates** → Mitigation: The librarian's write gates filter aggressively. The salience threshold is LLM-determined per batch, not a static number. The librarian remains the final authority.
- **[Risk] Qdrant and SQLite divergence** → Mitigation: Both stores are populated from the same accepted events. The memory store insert method is transactional: SQLite insert succeeds → Qdrant upsert succeeds → return. If Qdrant fails, the SQLite row is rolled back.
- **[Risk] Decay scan at scale** → Mitigation: SQLite index on `(decay_policy, created_at)` makes the scan efficient. Scan interval is configurable (default: hourly).
- **[Risk] Embedding API cost** → Mitigation: Embeddings are computed once at memory acceptance time and stored. Re-embedding only happens on content update (supersession).
- **[Trade-off] Librarian is a single actor (no distributed consensus)** → Simpler, correct for single-process v1. If the system becomes multi-process, the librarian's write gates would need consensus.

## Resolved Questions

- **Salience scoring**: LLM-driven. Accuracy of what becomes durable institutional memory matters far more than per-event latency. The attention engine batches events (process every N seconds or every M events) and sends them to an LLM for salience scoring. Low-confidence or irrelevant events are dropped; high-salience events proceed to memory proposal.
- **Qdrant collections**: Single collection with scope as a payload filter. Switching to per-scope later is a re-embedding operation, not a data loss. Cross-scope queries are useful (e.g., "has anyone in the organisation encountered this pattern?").
- **Project scope**: `MemoryScope::Project` means a single repository. A change request (OpenSpec change) uses project-scoped memories tagged with a change identifier, not a separate scope.
