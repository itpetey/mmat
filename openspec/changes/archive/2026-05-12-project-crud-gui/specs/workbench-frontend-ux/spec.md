## ADDED Requirements

### Requirement: Workbench provides project navigation sidebar
The UI SHALL include a project sidebar that lists all known projects, highlights the active project, and provides actions to create, select, rename, and delete projects.

#### Scenario: Sidebar is visible on load
- **WHEN** the workbench loads
- **THEN** the project sidebar MUST be visible alongside the main workbench layout
- **AND** the sidebar MUST populate from the `projects` field of the state snapshot

#### Scenario: Active project is clearly indicated
- **WHEN** a project is the active project
- **THEN** the sidebar MUST visually distinguish the active project entry from inactive ones

#### Scenario: Sidebar handles empty state
- **WHEN** no projects are known
- **THEN** the sidebar MUST display a prompt to create the first project

### Requirement: Project management actions use confirmation for destructive operations
The UI SHALL require explicit user confirmation before performing destructive project operations (deletion, reset).

#### Scenario: Delete requires confirmation
- **WHEN** the user triggers a project deletion
- **THEN** the UI MUST show a confirmation prompt before sending the delete request
- **AND** the delete request MUST NOT be sent if the user cancels the prompt

#### Scenario: Rename does not require confirmation
- **WHEN** the user triggers a project rename
- **THEN** the UI MUST prompt for the new name but MUST NOT require a second confirmation step
