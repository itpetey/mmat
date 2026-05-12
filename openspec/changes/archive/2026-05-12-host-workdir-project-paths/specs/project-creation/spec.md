## ADDED Requirements

### Requirement: Project directory is created under the host work dir
When a project is created, the system SHALL create a directory under the host work directory with the project name as the directory name.

#### Scenario: Create project with valid name
- **WHEN** a project named `my-app` is created
- **AND** `host_work_dir` is `/workspace`
- **THEN** directory `/workspace/my-app` MUST be created
- **AND** a `ProjectCreated` event MUST be published with `project_id` set to `my-app`

#### Scenario: Project name contains invalid characters
- **WHEN** a project is created with a name containing path separators (`/`, `\`) or shell metacharacters
- **THEN** the request MUST be rejected with a validation error
- **AND** no directory SHALL be created

#### Scenario: Project name conflicts with existing directory
- **WHEN** a project is created with a name that already exists as a directory under the host work dir
- **THEN** the request MUST be rejected with a conflict error

#### Scenario: Create project without configured host work dir
- **WHEN** a project is created but `host_work_dir` is not set
- **THEN** the request MUST be rejected with an error indicating that the host work directory must be configured first

### Requirement: Workbench supports project creation via HTTP API
The workbench SHALL expose a `POST /api/projects` endpoint that accepts a project name and creates the corresponding directory.

#### Scenario: Successful project creation
- **WHEN** a `POST /api/projects` request is made with `{ "name": "my-app" }`
- **AND** the host work dir is configured and writable
- **THEN** the response MUST return `201 Created` with the project details
- **AND** the active project in the projection MUST be updated to `my-app`

#### Scenario: Invalid request body
- **WHEN** a `POST /api/projects` request is made with a missing or empty `name` field
- **THEN** the response MUST return `400 Bad Request` with a descriptive error

### Requirement: Project path is reflected in artefact output metadata
When the Worker publishes code output artefacts, the `RepositoryOutputRef::repository_path` SHALL reflect the resolved project directory path.

#### Scenario: Repository path in output reflects project path
- **WHEN** the Worker publishes a code output for a task in project `my-app`
- **AND** `host_work_dir` is `/workspace`
- **THEN** `RepositoryOutputRef::repository_path` MUST be `/workspace/my-app`
