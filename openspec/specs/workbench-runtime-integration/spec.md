# workbench-runtime-integration Specification

## Purpose
TBD - created by archiving change runtime-ui-integration. Update Purpose after archive.

## Requirements

### Requirement: Mentions emit role-appropriate semantic events
The workbench SHALL translate role mentions and inline actions into semantic events that match the target role's input contract.

#### Scenario: Scholar mention creates research task
- **WHEN** a chat message mentions `@scholar`
- **THEN** the workbench MUST publish `TaskAssigned` to `scholar-001` with a research-oriented contract

#### Scenario: Reviewer mention creates review request or guidance
- **WHEN** a chat message mentions `@reviewer` without a reviewable task or artefact
- **THEN** the workbench MUST NOT publish a generic `TaskAssigned` to Reviewer
- **AND** it MUST either ask for the target artefact/task or publish a valid `ReviewRequested` when context is available

### Requirement: Librarian runs as a visible memory service
The runtime-backed workbench SHALL start a Librarian service when memory processing is enabled and expose its activity in the UI.

#### Scenario: Memory proposal is accepted
- **WHEN** a role publishes `MemoryProposed` and the Librarian accepts it
- **THEN** the UI MUST show Librarian activity linked to the resulting `MemoryAccepted` event

#### Scenario: Memory proposal is rejected
- **WHEN** the Librarian rejects a memory proposal
- **THEN** the UI MUST show the rejection gate and reason without requiring a modal prompt

### Requirement: DAG state follows runtime task state
The workbench DAG SHALL derive task state from semantic task/review/escalation events and scheduler state.

#### Scenario: Task fails
- **WHEN** a `TaskFailed` event is published
- **THEN** the DAG step for that task MUST show failed state
- **AND** the detail panel MUST link to the failure event

#### Scenario: Review creates review step
- **WHEN** a `ReviewRequested` event is published
- **THEN** the DAG MUST include a review step linked to the reviewed task

### Requirement: Runtime auto-chaining is explicit
The workbench SHALL make role dispatches visible when one role automatically assigns work to another.

#### Scenario: Intent Lead dispatches Scholar
- **WHEN** Intent Lead publishes a `TaskAssigned` event to Scholar
- **THEN** the chat or DAG MUST show that handoff as a visible system event

### Requirement: Workbench publishes lane-scoped human input
The runtime-backed workbench SHALL publish human input as semantic events with the selected persisted lane in `EventContext.lane_id`.

#### Scenario: Mention in selected lane
- **WHEN** a chat message mentions `@scholar` while lane `lane-a` is selected
- **THEN** the workbench MUST persist and publish semantic events whose context includes `lane-a`
- **AND** projected runtime responses caused by that message SHOULD remain associated with `lane-a` when causally attributable

### Requirement: Project creation may create an initial lane
The workbench MAY create an initial persisted lane when a new project is created for ergonomic startup. The initial lane MUST be ordinary and archiveable.

#### Scenario: New project creates initial lane
- **WHEN** the user creates a new project through the workbench
- **THEN** the system MAY create an initial active lane for that project
- **AND** the lane MUST NOT be immutable or special beyond its creation timing

### Requirement: Workbench streams runtime assistant replies
The runtime-backed workbench SHALL dispatch accepted lane-scoped chat messages through a shared `OrganisationRuntime` boundary or coordinator-owned runtime facade and stream assistant replies over the workbench chat WebSocket. A successful assistant path MUST NOT emit `AssistantStreamUnavailable` and MUST NOT bypass the runtime boundary by calling the LLM client directly from the workbench chat API.

#### Scenario: Valid lane message streams an assistant reply
- **WHEN** the user submits a non-empty message to an active persisted lane and the runtime facade is configured
- **THEN** the server MUST acknowledge the user message with its persisted message ID
- **AND** the server MUST send an assistant stream start update with a stable assistant message ID, the selected lane ID, and the user message ID it replies to
- **AND** the server MUST forward assistant content deltas for that assistant message as they arrive
- **AND** the server MUST send a completion update only after the final assistant response has been durably persisted

#### Scenario: Workbench publishes input through runtime boundary
- **WHEN** the workbench accepts a lane-scoped user message
- **THEN** the message MUST be appended and published through the shared runtime boundary
- **AND** runtime subscribers MUST be able to observe the resulting `HumanFeedbackReceived` event on the runtime event bus
- **AND** the workbench MUST NOT rely on a separate local event bus as the authoritative integration point

#### Scenario: LLM runtime is not configured
- **WHEN** the user submits a valid lane message but the server has no usable runtime or LLM configuration
- **THEN** the server MUST report a recoverable chat error that identifies the missing configuration
- **AND** the server MUST NOT send `AssistantStreamUnavailable` as the normal response
- **AND** the already persisted user message MUST remain visible in the lane transcript

#### Scenario: LLM stream fails before completion
- **WHEN** an assistant stream starts and the LLM runtime returns an error before completion
- **THEN** the server MUST send an assistant stream failure update for the assistant message ID
- **AND** the server MUST NOT mark the assistant message complete
- **AND** the server MUST NOT persist an incomplete assistant response as a completed assistant message

### Requirement: Workbench cancels in-flight assistant streams
The workbench SHALL honour cancellation requests for in-flight assistant streams by stopping further runtime streaming for that assistant message through the runtime facade.

#### Scenario: User cancels active assistant stream
- **WHEN** the client sends a cancellation request for an assistant message that is still streaming
- **THEN** the server MUST stop forwarding additional deltas for that assistant message
- **AND** the server MUST send a cancellation acknowledgement
- **AND** the server MUST NOT persist the partial assistant content as a completed assistant message
