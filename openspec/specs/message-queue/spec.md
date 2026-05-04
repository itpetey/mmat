## ADDED Requirements

### Requirement: User messages can be queued while a step is running
When a step is running (`composer_mode == Working`), the composer SHALL accept text input from the user and allow submission. Submitted messages MUST be placed in a message queue rather than sent immediately to the LLM.

#### Scenario: Submitting a message during a running step
- **WHEN** a step is running and the user types text into the composer and submits it
- **THEN** the text MUST be appended to a per-project message queue (`VecDeque<String>`)
- **AND** the composer MUST clear its input field
- **AND** a queue indicator MUST be displayed showing the number of queued messages
- **AND** the LLM MUST NOT be called immediately

#### Scenario: Multiple messages queued
- **WHEN** a step is running and the user submits three separate messages
- **THEN** all three messages MUST be in the queue in submission order
- **AND** the queue indicator MUST show "3 messages queued"

#### Scenario: Composer is still interactive during a step
- **WHEN** a step is running
- **THEN** the composer MUST remain in `Working` mode with the ability to type and submit
- **AND** the queued messages MUST appear as conversation entries with a visual distinction (e.g., dimmed or marked "queued")

### Requirement: Queued messages are flushed as a single LLM turn on step completion
When a running step completes (LLM response finishes or tool call completes), the system SHALL flush all queued messages by concatenating them into a single user turn and sending it to the LLM.

#### Scenario: Queue flushed after step completes
- **WHEN** a step completes and there are queued messages
- **THEN** all messages in the queue MUST be concatenated with separator markers
- **AND** the concatenated text MUST be sent as a single user message to the LLM
- **AND** the queue MUST be emptied
- **AND** the queue indicator MUST be removed

#### Scenario: No queued messages on step completion
- **WHEN** a step completes and there are no queued messages
- **THEN** the system MUST proceed normally without sending any additional user message

#### Scenario: Messages queued during interruption
- **WHEN** a step is interrupted via the step-interrupt mechanism and there are queued messages
- **THEN** the queued messages MUST be preserved in the queue
- **AND** the queue MUST NOT be flushed (the user should decide whether to re-submit after interruption)

### Requirement: Queue state is visible in the UI snapshot
The `UiSnapshot` SHALL include the current message queue count so the frontend can render a queue indicator.

#### Scenario: Snapshot includes queue count
- **WHEN** `UiSnapshot` is produced while messages are queued
- **THEN** the snapshot MUST include a `message_queue_count: usize` field reflecting the number of queued messages
- **AND** the frontend MUST render this count as a queue indicator near the composer
