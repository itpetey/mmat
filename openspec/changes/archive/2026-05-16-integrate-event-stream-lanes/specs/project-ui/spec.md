## ADDED Requirements

### Requirement: Sidebar displays lane navigation
The workbench sidebar SHALL display conversation lane navigation in place of placeholder static navigation. Active persisted lanes MUST appear in a primary group, the synthetic System lane MUST be available for unscoped events, and archived lanes MUST appear in a secondary archived group.

#### Scenario: Active lanes are shown
- **WHEN** the workbench loads persisted active lanes for the current project
- **THEN** the sidebar MUST list those lanes in the primary lane group
- **AND** selecting a lane MUST show that lane's transcript/projection

#### Scenario: Archived lanes are shown separately
- **WHEN** the current project has archived lanes
- **THEN** the sidebar MUST show them in a secondary archived group
- **AND** selecting an archived lane MUST show its persisted transcript without making it active

#### Scenario: System lane is shown
- **WHEN** the workbench displays lane navigation
- **THEN** the sidebar MUST include a System lane affordance for unscoped events
- **AND** the System lane MUST be visually distinct from persisted user/LLM-created lanes

### Requirement: Sidebar can create blank lanes
The workbench sidebar SHALL provide a lane creation affordance that creates an empty persisted lane for the current project.

#### Scenario: User creates blank lane
- **WHEN** the user activates the new lane button and provides a title
- **THEN** the workbench MUST persist a new active lane
- **AND** the main pane MUST show a blank transcript for that lane
