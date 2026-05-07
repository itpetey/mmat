## 1. Crate Setup

- [ ] 1.1 Scaffold `crates/memory/Cargo.toml` with dependencies (event-stream with path, serde, serde_json, tokio, rusqlite bundled, qdrant-client, uuid with serde+v4, thiserror, tracing, parking_lot)
- [ ] 1.2 Add `crates/memory` to workspace members in root `Cargo.toml`
- [ ] 1.3 Create `src/lib.rs` with module declarations: `types`, `store`, `attention`, `provenance`, `librarian`

## 2. Memory Types

- [ ] 2.1 Define `MemoryId` as `uuid::Uuid` newtype with Serialize, Deserialize, Copy, Clone, Display, From<Uuid>
- [ ] 2.2 Define `MemoryType` enum with 11 variants (Fact, Decision, Constraint, Preference, Risk, Lesson, SOP, Incident, Assumption, OpenQuestion, Relationship), each with a `discriminant_str()` method for SQLite storage
- [ ] 2.3 Define `MemoryScope` enum (Ephemeral, Project, Organisational, WorldModel) with `default_decay()` method returning suggested DecayPolicy
- [ ] 2.4 Define `Authority` enum with total ordering (CompilerOutput > UserInstruction > RepositoryState > AcceptedADR > ReviewFindings > LLMInference > SpeculativeReasoning), derive `Ord`/`PartialOrd`
- [ ] 2.5 Define `Confidence` as `f64` newtype with constructor validation (0.0..=1.0) and `Serialize`/`Deserialize`
- [ ] 2.6 Define `DecayPolicy` enum (Never, StaleAfterDays(u32), SupersededOnly) with `is_decayed(&self, created_at: DateTime<Utc>) -> bool`
- [ ] 2.7 Define `Memory` struct with all fields from design.md (id, memory_type, content, embedding, scope, authority, confidence, decay_policy, evidence_refs, supersedes, superseded_by, created_at, last_accessed_at, source_agent)
- [ ] 2.8 Implement `Memory::builder()` returning a typed builder with required-field enforcement and `build()` validation
- [ ] 2.9 Implement `Serialize`/`Deserialize` for `Memory` (skip embedding in serde, handle separately for Qdrant)

## 3. Memory Store — SQLite

- [ ] 3.1 Implement `MemoryStore` struct with `rusqlite::Connection`
- [ ] 3.2 Implement `MemoryStore::open(path)` — opens/creates database, runs schema migration, sets WAL mode
- [ ] 3.3 Implement schema migration: CREATE TABLE memories with columns for all structured fields, indices on (memory_type), (scope), (authority), (superseded_by), (decay_policy, created_at)
- [ ] 3.4 Implement `MemoryStore::insert(&self, memory: &Memory)` — serializes and inserts row
- [ ] 3.5 Implement `MemoryStore::get_by_id(&self, id: MemoryId)` — returns `Option<Memory>`
- [ ] 3.6 Implement `MemoryStore::query_by_type(&self, memory_type: MemoryType)` — returns `Vec<Memory>`
- [ ] 3.7 Implement `MemoryStore::query_by_scope(&self, scope: MemoryScope)` — returns `Vec<Memory>`
- [ ] 3.8 Implement `MemoryStore::query_by_authority(&self, min: Authority, max: Authority)` — returns `Vec<Memory>` within range
- [ ] 3.9 Implement `MemoryStore::query_decayed(&self)` — returns memories past their decay date
- [ ] 3.10 Implement `MemoryStore::supersede(&self, old_id: MemoryId, new_id: MemoryId)` — updates `superseded_by` on old, `supersedes` on new, transactionally
- [ ] 3.11 Implement `MemoryStore::get_supersession_chain(&self, id: MemoryId)` — walks `supersedes` and `superseded_by` to return full ordered chain
- [ ] 3.12 Implement `MemoryStore::query_current_only(&self, query_fn)` — filter wrapper that excludes superseded memories (superseded_by IS NOT NULL)

## 4. Memory Store — Qdrant

- [ ] 4.1 Implement `QdrantMemoryBackend` struct holding `qdrant_client::QdrantClient` + collection name + vector config
- [ ] 4.2 Implement `QdrantMemoryBackend::new(config)` — connects to Qdrant, creates collection if not exists with configured dimensions and cosine distance
- [ ] 4.3 Implement `QdrantMemoryBackend::upsert(&self, id: MemoryId, embedding: Vec<f32>, payload: HashMap<String, Value>)`
- [ ] 4.4 Implement `QdrantMemoryBackend::search(&self, query_embedding: Vec<f32>, limit: u64)` — returns ranked `Vec<(MemoryId, f32)>` (id + score)
- [ ] 4.5 Implement `QdrantMemoryBackend::delete(&self, id: MemoryId)` — removes point from collection
- [ ] 4.6 Implement `QdrantMemoryConfig` with `url`, `api_key`, `collection_name`, `vector_dimension`
- [ ] 4.7 Implement `MemoryStore::insert_with_embedding(...)` — transactional insert to SQLite, then Qdrant upsert; roll back SQLite on Qdrant failure
- [ ] 4.8 Implement `MemoryStore::search_similar(&self, embedding: Vec<f32>, limit: u64)` — delegates to Qdrant backend

## 5. Attention Engine

- [ ] 5.1 Implement `AttentionEngine` struct with config (salience_threshold: f64, similarity_threshold: f64)
- [ ] 5.2 Implement `AttentionEngine::run(bus, store)` actor loop — subscribes to event bus for ClaimMade, DecisionRecorded, ToolExecuted, HumanFeedbackReceived, ArtefactProduced
- [ ] 5.3 Implement salience scoring function: weights event type (HumanFeedback > ToolExecuted > ClaimMade > DecisionRecorded), source agent authority, evidence presence, confidence
- [ ] 5.4 Implement novelty check: compute embedding of proposed content, query Qdrant for similar memories, suppress if cosine similarity > threshold
- [ ] 5.5 Implement rehearsal: when a near-duplicate is detected, update existing memory's `last_accessed_at` via store
- [ ] 5.6 Publish `MemoryProposed` event when salience threshold is met and novelty check passes
- [ ] 5.7 Implement `AttentionConfig` with configurable `salience_threshold` (default 0.5), `similarity_threshold` (default 0.95)

## 6. Provenance Engine

- [ ] 6.1 Implement `ProvenanceEngine` struct with in-memory `HashMap<EventId, Vec<EventId>>` evidence index
- [ ] 6.2 Implement `ProvenanceEngine::run(bus, event_store)` actor loop — subscribes to event bus, indexes ClaimMade and DecisionRecorded events with their evidence_refs
- [ ] 6.3 Implement startup replay: query event store for existing ClaimMade/DecisionRecorded events and rebuild index
- [ ] 6.4 Implement `ProvenanceEngine::trace_evidence(&self, event_id: EventId)` — walks evidence_refs recursively, returns ordered chain of events from store
- [ ] 6.5 Implement `ProvenanceEngine::trace_memory(&self, memory: &Memory)` — traces from memory's evidence_refs through the event chain
- [ ] 6.6 Implement `ProvenanceEngine::assess_confidence(&self, event_id: EventId)` — scores based on evidence chain quality (direct tool output > indirect claim > none)
- [ ] 6.7 Implement broken evidence detection: if a ClaimMade references a non-existent EventId, publish `PolicyViolationDetected` event

## 7. Librarian

- [ ] 7.1 Implement `Librarian` struct with `MemoryStore` reference, `QdrantMemoryBackend` reference
- [ ] 7.2 Implement `Librarian::run(bus, store, qdrant)` actor loop — subscribes to `MemoryProposed` events
- [ ] 7.3 Implement durability gate: is proposed content meaningful beyond current turn? (reject transient/trivial claims)
- [ ] 7.4 Implement grounding gate: does proposal have evidence_refs or come from user instruction? (reject ungrounded LLM claims)
- [ ] 7.5 Implement scope gate: is proposed scope appropriate for memory type? (reject SOP with Ephemeral scope, etc.)
- [ ] 7.6 Implement duplicate gate: query Qdrant for similar memories, reject if cosine similarity > 0.92
- [ ] 7.7 Implement contradiction detection: query store for same-type, same-scope memories; compare content embeddings; detect contradictions
- [ ] 7.8 Implement authority-based contradiction resolution: higher authority overrides lower; equal authority favours recency
- [ ] 7.9 Implement memory acceptance: compute embedding, insert to MemoryStore, publish `MemoryAccepted` event
- [ ] 7.10 Implement memory rejection: publish `MemoryRejected` event with reason and gate that failed
- [ ] 7.11 Implement explicit supersession handling: consume `MemorySuperseded` events, update store
- [ ] 7.12 Implement periodic decay scan (tokio::time::interval, default 1 hour): query store for decayed memories, publish `MemorySuperseded` for each
- [ ] 7.13 Implement ontology violations: check type-scope combinations, downgrade authority for unsupported Facts, publish `PolicyViolationDetected` events

## 8. Integration

- [ ] 8.1 Write integration test: event published → attention engine produces MemoryProposed → librarian accepts → memory in store → searchable
- [ ] 8.2 Write integration test: near-duplicate event → attention engine suppresses → rehearsal updates access time
- [ ] 8.3 Write integration test: ungrounded proposal → librarian rejects → MemoryRejected published
- [ ] 8.4 Write integration test: contradiction with higher authority → librarian supersedes old memory
- [ ] 8.5 Write integration test: provenance engine traces evidence chain from memory back to tool execution
- [ ] 8.6 Write integration test: decay scan detects stale memory and supersedes it

## 9. Validation

- [ ] 9.1 `cargo fmt --all` passes
- [ ] 9.2 `cargo clippy -- -D warnings` passes on all crates
- [ ] 9.3 `cargo test` passes all tests including doc tests
