## ADDED Requirements

### Requirement: Workbench supports conversation lanes
The workbench SHALL support multiple conversation lanes per project. A lane MUST have an ID, name, kind, colour/accent, purpose, status, creator, and optional parent and related lane IDs.

#### Scenario: User creates a lane
- **WHEN** the user creates a lane named `Notification-thread UX`
- **THEN** the lane MUST appear in the lane list with its chosen or generated colour
- **AND** new messages posted while viewing that lane MUST carry that lane as their primary lane

#### Scenario: Lane is archived
- **WHEN** a lane is archived
- **THEN** it MUST disappear from active lane navigation
- **AND** its messages MUST remain visible in global/history views

### Requirement: LLM tools can create lanes
The system SHALL expose a lane-creation tool that roles or LLM workflows can call during conversation.

#### Scenario: Tool creates lane from message
- **WHEN** a role calls `create_lane` with a source message ID, name, kind, and purpose
- **THEN** the workbench MUST create the lane
- **AND** the source message MUST show a clickable lane chip for the new lane

#### Scenario: Tool-created lane records provenance
- **WHEN** a lane is created by a role/tool
- **THEN** the lane metadata MUST record the creating role/tool and source message ID

### Requirement: Messages and events carry lane metadata
Messages and projected events SHALL support a primary lane and zero or more related lanes.

#### Scenario: Global view shows lane tags
- **WHEN** the global chat view displays lane-tagged messages
- **THEN** each tagged message MUST show a coloured lane chip

#### Scenario: Single-lane view filters messages
- **WHEN** the user opens a single-lane view
- **THEN** the chat MUST show messages whose primary or related lane matches that lane
- **AND** unrelated delivery/runtime chatter MUST be hidden by default

#### Scenario: Multi-lane view combines related lanes
- **WHEN** the user selects multiple lanes
- **THEN** the chat MUST show messages from any selected lane in chronological order

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
- **WHEN** a role asks `@me` for clarification
- **THEN** the chat MUST show an inline pending action request in the appropriate lane
- **AND** the user MUST be able to continue sending other messages before responding

#### Scenario: User replies to action request
- **WHEN** the user replies to an action request
- **THEN** the system MUST emit the typed semantic event for that request kind
- **AND** the action request MUST be marked resolved
