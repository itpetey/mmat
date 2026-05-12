## ADDED Requirements

### Requirement: Workbench shows actionable next states
The UI SHALL show what the user can do next based on active lane, role state, pending action requests, and task status.

#### Scenario: No project activity yet
- **WHEN** a new project has no conversation history
- **THEN** the UI MUST show a clear starting prompt and suggested actions

#### Scenario: Role is running
- **WHEN** a role has active work
- **THEN** the UI MUST show a running indicator near the relevant lane/role/task

### Requirement: Workbench handles connection state
The UI SHALL display SSE connection, reconnecting, and stale-state status.

#### Scenario: Event stream disconnects
- **WHEN** the SSE connection drops
- **THEN** the UI MUST show a reconnecting state
- **AND** recover by reloading `/api/state` after reconnect

### Requirement: Workbench is accessible and responsive
The UI SHALL support keyboard navigation, visible focus states, labels for icon buttons, and responsive layouts.

#### Scenario: Keyboard user navigates actions
- **WHEN** a keyboard user tabs through the workbench
- **THEN** focus order MUST reach lane navigation, chat composer, notifications, view controls, and inline action buttons

#### Scenario: Mobile user opens DAG
- **WHEN** the viewport is narrow
- **THEN** DAG and detail panes MUST stack without horizontal overflow

### Requirement: Messages and artefacts render readable content
The UI SHALL render markdown/code-friendly message content while preserving access to raw event JSON and artefact payloads.

#### Scenario: Message contains code block
- **WHEN** a chat message contains fenced code
- **THEN** the UI MUST render it in a readable code block
- **AND** not execute embedded HTML or scripts

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
