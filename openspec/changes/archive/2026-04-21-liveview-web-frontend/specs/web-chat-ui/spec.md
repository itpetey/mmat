## ADDED Requirements

### Requirement: Chat layout with scrollable transcript and bottom composer
The system SHALL render a full-height chat layout consisting of a scrollable transcript area above and a fixed composer input below.

#### Scenario: Initial page load shows empty transcript
- **WHEN** the browser first loads the LiveView page
- **THEN** the transcript area is empty or shows a welcome message
- **AND** the composer input is visible at the bottom

#### Scenario: Transcript scrolls as new events arrive
- **WHEN** new `UiEvent` entries are appended to `UiState.event_history`
- **THEN** the transcript area updates to show the new entries
- **AND** the view scrolls to the latest entry

### Requirement: Initial prompt input card
The system SHALL render an initial prompt card when no initial prompt was provided via CLI. The card SHALL contain a text input and a submit button.

#### Scenario: User sees initial prompt card on first load
- **WHEN** `UiState.pending_initial_input` is `Some`
- **THEN** a card with the text "What are we building?" is displayed
- **AND** a text input and submit button are available

#### Scenario: Submitting initial prompt clears the card
- **WHEN** the user types text and clicks submit
- **THEN** the initial prompt card is removed
- **AND** the submitted text appears as a user message in the transcript

### Requirement: Human prompt reply card
The system SHALL render a prompt card when a `HumanQuestion` is pending. The card SHALL display the question text and provide a reply input.

#### Scenario: Discovery clarification prompt is displayed
- **WHEN** `UiState.pending_prompt` contains a discovery clarification question
- **THEN** the question text is displayed in a prompt card
- **AND** a reply input and submit button are available

#### Scenario: Approval prompt is displayed
- **WHEN** `UiState.pending_prompt` contains an approval or contract question
- **THEN** the question text is displayed in a prompt card
- **AND** a reply input and submit button are available

#### Scenario: Submitting prompt reply clears the card
- **WHEN** the user types a reply and clicks submit
- **THEN** the prompt card is removed
- **AND** the reply appears as a user message in the transcript

### Requirement: Log entries render with level-appropriate styling
The system SHALL render log events in the transcript with visual distinction based on log level (info, warn, error).

#### Scenario: Info log renders as normal text
- **WHEN** a `UiEvent::Log` with level `info` is rendered
- **THEN** it appears as standard text in the transcript

#### Scenario: Warning log renders with warning styling
- **WHEN** a `UiEvent::Log` with level `warn` is rendered
- **THEN** it appears with a warning visual indicator (e.g. amber colour)

#### Scenario: Error log renders with error styling
- **WHEN** a `UiEvent::Log` with level `error` is rendered
- **THEN** it appears with an error visual indicator (e.g. red colour)

### Requirement: Step events render as status markers
The system SHALL render step lifecycle events as compact status markers in the transcript.

#### Scenario: Step start renders as a start marker
- **WHEN** a `UiEvent::StepStarted` is rendered
- **THEN** it appears as a compact marker showing the task name

#### Scenario: Step completion renders as a done marker
- **WHEN** a `UiEvent::StepCompleted` is rendered
- **THEN** it appears as a compact marker showing the task name and duration

#### Scenario: Step failure renders as an error marker
- **WHEN** a `UiEvent::StepFailed` is rendered
- **THEN** it appears as a compact marker showing the task name and error

### Requirement: Connection status indicator
The system SHALL display a small connection status indicator showing whether the LiveView websocket is connected.

#### Scenario: Connected status is shown
- **WHEN** the LiveView websocket is active
- **THEN** a green or neutral status indicator is visible

#### Scenario: Disconnected status is shown
- **WHEN** the LiveView websocket connection is lost
- **THEN** a red or warning status indicator is visible
