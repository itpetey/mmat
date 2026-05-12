## ADDED Requirements

### Requirement: Project discovery scans host work directory
The system SHALL scan the host work directory (`MMAT_HOST_WORK_DIR`) at startup to discover existing project directories and register them as known projects.

#### Scenario: Directory contains valid projects
- **WHEN** the workbench starts and `MMAT_HOST_WORK_DIR` contains directories with valid project structures
- **THEN** a `ProjectListed` event MUST be published for each discovered directory
- **AND** each discovered project MUST appear in the `GET /api/projects` response

#### Scenario: Directory is empty
- **WHEN** the workbench starts and `MMAT_HOST_WORK_DIR` is empty
- **THEN** no `ProjectListed` events MUST be published
- **AND** `GET /api/projects` MUST return an empty array

#### Scenario: Directory contains non-project directories
- **WHEN** the workbench starts and `MMAT_HOST_WORK_DIR` contains directories that are not valid projects (e.g., no expected structure)
- **THEN** non-project directories MUST be ignored and no `ProjectListed` event published for them

### Requirement: Discovery deduplicates against known projects
The system SHALL skip project discovery for directories that already match a known project from past `ProjectCreated` events.

#### Scenario: Directory already known from creation event
- **WHEN** the workbench starts and a directory matches a project previously created via `POST /api/projects`
- **THEN** no duplicate `ProjectListed` event MUST be published for that directory
- **AND** the existing project entry MUST retain its event-derived metadata

### Requirement: Discovery validates project directories
The system SHALL validate that a discovered directory is a plausible project directory before publishing a `ProjectListed` event.

#### Scenario: Directory has no recognizable project structure
- **WHEN** the workbench scans a directory that contains only unrelated files
- **THEN** the directory MUST be skipped
- **AND** no `ProjectListed` event MUST be published for it

#### Scenario: Directory name contains only valid filesystem characters
- **WHEN** the workbench scans a directory with a name containing special characters or spaces
- **THEN** a `ProjectListed` event MAY be published using the sanitised directory name as the project name
