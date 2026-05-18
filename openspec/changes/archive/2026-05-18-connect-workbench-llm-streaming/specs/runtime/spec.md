## ADDED Requirements

### Requirement: Runtime exposes workbench integration handle
`OrganisationRuntime` SHALL expose or be wrapped by a long-lived workbench integration handle that allows the workbench server to publish lane-scoped input, subscribe to runtime events, start assistant streams, and cancel in-flight assistant streams without constructing a runtime per WebSocket connection.

#### Scenario: Workbench starts with shared runtime handle
- **WHEN** the workbench server starts in runtime-backed mode
- **THEN** it MUST initialise or receive one shared runtime integration handle for the server process
- **AND** WebSocket connections MUST reuse that handle rather than creating independent runtime instances
- **AND** runtime events published through the handle MUST be visible to runtime subscribers and workbench subscribers

#### Scenario: Workbench publishes durable input through runtime handle
- **WHEN** the workbench accepts a lane-scoped user message
- **THEN** the runtime handle MUST provide an operation that makes the event durable exactly once
- **AND** the event MUST be published to the runtime event bus after durability is established or through an acknowledgement path that proves durability before user acknowledgement
- **AND** the workbench MUST NOT manually append the event and also publish it to a persistence subscriber that appends it again

#### Scenario: Workbench starts assistant stream through runtime handle
- **WHEN** the workbench requests an assistant reply for a persisted lane message
- **THEN** the runtime handle MUST start the configured assistant streaming path
- **AND** stream deltas MUST be associated with a stable assistant message ID and the originating user message ID
- **AND** cancellation MUST stop further deltas for that assistant message
