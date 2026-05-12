## ADDED Requirements

### Requirement: ProjectListed event variant exists
The system SHALL define a `ProjectListed` variant in the `SemanticEvent` enum for tracking project discovery from filesystem scanning.

#### Scenario: ProjectListed event is published
- **WHEN** a project directory is discovered during startup filesystem scan
- **THEN** a `ProjectListed` event MUST be published containing the `project_id` and `path`
- **AND** the event MUST include a `source_agent` set to the system/coordinator role
- **AND** the event MUST include a `timestamp_ns` set at construction time

### Requirement: ProjectRenamed event variant exists
The system SHALL define a `ProjectRenamed` variant in the `SemanticEvent` enum for tracking project name changes.

#### Scenario: ProjectRenamed event is published
- **WHEN** a project is renamed via the API
- **THEN** a `ProjectRenamed` event MUST be published containing the `project_id`, `old_name`, and `new_name`
- **AND** the event MUST include a `source_agent` set to `human` (the user who initiated the rename)
- **AND** the event MUST be serializable to and deserializable from JSON

### Requirement: ProjectDeleted event variant exists
The system SHALL define a `ProjectDeleted` variant in the `SemanticEvent` enum for tracking project removal.

#### Scenario: ProjectDeleted event is published
- **WHEN** a project is deleted via the API
- **THEN** a `ProjectDeleted` event MUST be published containing the `project_id` and `name` of the deleted project
- **AND** the event MUST include a `source_agent` set to `human` (the user who initiated the deletion)
- **AND** the event MUST be durable and survive process restart via the event store

### Requirement: Project lifecycle events carry complete provenance
All project lifecycle events (`ProjectListed`, `ProjectRenamed`, `ProjectDeleted`) SHALL carry the same provenance fields as other `SemanticEvent` variants.

#### Scenario: Project event provenance
- **WHEN** any project lifecycle event is constructed
- **THEN** the event MUST include a unique `EventId`, `source_agent`, and `timestamp_ns`
- **AND** the `source_agent` MUST identify the originating role
- **AND** the `timestamp_ns` MUST be set at event construction time
