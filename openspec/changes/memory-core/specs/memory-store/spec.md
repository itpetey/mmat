## ADDED Requirements

### Requirement: Memory store persists typed memories to SQLite
The system SHALL provide a `MemoryStore` with a SQLite backend that stores all structured fields of a `Memory`. The store MUST create the schema on first open and support insert, query-by-id, query-by-type, query-by-scope, query-by-authority-range, and query-by-decay-status operations.

#### Scenario: Memory is inserted and retrievable by ID
- **WHEN** a `Memory` is inserted via `store.insert(memory)`
- **THEN** `store.get_by_id(memory.id)` MUST return the same memory with all fields intact

#### Scenario: Memories are queryable by type
- **WHEN** multiple `Decision` and `Fact` memories exist
- **THEN** `store.query_by_type(MemoryType::Decision)` MUST return only `Decision` memories

#### Scenario: Memories are queryable by scope
- **WHEN** multiple memories with `Project` and `Ephemeral` scopes exist
- **THEN** `store.query_by_scope(MemoryScope::Project)` MUST return only `Project`-scoped memories

#### Scenario: Memories are queryable by authority range
- **WHEN** memories with varying authorities exist
- **THEN** `store.query_by_authority(min: ReviewFindings, max: CompilerOutput)` MUST return only memories within that authority range
  
#### Scenario: Decayed memories are queryable for cleanup
- **WHEN** the librarian's decay scan queries for memories past their decay date
- **THEN** the store MUST support `query_decayed()` returning memories where `decay_policy = StaleAfterDays(d) AND created_at + d days < now()`

### Requirement: Memory store indexes vector embeddings in Qdrant
The system SHALL integrate with Qdrant for vector similarity search. Each memory's `content` embedding MUST be upserted to a Qdrant collection keyed by `MemoryId`. The collection configuration (dimensions, distance metric) MUST be defined at store initialisation.

#### Scenario: Memory embedding is searchable
- **WHEN** a memory is inserted with an embedding
- **THEN** `store.search_similar(query_embedding, limit: 10)` MUST return the most similar memories ranked by cosine distance

#### Scenario: Embedding is upserted on memory update
- **WHEN** a memory's content is updated via supersession
- **THEN** the old embedding MUST be replaced in Qdrant with the new content's embedding

#### Scenario: Qdrant failure rolls back SQLite insert
- **WHEN** a memory is being inserted and the Qdrant upsert fails
- **THEN** the SQLite insert MUST be rolled back
- **AND** an error MUST be returned to the caller

### Requirement: Store supports supersession chains
The system SHALL maintain bidirectional supersession links: when memory B supersedes memory A, A's `superseded_by` MUST point to B's ID and B's `supersedes` MUST point to A's ID. The store MUST provide `get_supersession_chain(memory_id)` returning the full chain from original to current.

#### Scenario: Supersession chain is queryable
- **WHEN** memory A is superseded by B, and B is superseded by C
- **THEN** `store.get_supersession_chain(A)` MUST return `[A, B, C]`
- **AND** `store.get_supersession_chain(C)` MUST return `[A, B, C]`

#### Scenario: Retrieval returns only current (non-superseded) memories
- **WHEN** memory A has `superseded_by = Some(B_id)`
- **THEN** `store.query_by_type(MemoryType::Decision)` MUST NOT include A (only B and any non-superseded memories)
