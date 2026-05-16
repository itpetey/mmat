## Context

`mmat-event-stream` currently defines semantic events, an in-memory broadcast bus, and SQLx/Rusqlite persistence. The project is replacing direct database access with `mmat-db`, which is the canonical Diesel/Postgres database boundary. The current Dioxus workbench has a chat-only WebSocket and local transcript state, while the old workbench demonstrated that chat rows, lane lists, artefacts, notifications, and DAG state should be projections of semantic events.

Lanes are being refined from broad event tags into durable conversation branches: each persisted lane represents an isolated conversation thread for a project. Unscoped runtime/system events remain visible through a synthetic UI-only System lane.

## Goals / Non-Goals

**Goals:**
- Make `mmat-db` the only owner of event and lane persistence.
- Remove SQLx/Rusqlite event storage from `mmat-event-stream`.
- Keep `mmat-event-stream` responsible for event types and live pub/sub only.
- Add lane-aware event context so workbench projections can group events by one primary lane.
- Replace the workbench chat backend with command handling, persisted semantic events, live broadcasts, and replayable projections.
- Replace placeholder sidebar navigation with active lanes, a System lane, and archived lanes.

**Non-Goals:**
- Implement multi-lane tagging or global combined chat views.
- Implement full runtime/LLM streaming responses beyond event-backed projection plumbing.
- Add a persisted row for the synthetic System lane.
- Preserve the old workbench implementation structure.

## Decisions

1. `mmat-db` owns persistence.

   Event append/replay/query and lane CRUD will be Diesel functions in `mmat-db`. `mmat-event-stream` will not depend on SQLx, Rusqlite, Diesel, or database URLs. This keeps the database boundary singular and avoids competing schema creation paths.

2. `EventBus` is live-only.

   Publishing to the bus will broadcast an already-created `SemanticEvent`. Durable writes happen before broadcast in the workbench/runtime service layer. This makes persistence ordering explicit: append to Postgres, then notify subscribers.

3. Lanes are single-primary conversation branches.

   `EventContext` will carry `lane_id: Option<String>`. User conversation events and lane-scoped runtime events will set it. Unscoped events will use `None` and appear in the synthetic System lane. Multi-lane membership remains out of scope.

4. Persist lane current state separately from event history.

   `LaneCreated`, `LaneArchived`, and later lane lifecycle events remain in the event log, but a normalised `lanes` table stores current title/status/origin metadata for fast sidebar load and reliable reload behaviour. Archive state will not require full event replay.

5. The System lane is a projection, not data.

   The UI projection creates a System lane item for events with no lane ID. It cannot be archived, renamed, or selected as a destination for new human chat messages.

6. Workbench WebSocket messages become workbench commands and updates.

   The client sends commands such as `SendMessage`, `CreateLane`, and `ArchiveLane`. The server responds with snapshots, event/projection updates, and command acknowledgements. Chat rows are projected from events for the selected lane.

## Risks / Trade-offs

- Schema mismatch during SQLx removal -> Align `mmat-db` schema/models with the existing event payload shape before removing `EventStore` callers.
- Event append succeeds but broadcast fails or connection drops -> Treat Postgres as source of truth; reconnect snapshots replay/load from `mmat-db`.
- Synthetic System lane becomes a dumping ground -> Keep normal conversation commands lane-scoped and use System only for genuinely unscoped runtime/project events.
- Workbench projection grows into old-workbench complexity -> Keep projection types focused on sidebar lanes and transcript updates first; add graph/artefact projections incrementally.
- Existing tests depend on `EventBus::with_store` or `EventStore` -> Migrate tests to append through `mmat-db` where persistence is required and use live-only `EventBus` where persistence is irrelevant.

## Migration Plan

1. Add Diesel schema/models/functions for event rows and lane rows in `mmat-db`.
2. Add lane scope to `EventContext` and constructors/helpers.
3. Remove SQLx/Rusqlite persistence from `mmat-event-stream` and simplify `EventBus` to live-only broadcast/replay-free behaviour.
4. Add a workbench event service that appends via `mmat-db`, broadcasts via `EventBus`, and updates projections.
5. Replace chat WebSocket types with workbench command/update types while preserving the UI chat composer as a lane transcript input.
6. Replace placeholder sidebar nav with lane groups.

## Open Questions

None for the first pass. Future work can add lane rename, restored archived lanes, all-activity views, and richer artefact/DAG projections.
