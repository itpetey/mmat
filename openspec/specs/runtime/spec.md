# runtime Specification

## Purpose
TBD - created by archiving change coordinator. Update Purpose after archive.
## Requirements
### Requirement: Runtime boots the organisation
The system SHALL provide an `OrganisationRuntime` that is the single entry point for starting the organisation simulator. It MUST initialise the event bus, event store, memory store, and role registry in that order, then spawn all registered roles as concurrent tasks, then enter the main event loop.

#### Scenario: Runtime starts successfully
- **WHEN** `OrganisationRuntime::new(config).run().await` is called
- **THEN** the event bus and event store MUST be initialised
- **THEN** the memory store MUST be opened
- **THEN** all registered roles MUST be spawned
- **AND** an `OrganisationStarted` event MUST be published

#### Scenario: Runtime fails on missing dependency
- **WHEN** the event store cannot be opened (e.g., disk full)
- **THEN** the runtime MUST return an error without starting any roles
- **AND** the error MUST indicate which component failed

### Requirement: Runtime dispatches events to subscribing roles
The system SHALL maintain a dispatch table mapping event types to the roles that subscribe to them. When an event is published, the runtime MUST ensure every subscribing role receives it via the bus.

#### Scenario: Event reaches all subscribers
- **WHEN** a `TaskAssigned` event targeting Worker-1 is published
- **THEN** Worker-1 MUST receive the event
- **AND** the Reviewer, Auditor, and other roles that subscribe to `TaskAssigned` MUST also receive it
- **AND** roles that do NOT subscribe to `TaskAssigned` MUST NOT receive it

### Requirement: Runtime tracks organisation-level lifecycle
The system SHALL publish `OrganisationStarted` at boot, `OrganisationStopped` at graceful shutdown, and periodic `Heartbeat` events (configurable interval) while running. The heartbeat MUST include counts of active roles, completed tasks, and failed tasks.

#### Scenario: Heartbeat is published periodically
- **WHEN** the organisation is running
- **THEN** a `Heartbeat` event MUST be published at the configured interval (default: 30 seconds)
- **AND** the heartbeat MUST include the count of roles in each lifecycle state

#### Scenario: Graceful shutdown publishes stopped event
- **WHEN** the runtime receives a shutdown signal (SIGTERM or Ctrl+C)
- **THEN** it MUST publish `OrganisationStopped`
- **AND** flush the event store
- **AND** wait for all roles to complete their current task (with a configurable grace period)
- **AND** then exit

### Requirement: Runtime supports restart from event store
The system SHALL support restarting the organisation from the event store. On startup, the runtime MUST replay events to rebuild the scheduler's role state and the retrieval planner's context.

#### Scenario: Restart recovers role states
- **WHEN** the runtime restarts after a crash
- **AND** the event store contains prior `RoleStateChanged` events
- **THEN** the scheduler MUST rebuild role states from replayed events
- **AND** roles that were `Running` at crash time MUST be restarted
- **AND** roles that were `Completed` at crash time MUST NOT be restarted

#### Scenario: Restart with empty event store
- **WHEN** the runtime starts for the first time with an empty event store
- **THEN** all roles MUST start in `Idle` state
- **AND** no tasks MUST be in flight

### Requirement: Runtime supports Postgres-only workbench mode
The runtime SHALL expose configuration suitable for workbench usage where Postgres-backed event, memory, and artefact stores are mandatory.

#### Scenario: Workbench mode rejects SQLite store paths
- **WHEN** the workbench constructs runtime configuration
- **THEN** `database_url` MUST be populated
- **AND** legacy SQLite event or memory store paths MUST NOT be used

#### Scenario: Store initialisation failure stops runtime creation
- **WHEN** Postgres event, memory, or artefact store initialisation fails
- **THEN** runtime construction MUST return an error
- **AND** no roles MUST be spawned

