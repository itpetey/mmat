## MODIFIED Requirements

### Requirement: Workbench manages project and run identity
The workbench SHALL expose explicit project and run identity in API state and UI projections. The workbench SHALL maintain a list of all known projects alongside the active project.

#### Scenario: Active project is visible
- **WHEN** the workbench loads
- **THEN** `/api/state` MUST include the active project ID, name, status, and active run ID when one exists
- **AND** `/api/state` MUST include a `projects` array containing all known projects with at minimum `id`, `name`, `path`, `status`, and `run_count`

#### Scenario: New run is created
- **WHEN** the user starts a new delivery run from the UI
- **THEN** emitted events MUST carry the new run ID in event context

### Requirement: Workbench supports safe project reset and archive controls
The workbench SHALL provide explicit controls for creating, listing, renaming, deleting, archiving, and resetting project UI state.

#### Scenario: User archives a run
- **WHEN** the user archives a completed run
- **THEN** the run MUST disappear from the active work surface
- **AND** its events MUST remain available through history/replay

#### Scenario: Destructive reset requires confirmation
- **WHEN** the user requests destructive reset of a project or run
- **THEN** the UI MUST require confirmation before making changes

#### Scenario: User deletes a project
- **WHEN** the user requests deletion of a project
- **THEN** the API MUST require explicit confirmation (via the request itself or a preceding confirmation step)
- **AND** the project directory MUST be removed from disk
- **AND** the project MUST be removed from the known projects list

## ADDED Requirements

### Requirement: Workbench supports project listing and switching
The workbench SHALL provide API endpoints for listing all known projects and switching the active project.

#### Scenario: Projects are listed via API
- **WHEN** the client sends `GET /api/projects`
- **THEN** the response MUST include all known projects with their metadata

#### Scenario: Active project is switched
- **WHEN** the client sends `POST /api/projects/{id}/select` for a known project
- **THEN** the active project in the projection MUST change to the selected project
- **AND** subsequent events MUST carry the new project ID in their context

#### Scenario: Active project is cleared
- **WHEN** the active project is deleted
- **THEN** the active project in the projection MUST be cleared
- **AND** `/api/state` MUST reflect no active project
