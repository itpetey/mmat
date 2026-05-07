## ADDED Requirements

### Requirement: Attention engine subscribes to the event stream
The system SHALL provide an `AttentionEngine` actor that subscribes to the event bus and processes each event for memory candidacy. It MUST handle events of types: `ClaimMade`, `DecisionRecorded`, `ToolExecuted`, `HumanFeedbackReceived`, and `ArtefactProduced`.

#### Scenario: Attention engine receives and scores events
- **WHEN** a `ClaimMade` event is published to the bus
- **THEN** the attention engine MUST receive the event and score it for salience
- **AND** if the salience score exceeds the configured threshold, a `MemoryProposed` event MUST be published

#### Scenario: Low-salience events are filtered out
- **WHEN** an event scores below the configured salience threshold
- **THEN** the attention engine MUST NOT publish a `MemoryProposed` event for it

### Requirement: Salience scoring uses an LLM for contextual judgment
The system SHALL send batched events to a dedicated LLM for salience assessment. The LLM MUST be prompted to identify events containing durable, consequential information — decisions, constraints, discoveries, risks, and user instructions. Events judged below the salience threshold MUST be dropped. The LLM MUST NOT be the same model instance used by role agents; it SHALL be a separate, configurable model.

#### Scenario: Batched events are scored by LLM
- **WHEN** the attention engine accumulates N events or T seconds have passed (whichever comes first)
- **THEN** it MUST send the batch to the salience LLM with a prompt asking "which of these events contain durable, consequential information?"
- **AND** events scored above threshold MUST proceed to memory proposal
- **AND** events scored below threshold MUST be dropped

#### Scenario: Salience LLM is configurable per deployment
- **WHEN** the attention engine is initialised
- **THEN** it MUST accept a configurable `LlmClient` for salience scoring, independent of role LLM clients
- **AND** the model, base URL, and API key MUST be configurable separately from the default LLM config

### Requirement: Duplicate detection prevents redundant proposals
The system SHALL query the memory store before publishing a `MemoryProposed` event to check if a semantically similar memory already exists. Semantic similarity MUST use the Qdrant vector index with a configurable similarity threshold.

#### Scenario: Near-duplicate is suppressed
- **WHEN** a new claim's embedding is within 0.95 cosine similarity of an existing memory
- **THEN** the attention engine MUST NOT publish a `MemoryProposed` event
- **AND** it MUST increment the existing memory's `last_accessed_at` timestamp (rehearsal)

#### Scenario: Novel claim passes duplicate check
- **WHEN** a new claim's embedding has cosine similarity < 0.95 to all existing memories
- **THEN** the duplicate check MUST pass
- **AND** the attention engine MUST proceed to publish `MemoryProposed` if salience threshold is met

### Requirement: Memory proposal carries typed metadata
The system SHALL ensure every `MemoryProposed` event includes the proposed memory type, content, recommended scope, recommended authority, confidence, and source event references. The event MUST NOT include an embedding (embedding is computed during acceptance, not proposal).

#### Scenario: Proposal captures all source context
- **WHEN** a `MemoryProposed` event is published
- **THEN** it MUST reference all source events via `evidence_refs`
- **AND** it MUST include the `source_agent` that produced the source event
- **AND** it MUST propose a `memory_type`, `scope`, `authority`, and `confidence` based on the source event characteristics
