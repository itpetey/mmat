## ADDED Requirements

### Requirement: Workbench publishes lane-scoped human input
The runtime-backed workbench SHALL publish human input as semantic events with the selected persisted lane in `EventContext.lane_id`.

#### Scenario: Mention in selected lane
- **WHEN** a chat message mentions `@scholar` while lane `lane-a` is selected
- **THEN** the workbench MUST persist and publish semantic events whose context includes `lane-a`
- **AND** projected runtime responses caused by that message SHOULD remain associated with `lane-a` when causally attributable

### Requirement: Project creation may create an initial lane
The workbench MAY create an initial persisted lane when a new project is created for ergonomic startup. The initial lane MUST be ordinary and archiveable.

#### Scenario: New project creates initial lane
- **WHEN** the user creates a new project through the workbench
- **THEN** the system MAY create an initial active lane for that project
- **AND** the lane MUST NOT be immutable or special beyond its creation timing
