## MODIFIED Requirements

### Requirement: Event types are defined as a single serializable enum
The system SHALL define a `SemanticEvent` enum with one variant per cognitive event type. Every variant MUST include a unique `EventId`, a `source_agent` field identifying the originating role, a `timestamp_ns` in nanoseconds since epoch, and common `EventContext` metadata. `EventContext` MUST include organisation, workspace, project, run, optional task, optional primary lane, causation, and correlation identifiers. All variants MUST implement `Clone`, `Debug`, `Serialize`, and `Deserialize`.

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
- `LaneCreated` — lane ID, name, branch metadata, source message/event reference
- `LaneArchived` — lane ID
- `ProjectCreated` — project ID, host work directory
- `ProjectListed` — project ID, path, source agent
- `ProjectRenamed` — project ID, old name, new name
- `ProjectDeleted` — project ID, name

#### Scenario: Event carries full provenance
- **WHEN** a Worker emits a `ClaimMade` event asserting "cargo test passed"
- **THEN** the event MUST include `evidence_refs` linking to a prior `ToolExecuted` event
- **AND** the `source_agent` MUST identify the Worker role
- **AND** the `timestamp_ns` MUST be set at event construction time
- **AND** the event context MUST identify the project/run and MAY identify the primary lane

#### Scenario: Events are serializable for durable storage
- **WHEN** any `SemanticEvent` variant is serialized to JSON
- **THEN** it MUST produce a valid JSON object containing all variant fields and context fields
- **AND** deserializing that JSON MUST reconstruct an identical event

## REMOVED Requirements

### Requirement: Event store provides durable append-only logging
**Reason**: Durable event storage is no longer part of semantic event type ownership and MUST be provided by `mmat-db`.
**Migration**: Use `mmat-db` event append/replay/query functions and keep `mmat-event-stream` focused on event definitions and live pub/sub.
