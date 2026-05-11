## ADDED Requirements

### Requirement: Workbench manages project and run identity
The workbench SHALL expose explicit project and run identity in API state and UI projections.

#### Scenario: Active project is visible
- **WHEN** the workbench loads
- **THEN** `/api/state` MUST include the active project ID, name, status, and active run ID when one exists

#### Scenario: New run is created
- **WHEN** the user starts a new delivery run from the UI
- **THEN** emitted events MUST carry the new run ID in event context

### Requirement: Workbench provides event history inspection
The workbench SHALL let users inspect semantic event history without exposing raw chain-of-thought as default content.

#### Scenario: Raw event is inspected
- **WHEN** the user selects an event in the event history view
- **THEN** the UI MUST show the event variant, source, timestamp, context IDs, summary, and raw JSON payload

#### Scenario: Event history is filtered
- **WHEN** the user filters by role, event type, run, task, or lane
- **THEN** the visible event history MUST include only matching events

### Requirement: Workbench supports safe project reset and archive controls
The workbench SHALL provide explicit controls for creating, archiving, and resetting project UI state.

#### Scenario: User archives a run
- **WHEN** the user archives a completed run
- **THEN** the run MUST disappear from the active work surface
- **AND** its events MUST remain available through history/replay

#### Scenario: Destructive reset requires confirmation
- **WHEN** the user requests destructive reset of a project or run
- **THEN** the UI MUST require confirmation before making changes

### Requirement: Workbench projects robust role and task states
The workbench SHALL distinguish idle, running, waiting, blocked, completed, failed, and escalated states for roles and DAG steps.

#### Scenario: Role fails
- **WHEN** a role task returns an error or emits failure state
- **THEN** the UI MUST mark the role as failed
- **AND** provide a link to relevant event details
