## 1. Database Layer

- [x] 1.1 Align `mmat-db` event schema/models with canonical event IDs, row IDs, variant, JSON payload, timestamp, and source agent fields.
- [x] 1.2 Add `mmat-db` event append, replay, variant query, latest-row, row lookup, and event lookup functions.
- [x] 1.3 Add `mmat-db` lane schema/models for persisted lane metadata and active/archive status.
- [x] 1.4 Add `mmat-db` lane create, archive, list active, list archived, and lookup functions.

## 2. Event Stream Crate

- [x] 2.1 Add optional primary lane ID to `EventContext` with constructor/helper support.
- [x] 2.2 Update lane event fields/constructors to represent branch provenance.
- [x] 2.3 Remove SQLx/Rusqlite event persistence from `mmat-event-stream`.
- [x] 2.4 Simplify `EventBus` and tests to live-only pub/sub without store attachment.

## 3. Workbench Event Service

- [x] 3.1 Add server-side workbench event service that appends events through `mmat-db` and broadcasts through `EventBus`.
- [x] 3.2 Add workbench projection types for active lanes, archived lanes, System lane, and lane transcript items.
- [x] 3.3 Build projection snapshots from persisted lane rows and event replay.
- [x] 3.4 Project live events into lane transcripts and the synthetic System lane.

## 4. Workbench API And UI

- [x] 4.1 Replace chat-only WebSocket messages with workbench commands and server updates.
- [x] 4.2 Route user message submission to lane-scoped `HumanFeedbackReceived` events.
- [x] 4.3 Add lane creation and archive commands.
- [x] 4.4 Replace placeholder sidebar navigation with active lanes, System lane, archived lanes, and new-lane button.
- [x] 4.5 Render selected lane transcript from projection state, including blank lane state.

## 5. Tooling And Tests

- [x] 5.1 Update role lane tool publishing to include lane branch provenance.
- [x] 5.2 Add tests for `mmat-db` event CRUD and lane CRUD.
- [x] 5.3 Add tests for workbench projection reload, System lane projection, blank lane creation, and archived lane grouping.
- [x] 5.4 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and relevant tests.
