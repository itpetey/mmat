## ADDED Requirements

### Requirement: Provenance engine builds evidence chains from the event stream
The system SHALL provide a `ProvenanceEngine` that subscribes to the event bus, tracks `ClaimMade` → `ToolExecuted` evidence references, and provides trace queries for any `MemoryId` or `EventId`. The engine MUST maintain an in-memory index of evidence relationships for fast trace queries.

#### Scenario: Evidence chain is traceable from memory back to tool execution
- **WHEN** a `Memory` carries `evidence_refs` pointing to a `ClaimMade` event
- **AND** that `ClaimMade` event references a `ToolExecuted` event
- **THEN** `provenance.trace(memory_id)` MUST return all three nodes in order: tool execution → claim → memory

#### Scenario: Untraceable claim is flagged
- **WHEN** a `ClaimMade` event references an `EventId` that does not exist in the event store
- **THEN** the provenance engine MUST flag the claim as having broken evidence
- **AND** it MUST publish a `PolicyViolationDetected` event

### Requirement: Provenance index is rebuilt from event store on startup
The system SHALL rebuild its in-memory evidence index by replaying relevant events from the event store on startup. Only `ClaimMade`, `DecisionRecorded`, `MemoryAccepted`, and `ToolExecuted` events need to be replayed.

#### Scenario: Provenance engine starts with empty event store
- **WHEN** the provenance engine starts and the event store is empty
- **THEN** the in-memory index MUST be empty
- **AND** new events MUST be indexed as they arrive on the bus

#### Scenario: Provenance engine starts with existing events
- **WHEN** the provenance engine starts and the event store contains prior `ClaimMade` and `ToolExecuted` events
- **THEN** the in-memory index MUST contain all evidence relationships from the replayed events
- **AND** trace queries MUST work immediately for those relationships

### Requirement: Provenance engine supports confidence assessment
The system SHALL provide a method to assess the confidence of a claim based on its evidence chain. Direct evidence (tool output) confers high confidence; indirect evidence (another claim) confers medium confidence; no evidence confers low confidence.

#### Scenario: Claim with direct tool evidence gets high confidence assessment
- **WHEN** a `ClaimMade` event references a `ToolExecuted` event
- **AND** the tool executed successfully (exit code 0)
- **THEN** `provenance.assess_confidence(claim_event_id)` MUST return a confidence value >= 0.8

#### Scenario: Claim with no evidence gets low confidence assessment
- **WHEN** a `ClaimMade` event has an empty `evidence_refs` list
- **THEN** `provenance.assess_confidence(claim_event_id)` MUST return a confidence value <= 0.3
