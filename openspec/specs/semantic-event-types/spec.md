## ADDED Requirements

### Requirement: Event types are defined as a single serializable enum
The system SHALL define a `SemanticEvent` enum with one variant per cognitive event type. Every variant MUST include a unique `EventId`, a `source_agent` field identifying the originating role, and a `timestamp_ns` in nanoseconds since epoch. All variants MUST implement `Clone`, `Debug`, `Serialize`, and `Deserialize`.

The variants SHALL include at minimum:
- `ToolExecuted` — tool name, arguments, exit code, stdout, stderr
- `ClaimMade` — claim text, evidence references, confidence score
- `DecisionRecorded` — decision text, rationale references
- `MemoryProposed` — memory type, content, scope, proposed authority
- `MemoryAccepted` — memory ID, accepted authority
- `MemorySuperseded` — old memory ID, new memory ID, reason
- `TaskAssigned` — task ID, worker ID, contract reference, dependencies
- `TaskStarted` — task ID, worker ID
- `TaskCompleted` — task ID, output artefact reference
- `TaskFailed` — task ID, error description
- `ReviewRequested` — task ID, reviewer ID
- `ReviewCompleted` — task ID, findings list, accepted boolean
- `EscalationRequested` — from role, to role, reason, severity
- `HumanFeedbackRequested` — question, context
- `HumanFeedbackReceived` — answer
- `ArtefactProduced` — artefact type, reference, producer role

#### Scenario: Event carries full provenance
- **WHEN** a Worker emits a `ClaimMade` event asserting "cargo test passed"
- **THEN** the event MUST include `evidence_refs` linking to a prior `ToolExecuted` event
- **AND** the `source_agent` MUST identify the Worker role
- **AND** the `timestamp_ns` MUST be set at event construction time

#### Scenario: Events are serializable for durable storage
- **WHEN** any `SemanticEvent` variant is serialized to JSON
- **THEN** it MUST produce a valid JSON object containing all variant fields
- **AND** deserializing that JSON MUST reconstruct an identical event

### Requirement: Event bus distributes events to all subscribers
The system SHALL provide an `EventBus` backed by `tokio::broadcast` that allows multiple concurrent subscribers to receive published events. Publishing MUST be non-blocking for producers. Subscribers that fall behind the buffer capacity MUST receive a `Lagging` error rather than blocking the producer.

#### Scenario: Multiple subscribers receive the same event
- **WHEN** a `TaskAssigned` event is published to the bus
- **THEN** all active subscribers with matching subscriptions MUST receive a clone of that event
- **AND** the publish call MUST return immediately without waiting for subscriber processing

#### Scenario: Subscriber filters by event variant
- **WHEN** a subscriber registers interest only in `TaskAssigned` and `TaskCompleted` variants
- **THEN** it MUST NOT receive events of other variants published to the bus

#### Scenario: Lagging subscriber is notified
- **WHEN** a subscriber is slower than the broadcast channel buffer capacity
- **THEN** its next `recv()` call MUST return a `RecvError::Lagged(n)` indicating how many events were dropped
- **AND** the subscriber MAY recover by replaying events from the event store

### Requirement: Event store provides durable append-only logging
The system SHALL provide an `EventStore` backed by SQLite that durably records every published event in insertion order. Events MUST be queryable by `EventId` range and by variant type. The store MUST use WAL journal mode for concurrent read access.

#### Scenario: Event is durably stored on publish
- **WHEN** a `SemanticEvent` is written to the event store
- **THEN** a row MUST be inserted into the `events` table with the variant discriminator, full JSON payload, timestamp, and source agent
- **AND** the row MUST survive process restart

#### Scenario: Events are replayable from a given point
- **WHEN** a subscriber queries the event store with `WHERE event_id > ?`
- **THEN** it MUST receive all events with IDs greater than the given value in ascending `event_id` order
- **AND** the query MUST be efficient (index scan, not full table scan)

#### Scenario: Events can be filtered by variant
- **WHEN** a subscriber queries the event store with `WHERE variant = 'TaskAssigned'`
- **THEN** it MUST receive only events of that variant type
- **AND** the query MUST use the variant index
