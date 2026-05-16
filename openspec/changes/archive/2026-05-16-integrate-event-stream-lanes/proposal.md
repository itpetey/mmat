## Why

The workbench currently treats chat as the primary backend abstraction, while the rest of the system already emits semantic events for tasks, artefacts, memory, role state, and human interaction. Conversation side topics also need durable branch-like isolation so important tangents and to-do threads can be captured without polluting the active discussion.

## What Changes

- Replace chat-specific backend state with a workbench event stream processor that persists semantic events through `mmat-db`, broadcasts live events through `mmat-event-stream`, and projects lane-scoped UI state.
- **BREAKING** remove SQLx/Rusqlite event persistence from `mmat-event-stream`; all database persistence MUST be implemented in `mmat-db` using Diesel/Postgres.
- Add lane CRUD and event persistence functions to `mmat-db`, including active/archive lane state and row-ordered event replay.
- Treat lanes as durable conversation branches per project rather than generic event tags.
- Add a synthetic UI-only System lane for unscoped events.
- Replace the placeholder workbench navigation with active and archived lane groups.
- Allow the UI and LLM/tool workflows to create blank or forked lanes.

## Capabilities

### New Capabilities
- `workbench-event-stream`: Workbench command, event processing, projection, and streaming behaviour built on semantic events.

### Modified Capabilities
- `event-store`: Move durable event persistence from `mmat-event-stream` to `mmat-db` Diesel/Postgres functions.
- `event-bus`: Keep live pub/sub in `mmat-event-stream` while removing direct persistence ownership from the bus.
- `semantic-event-types`: Add lane scope to common event context and remove SQLite event-store requirements from semantic event definitions.
- `conversation-lanes`: Redefine lanes as durable single-primary-lane conversation branches with archived state and a synthetic System lane.
- `workbench-runtime-integration`: Route workbench human input and runtime output through semantic events and lane-scoped projections.
- `project-ui`: Replace placeholder sidebar navigation with lane navigation.

## Impact

- Affects `crates/event-stream`, `crates/db`, `crates/workbench`, and role lane tooling.
- Removes direct SQLx/Rusqlite dependencies from event persistence code paths.
- Adds Diesel schema/models/functions for events and lanes.
- Changes workbench WebSocket protocol from chat-only messages to workbench commands/snapshots/events.
- Requires projection tests for lane creation, archive grouping, unscoped System lane events, and event replay.
