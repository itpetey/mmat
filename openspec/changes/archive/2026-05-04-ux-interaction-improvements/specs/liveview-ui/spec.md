## MODIFIED Requirements

### Requirement: UI remains compatible with existing single-project flow
The system SHALL preserve all existing UI functionality for projects that do not use domain-mapped planning. The project switcher, queue panel, raw logs disclosure, and conversation rendering MUST work identically to the current implementation when no domain tree is present. Additionally, the composer MUST support message queuing during running steps, and the Escape key MUST provide step interruption as defined in the `step-interrupt` and `message-queue` capabilities.

#### Scenario: Existing conversation flow works unchanged
- **WHEN** a project has no domain tree
- **THEN** the user MUST be able to type an initial prompt, interact with discovery questions, select solution branches, and view streaming LLM output exactly as before

#### Scenario: Delivery queue panel remains functional
- **WHEN** a project has no domain tree
- **THEN** the `QueuePanel` component MUST render at its existing position in the single-column layout

#### Scenario: Composer accepts input during running step
- **WHEN** a step is running and the user types into the composer and submits
- **THEN** the message MUST be queued (as per message-queue capability)
- **AND** a queue indicator MUST appear showing the number of queued messages

#### Scenario: Escape key provides step interruption
- **WHEN** a step is running and the user presses Escape twice within 3 seconds
- **THEN** the step MUST be interrupted (as per step-interrupt capability)

## ADDED Requirements

### Requirement: First boot shows project creation form instead of default project
When the application starts and the project registry is empty, the UI SHALL display a project creation form instead of the default project conversation view. The form MUST collect a project name and root directory, and on submission create the project before showing the conversation composer.

#### Scenario: Empty registry on first boot
- **WHEN** the application starts and the project registry has no projects
- **THEN** the UI MUST render a `NewProjectForm` component instead of the composer
- **AND** the `NewProjectForm` MUST contain fields for project name and root directory
- **AND** the form MUST have a "Create Project" submit button
- **AND** the `ensure_default_project` logic MUST NOT run

#### Scenario: Project creation form submitted
- **WHEN** the user fills in the project name and root directory and clicks "Create Project"
- **THEN** the system MUST call `UiState::register_project` with the submitted values
- **AND** on success, the form MUST be replaced by the normal conversation composer
- **AND** on failure, the form MUST display the error message

#### Scenario: Registry already has projects on startup
- **WHEN** the application starts and the project registry already has one or more projects
- **THEN** the UI MUST render the normal conversation composer directly
- **AND** the project creation form MUST NOT be shown

#### Scenario: Project creation form validates input
- **WHEN** the user submits the form with an empty project name
- **THEN** the form MUST display a validation error and MUST NOT create the project

### Requirement: International English directive in all system prompts
All LLM system prompts in the planning and delivery stages SHALL include an International English directive. The directive MUST specify Oxford spelling conventions (e.g., `organise`, `realisation`, `colour`, `favour`, `metre`), avoid American spellings, and instruct the model to use International English exclusively.

#### Scenario: Discovery stage system prompt includes directive
- **WHEN** the discovery stage builds its system prompt
- **THEN** the system prompt MUST include the International English directive

#### Scenario: Solutions stage system prompt includes directive
- **WHEN** the solutions stage builds its system prompt
- **THEN** the system prompt MUST include the International English directive

#### Scenario: Knowledge stage system prompt includes directive
- **WHEN** the knowledge stage builds its system prompt
- **THEN** the system prompt MUST include the International English directive

#### Scenario: Architect stage system prompt includes directive
- **WHEN** the architect stage builds its system prompt
- **THEN** the system prompt MUST include the International English directive

#### Scenario: Delivery engine system prompts include directive
- **WHEN** the delivery engine builds any system prompt (planning, implementation, peer review, contract validation, final review)
- **THEN** each system prompt MUST include the International English directive