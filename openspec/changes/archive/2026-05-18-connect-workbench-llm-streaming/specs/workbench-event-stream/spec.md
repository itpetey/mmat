## ADDED Requirements

### Requirement: Workbench persists assistant stream completions
The workbench SHALL persist completed runtime assistant replies as semantic events through the shared runtime boundary before reporting stream completion to the client. Persisted assistant replies MUST include the selected lane, the assistant message ID, the user message ID they reply to, and the completed text content.

#### Scenario: Assistant stream completes successfully
- **WHEN** a runtime assistant stream finishes with text content for lane `lane-a`
- **THEN** the server MUST make an assistant message semantic event durable exactly once with `EventContext.lane_id` set to `lane-a`
- **AND** the event MUST reference the persisted user message ID that caused the assistant response
- **AND** the event MUST be broadcast to runtime and workbench live subscribers after durability is established

#### Scenario: Assistant persistence fails
- **WHEN** a runtime assistant stream finishes but the server cannot append the assistant message event through `mmat-db`
- **THEN** the server MUST send an assistant stream failure update to the client
- **AND** the server MUST NOT send a completion update for that assistant message
- **AND** the failed assistant response MUST NOT appear in replayed transcript snapshots

### Requirement: Workbench projections include assistant messages
The workbench SHALL project persisted assistant message events into lane transcripts alongside human messages. A reloaded client MUST see completed assistant replies from persisted events without relying on browser memory.

#### Scenario: Client reloads after assistant reply
- **WHEN** a completed assistant message event exists for lane `lane-a`
- **AND** the browser reloads and requests the transcript for `lane-a`
- **THEN** the transcript MUST include a message row for the assistant reply
- **AND** the row MUST use the persisted assistant event ID as its stable item ID
- **AND** the row MUST be omitted from unrelated lane transcripts

#### Scenario: Live assistant event updates projection
- **WHEN** a completed assistant message event is broadcast while a client is connected to the matching lane
- **THEN** the client MUST be able to apply the update without duplicating any assistant row already created by streaming deltas
