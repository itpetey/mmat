## ADDED Requirements

### Requirement: Project list endpoint returns all known projects
The system SHALL provide a `GET /api/projects` endpoint that returns a JSON array of all known projects with their metadata.

#### Scenario: Projects are listed
- **WHEN** the client sends `GET /api/projects`
- **THEN** the response MUST be a JSON array of project objects, each containing at minimum `id`, `name`, `path`, `status`, and `run_count`
- **AND** the response status MUST be 200

#### Scenario: No projects exist
- **WHEN** the client sends `GET /api/projects` and no projects are known
- **THEN** the response MUST be an empty JSON array with status 200

### Requirement: Project detail endpoint returns single project
The system SHALL provide a `GET /api/projects/{id}` endpoint that returns metadata for a single project.

#### Scenario: Project exists
- **WHEN** the client sends `GET /api/projects/{id}` for a known project ID
- **THEN** the response MUST be a JSON object with the project's metadata
- **AND** the response status MUST be 200

#### Scenario: Project does not exist
- **WHEN** the client sends `GET /api/projects/{id}` for an unknown project ID
- **THEN** the response status MUST be 404

### Requirement: Project rename endpoint updates project name
The system SHALL provide a `PATCH /api/projects/{id}` endpoint that renames a project.

#### Scenario: Successful rename
- **WHEN** the client sends `PATCH /api/projects/{id}` with a valid `{ "name": "new-name" }` body
- **THEN** the project's directory on disk MUST be renamed to the new slugified name
- **AND** a `ProjectRenamed` event MUST be published to the event bus
- **AND** the response MUST be the updated project object with status 200

#### Scenario: Rename to existing name
- **WHEN** the client sends `PATCH /api/projects/{id}` with a name that already exists
- **THEN** the response status MUST be 409 with an error message

#### Scenario: Rename unknown project
- **WHEN** the client sends `PATCH /api/projects/{id}` for an unknown project ID
- **THEN** the response status MUST be 404

### Requirement: Project delete endpoint removes project
The system SHALL provide a `DELETE /api/projects/{id}` endpoint that permanently removes a project.

#### Scenario: Successful deletion
- **WHEN** the client sends `DELETE /api/projects/{id}` for a known project
- **THEN** the project's directory on disk MUST be removed
- **AND** a `ProjectDeleted` event MUST be published to the event bus
- **AND** the response MUST be a JSON confirmation with status 200

#### Scenario: Delete active project
- **WHEN** the client deletes the currently active project
- **THEN** the active project in the projection MUST be cleared
- **AND** the response status MUST be 200

#### Scenario: Delete unknown project
- **WHEN** the client sends `DELETE /api/projects/{id}` for an unknown project ID
- **THEN** the response status MUST be 404

### Requirement: Project selection endpoint activates a project
The system SHALL provide a `POST /api/projects/{id}/select` endpoint that switches the active project in the GUI.

#### Scenario: Successful selection
- **WHEN** the client sends `POST /api/projects/{id}/select` for a known project
- **THEN** the active project in the projection MUST be set to the selected project
- **AND** the event context for subsequent events MUST carry the selected project ID
- **AND** the response MUST return the updated project state with status 200

#### Scenario: Select unknown project
- **WHEN** the client sends `POST /api/projects/{id}/select` for an unknown project ID
- **THEN** the response status MUST be 404

### Requirement: Project creation validates uniqueness
The system SHALL reject project creation when a project with the same name already exists.

#### Scenario: Duplicate project name
- **WHEN** the client sends `POST /api/projects` with a name matching an existing project
- **THEN** the response status MUST be 409 with an error message

#### Scenario: Valid project creation
- **WHEN** the client sends `POST /api/projects` with a unique valid name
- **THEN** a `ProjectCreated` event MUST be published
- **AND** the response MUST include the new project object with status 201
