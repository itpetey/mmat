## ADDED Requirements

### Requirement: Non-code artefacts are stored in Postgres
The system SHALL store transient and structured non-code artefacts in Postgres artefact storage.

#### Scenario: Role produces ADR artefact
- **WHEN** Architect produces an ADR artefact
- **THEN** the artefact payload MUST be stored in Postgres
- **AND** the `ArtefactProduced` event MUST reference it with a database-backed storage URI

### Requirement: Generated code is written to project repository paths
The system SHALL write generated code to the project repository or an isolated project worktree, not to workbench-local blob directories.

#### Scenario: Worker produces code
- **WHEN** Worker completes an implementation task
- **THEN** changed code files MUST be present under the target project repository/worktree
- **AND** the output event MUST reference repository-relative paths and worktree metadata

### Requirement: Workbench renders both blob and code artefacts
The workbench SHALL inspect Postgres artefacts and repository code outputs through different renderers.

#### Scenario: User opens blob artefact
- **WHEN** the user opens a `db://artefacts/{id}` artefact
- **THEN** the UI MUST fetch and render the stored payload from Postgres

#### Scenario: User opens code output
- **WHEN** the user opens a code output artefact
- **THEN** the UI MUST show repository path, worktree, diff/patch summary, and validation evidence when available

### Requirement: Artefact provenance is preserved
Artefact views SHALL include producer role, content hash or repository revision metadata, and evidence refs.

#### Scenario: Artefact has evidence refs
- **WHEN** an artefact includes evidence references
- **THEN** the UI MUST link those references to event details
