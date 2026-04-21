## MODIFIED Requirements

### Requirement: UiState manages pending initial input
The system SHALL store an optional oneshot sender for the initial user prompt while exposing a shared composer snapshot to the LiveView UI. When the initial prompt is submitted, the sender SHALL be completed with the user's text, the pending sender SHALL be cleared, and the submitted text SHALL be recorded as a visible user conversation entry.

#### Scenario: Initial input sender is stored
- **WHEN** `run_interactive()` starts with no `--prompt` flag
- **THEN** a oneshot sender is stored in `UiState.pending_initial_input`
- **AND** the LiveView UI can render the shared composer in initial-prompt mode

#### Scenario: User submits initial prompt
- **WHEN** the user types text and submits the initial prompt
- **THEN** the stored oneshot sender is completed with the user's text
- **AND** `pending_initial_input` is cleared
- **AND** the submitted text is appended to the visible conversation history

### Requirement: UiState manages pending human prompts
The system SHALL store an optional pending human prompt containing a question, optional choices, and a oneshot reply sender. When a pending prompt is created, the question SHALL also be available as an assistant-visible conversation entry. When the LiveView UI submits a reply, the sender SHALL be completed and the reply SHALL be recorded as a visible user conversation entry.

#### Scenario: Discovery clarification prompt is stored
- **WHEN** the workflow raises a `HumanQuestion` for discovery clarification
- **THEN** the question text is stored in `UiState.pending_prompt`
- **AND** the question is appended to the visible conversation history
- **AND** the LiveView UI renders the shared composer in reply mode

#### Scenario: Proposal approval prompt is stored
- **WHEN** the workflow raises a `HumanQuestion` for proposal approval
- **THEN** the question text is stored in `UiState.pending_prompt`
- **AND** the question is appended to the visible conversation history
- **AND** the available choices remain associated with the pending prompt

#### Scenario: Contract approval prompt is stored
- **WHEN** the workflow raises a `HumanQuestion` for contract approval
- **THEN** the question text is stored in `UiState.pending_prompt`
- **AND** the question is appended to the visible conversation history
- **AND** the available choices remain associated with the pending prompt

#### Scenario: User replies to a pending prompt
- **WHEN** the user types a reply and submits the prompt
- **THEN** the stored oneshot sender is completed with the reply text
- **AND** `pending_prompt` is cleared
- **AND** the submitted reply is appended to the visible conversation history

## ADDED Requirements

### Requirement: UiState stores conversation history separately from raw workflow logs
The system SHALL maintain conversation-oriented history alongside the bounded raw `UiEvent` history. Conversation history SHALL contain user submissions, assistant-visible prompt questions, and completed assistant-visible messages. Raw workflow logs SHALL remain available for inspection without being the sole source of transcript rendering.

#### Scenario: Raw workflow log is stored without becoming a chat turn
- **WHEN** a `FrontendEvent::Log` is received by the UiState receiver
- **THEN** a corresponding raw log entry is appended to the bounded raw event history
- **AND** no assistant-visible conversation message is appended unless the output is explicitly marked complete

#### Scenario: Completed assistant-visible output is stored once
- **WHEN** the workflow emits assistant-visible output that has reached a completed state
- **THEN** a single assistant conversation entry is appended to the conversation history
- **AND** partial token-level updates are not appended as separate conversation entries

## REMOVED Requirements

### Requirement: UiState tracks planning transition
**Reason**: The browser UI no longer switches into a log-stream-first mode once planning begins. Conversation history remains the primary surface throughout the run.

**Migration**: Render workflow progress through conversation/status entries and keep raw logs in the collapsed debug container instead of relying on `planning_started` to change UI modes.
