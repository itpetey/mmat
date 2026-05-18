# workbench-event-stream Specification

## Purpose
Define how the workbench turns user commands into durable semantic events, replays event-backed projections, and exposes System lane state for unscoped events.

## Requirements

### Requirement: Workbench processes commands through semantic events
The workbench SHALL translate user-facing commands into persisted `SemanticEvent` records before broadcasting live updates. Command handling MUST append events through `mmat-db` and MUST broadcast only after persistence succeeds.

#### Scenario: User sends a lane message
- **WHEN** the user submits a message while a persisted lane is selected
- **THEN** the server MUST append a `HumanFeedbackReceived` event with that lane ID in its context
- **AND** the event MUST be broadcast to live subscribers after the database append succeeds

#### Scenario: Event persistence fails
- **WHEN** the server cannot append a command-derived event through `mmat-db`
- **THEN** the event MUST NOT be broadcast as accepted
- **AND** the client MUST receive an error update for the failed command

### Requirement: Workbench provides replayable projection snapshots
The workbench SHALL build client snapshots from persisted events and lane records. A new or reloaded client MUST receive current lane navigation state and lane transcript state without relying on local browser memory.

#### Scenario: Client connects after reload
- **WHEN** the browser establishes the workbench stream after a reload
- **THEN** the server MUST send a snapshot containing persisted active lanes, archived lanes, the synthetic System lane, and projected transcript rows
- **AND** messages previously persisted for lanes MUST remain visible in their lanes

#### Scenario: Live event updates projection
- **WHEN** a persisted event is broadcast while a client is connected
- **THEN** the client MUST receive an update that can be applied to its workbench projection

### Requirement: Unscoped events appear in a synthetic System lane
The workbench projection SHALL expose a UI-only System lane for events whose `EventContext.lane_id` is absent. The System lane MUST NOT be persisted as a normal lane and MUST NOT be a destination for ordinary chat commands.

#### Scenario: Runtime emits unscoped event
- **WHEN** a semantic event is persisted without a lane ID
- **THEN** the event MUST appear in the System lane projection
- **AND** no lane row MUST be created for the System lane

#### Scenario: User attempts to send chat to System lane
- **WHEN** the client attempts to submit a normal chat message to the System lane
- **THEN** the server MUST reject the command or require selection of a persisted lane

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
