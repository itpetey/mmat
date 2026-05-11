## 1. Project And Run State

- [x] 1.1 Add active project and run fields to workbench projection and API state
- [x] 1.2 Populate project/run context on workbench-published events
- [x] 1.3 Add UI controls for creating a new run and viewing prior runs

## 2. Event Inspection

- [x] 2.1 Add event history panel with raw JSON inspection
- [x] 2.2 Add event filters for role, event type, run, task, and lane
- [x] 2.3 Link DAG steps and chat messages to event detail views

## 3. Operational Controls

- [x] 3.1 Add archive/reset API endpoints with confirmation semantics
- [x] 3.2 Add UI controls for archive/new-run/reset actions
- [x] 3.3 Improve role and DAG state derivation for blocked/failed/escalated states

## 4. Verification

- [x] 4.1 Add API tests for project/run state and event filters
- [x] 4.2 Add projection tests for failed and escalated states
- [x] 4.3 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
