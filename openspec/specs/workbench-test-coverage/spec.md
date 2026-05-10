# workbench-test-coverage Specification

## Purpose
TBD - created by archiving change workbench-test-coverage. Update Purpose after archive.
## Requirements
### Requirement: Workbench API routes are integration-tested
The test suite SHALL cover state, message posting, notification/action acknowledgement, and static routes.

#### Scenario: Message is posted
- **WHEN** a test posts a valid message to `/api/messages`
- **THEN** the route MUST return success
- **AND** the resulting state MUST include the corresponding human message event/projection

#### Scenario: Empty message is rejected
- **WHEN** a test posts an empty message to `/api/messages`
- **THEN** the route MUST return a client error

### Requirement: Workbench replay is tested against Postgres
The test suite SHALL verify that persisted events replay into the projection after restart.

#### Scenario: Restart resumes state
- **WHEN** events are inserted into a temporary Postgres event store
- **AND** the workbench projection is rebuilt
- **THEN** `/api/state` MUST include messages, DAG steps, memories, and artefacts derived from those events

### Requirement: SSE behaviour is smoke-tested
The test suite SHALL include bounded SSE tests for initial state and live event delivery.

#### Scenario: SSE sends initial state
- **WHEN** a client connects to `/events`
- **THEN** the first received update MUST contain current workbench state

#### Scenario: SSE sends live event
- **WHEN** an event is published after a client connects
- **THEN** the client MUST receive an event update within a bounded timeout

### Requirement: UI projection semantics are covered
The test suite SHALL cover mention routing, lane filtering, action request resolution, artefact loading, and DAG event projection.

#### Scenario: Lane filter excludes unrelated delivery events
- **WHEN** a discovery lane view is projected
- **THEN** unrelated delivery lane events MUST NOT appear by default

#### Scenario: Artefact load failure is represented
- **WHEN** an artefact reference cannot be loaded
- **THEN** the projection MUST include an error state rather than panicking

