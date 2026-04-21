## MODIFIED Requirements

### Requirement: Chat layout with scrollable transcript and bottom composer
The system SHALL render a full-height chat layout consisting of a scrollable conversation transcript area above a persistent composer. The same page SHALL also expose raw workflow logs in a collapsed container so the conversation remains the primary visible surface.

#### Scenario: Initial page load shows conversation-first layout
- **WHEN** the browser first loads the LiveView page
- **THEN** the main transcript area is reserved for conversation entries and status markers
- **AND** the composer is visible at the bottom
- **AND** the raw log container is present in a collapsed state

#### Scenario: Conversation transcript scrolls as new visible entries arrive
- **WHEN** a new user message, assistant-visible message, or status marker is appended to the UI snapshot
- **THEN** the conversation transcript updates to show the new entry
- **AND** the view scrolls to the latest visible entry

### Requirement: Initial prompt input card
The system SHALL render the initial prompt flow through the shared bottom composer when no initial prompt was provided via CLI. The composer SHALL remain mounted while the workflow is processing, and its submit button SHALL reflect whether input is currently accepted.

#### Scenario: User sees initial prompt composer on first load
- **WHEN** `UiState.pending_initial_input` is `Some`
- **THEN** the UI displays the prompt text "What are we building?"
- **AND** an enabled composer textarea and submit button are available
- **AND** the submit button is labelled for starting the run

#### Scenario: Submitting initial prompt clears the composer
- **WHEN** the user types text and submits the initial prompt
- **THEN** the submitted text appears as a user message in the conversation transcript
- **AND** the composer textarea is cleared
- **AND** the initial prompt sender is completed

#### Scenario: Workflow processing keeps the composer visible
- **WHEN** the initial prompt has been submitted and the workflow is processing without a pending human question
- **THEN** the composer remains visible in the layout
- **AND** the submit button shows a working state instead of replacing the textarea with placeholder-only content

### Requirement: Human prompt reply card
The system SHALL render pending human questions as assistant-visible conversation entries while using the shared composer for the reply. The reply flow SHALL support multiline drafting without hiding the main conversation transcript.

#### Scenario: Discovery clarification prompt is displayed
- **WHEN** `UiState.pending_prompt` contains a discovery clarification question
- **THEN** the question text is displayed as a visible conversation entry
- **AND** the composer is enabled for a reply
- **AND** the submit button is labelled for replying

#### Scenario: Approval prompt is displayed
- **WHEN** `UiState.pending_prompt` contains an approval or contract question
- **THEN** the question text is displayed as a visible conversation entry
- **AND** the composer is enabled for a reply
- **AND** the available choice actions remain accessible

#### Scenario: Submitting prompt reply clears the composer
- **WHEN** the user types a reply and submits the prompt
- **THEN** the submitted text appears as a user message in the conversation transcript
- **AND** the composer textarea is cleared
- **AND** the pending prompt sender is completed

#### Scenario: Multiline reply entry uses standard keyboard behaviour
- **WHEN** the user presses `Shift+Enter` while composing a reply
- **THEN** the composer inserts a newline instead of submitting the reply
- **AND** pressing `Enter` without `Shift` submits the reply when submission is enabled

### Requirement: Log entries render with level-appropriate styling
The system SHALL render raw log events inside the collapsed raw-log container with visual distinction based on log level (info, warn, error). Raw logs SHALL not replace the main conversation transcript as the primary surface.

#### Scenario: Raw logs are hidden by default
- **WHEN** the browser renders a page with one or more log events
- **THEN** the conversation transcript remains the primary visible content
- **AND** the raw logs are accessible through a collapsed disclosure container

#### Scenario: Expanded logs show level styling
- **WHEN** the user expands the raw-log container
- **THEN** info logs appear with standard styling
- **AND** warning logs appear with warning styling
- **AND** error logs appear with error styling

## ADDED Requirements

### Requirement: Assistant-visible conversation entries do not stream partial tokens
The system SHALL only append assistant-visible conversation entries when their content is stable enough to present as a complete message. Partial token or delta output SHALL remain out of the visible conversation transcript.

#### Scenario: Partial assistant output is not shown as a chat message
- **WHEN** the backend produces intermediate or token-level assistant output while a response is still in progress
- **THEN** the visible conversation transcript does not append partial assistant text
- **AND** the composer continues to show the current busy state until a stable message is available

#### Scenario: Completed assistant output appears once
- **WHEN** assistant-visible output reaches a completed state
- **THEN** the conversation transcript appends a single completed assistant message
- **AND** the message is rendered without prior token-by-token updates in the transcript
