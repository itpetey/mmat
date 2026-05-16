## MODIFIED Requirements

### Requirement: Workbench supports conversation lanes
The workbench SHALL support multiple durable conversation lanes per project. A lane MUST have an ID, project ID, title/name, optional summary/purpose, status, creator, optional parent lane ID, optional origin event/message reference, created timestamp, and updated timestamp. Lanes are conversation branches and MUST NOT be modelled as multi-lane tags.

#### Scenario: User creates a lane
- **WHEN** the user creates a lane named `Notification-thread UX`
- **THEN** the lane MUST be persisted through `mmat-db`
- **AND** the lane MUST appear in the active lane list
- **AND** the lane transcript MAY initially be empty

#### Scenario: Lane is archived
- **WHEN** a lane is archived
- **THEN** its persisted status MUST become archived
- **AND** it MUST move from active lane navigation to the archived sidebar group
- **AND** its messages MUST remain visible when the archived lane is opened

#### Scenario: Project has no active lanes
- **WHEN** a project has no active lanes
- **THEN** the workbench MUST show an empty lane state with an affordance to create a lane

### Requirement: LLM tools can create lanes
The system SHALL expose a lane-creation tool that roles or LLM workflows can call during conversation. Tool-created lanes MUST be persisted and MUST record provenance linking back to the originating lane and event/message when available.

#### Scenario: Tool creates lane from message
- **WHEN** a role calls `create_lane` with a source message or event ID, name, and purpose
- **THEN** the workbench MUST create a persisted lane
- **AND** the parent lane transcript MUST show a fork link to the new lane

#### Scenario: Tool-created lane records provenance
- **WHEN** a lane is created by a role/tool
- **THEN** the lane metadata MUST record the creating role/tool and source message or event ID

### Requirement: Messages and events carry lane metadata
Conversation messages and lane-scoped runtime events SHALL carry one primary lane via `EventContext.lane_id`. Events without a lane ID SHALL be projected into the synthetic System lane.

#### Scenario: Single-lane view filters messages
- **WHEN** the user opens a persisted lane
- **THEN** the chat MUST show messages whose event context primary lane matches that lane
- **AND** unrelated lane and System events MUST be hidden by default

#### Scenario: System lane shows unscoped events
- **WHEN** the user opens the System lane
- **THEN** the chat or event transcript MUST show events whose event context has no lane ID
- **AND** the System lane MUST NOT be persisted as a normal lane

### Requirement: Notifications deep-link to context
Notifications SHALL reference a chat message, lane, or DAG node and SHALL navigate to that context when clicked.

#### Scenario: Notification targets a message
- **WHEN** the user clicks a notification with `message_id` and `lane_id`
- **THEN** the UI MUST open the referenced lane
- **AND** scroll/focus the referenced message

#### Scenario: Notification targets DAG node
- **WHEN** the user clicks a notification with a DAG node target
- **THEN** the UI MUST open the DAG view and focus that node

### Requirement: Human action requests render inline
Human-facing action requests SHALL be represented as addressable chat messages with optional inline choices rather than blocking the entire chat window.

#### Scenario: Clarification request is pending
- **WHEN** a role asks for clarification within a lane
- **THEN** the chat MUST show an inline pending action request in that lane
- **AND** the user MUST be able to continue sending other messages before responding

#### Scenario: User replies to action request
- **WHEN** the user replies to an action request
- **THEN** the system MUST emit the typed semantic event for that request kind
- **AND** the action request MUST be marked resolved
