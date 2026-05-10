# workbench-persistence Specification

## Purpose
TBD - created by archiving change postgres-only-workbench-persistence. Update Purpose after archive.
## Requirements
### Requirement: Workbench requires Postgres persistence
The workbench SHALL require a valid Postgres `DATABASE_URL` before starting runtime or HTTP services.

#### Scenario: Missing DATABASE_URL fails startup
- **WHEN** `mmat-workbench` starts without `DATABASE_URL`
- **THEN** startup MUST fail before binding the HTTP listener
- **AND** the error MUST explain that Postgres configuration is required

#### Scenario: Valid DATABASE_URL starts workbench
- **WHEN** `mmat-workbench` starts with a reachable Postgres `DATABASE_URL`
- **THEN** it MUST initialise event, memory, and artefact stores from Postgres
- **AND** it MUST bind the configured HTTP listener

### Requirement: Workbench hydrates projection from Postgres events
The workbench SHALL rebuild its UI projection by replaying persisted Postgres events during startup.

#### Scenario: Existing conversation resumes
- **WHEN** Postgres contains prior `HumanFeedbackReceived` and `TaskAssigned` events
- **THEN** `/api/state` MUST include the corresponding chat messages and DAG steps after startup

#### Scenario: Empty event store seeds first prompt
- **WHEN** Postgres contains no conversation events for the active project
- **THEN** the workbench MUST seed the initial project prompt once

### Requirement: Workbench does not write .mmat persistence
The workbench SHALL NOT create `.mmat/workbench`, `.mmat/workbench/events.db`, `.mmat/workbench/memory.db`, or `.mmat/artefacts` during normal operation.

#### Scenario: Running workbench leaves .mmat untouched
- **WHEN** a user starts the workbench and submits messages
- **THEN** no new `.mmat/workbench` SQLite files MUST be created
- **AND** events, memories, and artefacts MUST be persisted in Postgres

