## ADDED Requirements

### Requirement: UiState captures workflow events
The system SHALL maintain a shared `UiState` struct that receives `FrontendEvent` entries from the tracing layer and stores them as `UiEvent` entries in a bounded event history.

#### Scenario: Log event is recorded in history
- **WHEN** a `FrontendEvent::Log` is received by the UiState receiver
- **THEN** a corresponding `UiEvent::Log` is appended to `event_history`
- **AND** the history is capped at 256 entries (oldest dropped when full)

#### Scenario: Step start event is recorded
- **WHEN** a `FrontendEvent::StepStarted` is received
- **THEN** a `UiEvent::StepStarted` is appended to `event_history`

#### Scenario: Step completion event is recorded
- **WHEN** a `FrontendEvent::StepCompleted` is received
- **THEN** a `UiEvent::StepCompleted` is appended to `event_history`

#### Scenario: Step failure event is recorded
- **WHEN** a `FrontendEvent::StepFailed` is received
- **THEN** a `UiEvent::StepFailed` is appended to `event_history`

### Requirement: UiState manages pending initial input
The system SHALL store an optional oneshot sender for the initial user prompt. When the LiveView UI submits the initial prompt, the sender SHALL be completed with the user's text.

#### Scenario: Initial input sender is stored
- **WHEN** `run_interactive()` starts with no `--prompt` flag
- **THEN** a oneshot sender is stored in `UiState.pending_initial_input`
- **AND** the LiveView UI renders an initial prompt input card

#### Scenario: User submits initial prompt
- **WHEN** the user types text and submits the initial prompt card
- **THEN** the stored oneshot sender is completed with the user's text
- **AND** `pending_initial_input` is cleared

### Requirement: UiState manages pending human prompts
The system SHALL store an optional pending human prompt containing a question, optional choices, and a oneshot reply sender. When the LiveView UI submits a reply, the sender SHALL be completed.

#### Scenario: Discovery clarification prompt is stored
- **WHEN** the workflow raises a `HumanQuestion` for discovery clarification
- **THEN** the question text is stored in `UiState.pending_prompt`
- **AND** the LiveView UI renders a prompt card with reply input

#### Scenario: Proposal approval prompt is stored
- **WHEN** the workflow raises a `HumanQuestion` for proposal approval
- **THEN** the question text is stored in `UiState.pending_prompt`
- **AND** the LiveView UI renders a prompt card with reply input

#### Scenario: Contract approval prompt is stored
- **WHEN** the workflow raises a `HumanQuestion` for contract approval
- **THEN** the question text is stored in `UiState.pending_prompt`
- **AND** the LiveView UI renders a prompt card with reply input

#### Scenario: User replies to a pending prompt
- **WHEN** the user types a reply and submits the prompt card
- **THEN** the stored oneshot sender is completed with the reply text
- **AND** `pending_prompt` is cleared

### Requirement: UiState tracks planning transition
The system SHALL set a `planning_started` flag in `UiState` when the planning step begins. The LiveView UI SHALL use this flag to switch from prompt-heavy rendering to log-stream rendering.

#### Scenario: Planning step sets the flag
- **WHEN** a `FrontendEvent::StepStarted` is received with task name `planning`
- **THEN** `UiState.planning_started` is set to `true`

#### Scenario: UI switches to log-stream mode
- **WHEN** `planning_started` is `true`
- **THEN** the LiveView UI renders all subsequent events as chronological log entries
- **AND** prompt cards are no longer the primary interaction mode

### Requirement: UiState maintains run summary snapshot
The system SHALL update a `RunSummary` snapshot in `UiState` whenever the workflow writes a run summary. The LiveView UI MAY use this for status display.

#### Scenario: Run summary is updated
- **WHEN** `write_run_summary()` is called by the workflow
- **THEN** the `RunSummary` in `UiState` is updated with the latest values
