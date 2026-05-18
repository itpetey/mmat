## ADDED Requirements

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
