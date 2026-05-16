## ADDED Requirements

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
