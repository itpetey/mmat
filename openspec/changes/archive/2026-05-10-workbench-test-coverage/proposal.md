## Why

The workbench is becoming a real runtime UI. It needs tests that cover API behaviour, projection replay, SSE, lane/action semantics, and browser-facing routes, not only unit tests for helper functions.

## What Changes

- Add HTTP/API integration tests for workbench routes.
- Add restart/resume tests using Postgres-backed event replay.
- Add tests for mentions, lanes, action requests, artefact loading, and DAG construction.
- Add smoke coverage for SSE and static assets.

## Capabilities

### New Capabilities
- `workbench-test-coverage`: Test coverage expectations for workbench runtime, API, frontend routes, and projections.

### Modified Capabilities

## Impact

- Affects test utilities, CI expectations, workbench API shape, and development workflow.
