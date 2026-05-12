## ADDED Requirements

### Requirement: Workbench provides project navigation sidebar
The UI SHALL display a project sidebar listing all known projects, with the active project visually highlighted.

#### Scenario: Sidebar displays all projects
- **WHEN** the workbench loads and projects are known
- **THEN** the project sidebar MUST list every project from the `/api/state` `projects` field
- **AND** each project entry MUST show the project name

#### Scenario: Active project is highlighted
- **WHEN** a project is the currently active project
- **THEN** the project sidebar MUST visually distinguish the active project (e.g., bold text, accent colour, or active indicator)

#### Scenario: No projects exist
- **WHEN** no projects are known
- **THEN** the project sidebar MUST show a message indicating no projects and a button to create one

### Requirement: Workbench supports project switching from the sidebar
The UI SHALL allow the user to switch the active project by clicking a project in the sidebar.

#### Scenario: User clicks a different project
- **WHEN** the user clicks a non-active project in the sidebar
- **THEN** a `POST /api/projects/{id}/select` request MUST be sent
- **AND** the UI MUST update to reflect the newly active project

### Requirement: Workbench supports creating a new project from the sidebar
The UI SHALL provide a button or inline input in the sidebar to create a new project.

#### Scenario: User creates a new project
- **WHEN** the user triggers the new project creation flow from the sidebar
- **THEN** the UI MUST prompt for a project name
- **AND** on submission, send `POST /api/projects` with the name
- **AND** on success, the new project MUST appear in the sidebar and become the active project

### Requirement: Workbench supports renaming a project from the sidebar
The UI SHALL provide a rename action on project entries in the sidebar.

#### Scenario: User renames a project
- **WHEN** the user activates the rename action on a project in the sidebar
- **THEN** the UI MUST prompt for a new name
- **AND** on submission, send `PATCH /api/projects/{id}` with the new name
- **AND** the sidebar MUST update to reflect the new name

### Requirement: Workbench supports deleting a project from the sidebar with confirmation
The UI SHALL provide a delete action on project entries in the sidebar, requiring explicit confirmation.

#### Scenario: User deletes a project with confirmation
- **WHEN** the user activates the delete action on a project in the sidebar
- **THEN** the UI MUST show a confirmation dialog asking the user to confirm deletion
- **AND** on confirmation, send `DELETE /api/projects/{id}`
- **AND** the project MUST be removed from the sidebar

#### Scenario: User cancels deletion
- **WHEN** the user dismisses the confirmation dialog
- **THEN** no delete request MUST be sent
- **AND** the project MUST remain in the sidebar

### Requirement: Project sidebar is keyboard accessible
The project sidebar SHALL support keyboard navigation consistent with the existing accessibility patterns.

#### Scenario: Keyboard user navigates project sidebar
- **WHEN** a keyboard user tabs through the workbench
- **THEN** focus order MUST include the project sidebar entries
- **AND** pressing Enter on a project entry MUST activate/switch to that project
- **AND** rename and delete actions MUST be accessible via keyboard
