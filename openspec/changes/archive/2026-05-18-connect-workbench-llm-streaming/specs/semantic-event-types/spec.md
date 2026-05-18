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
- `AssistantMessageProduced` — assistant message ID, reply-to message ID, content, finish reason
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

#### Scenario: Assistant message carries reply provenance
- **WHEN** the workbench persists a completed assistant reply to a lane-scoped user message
- **THEN** the event MUST use `AssistantMessageProduced`
- **AND** the event context MUST include the same primary lane as the user message
- **AND** the event MUST include the assistant message ID and the user message ID it replies to
- **AND** the event MUST include the completed assistant content
