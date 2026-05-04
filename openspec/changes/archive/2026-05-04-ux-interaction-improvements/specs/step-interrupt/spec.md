## ADDED Requirements

### Requirement: Escape key shows interrupt confirmation on first press
When a step is running and the user presses Escape, the system SHALL display a message "Esc again to interrupt" in the composer area. The system SHALL NOT interrupt the step on the first press.

#### Scenario: First Escape press during running step
- **WHEN** a step is running (`composer_mode == Working`) and the user presses Escape
- **THEN** the composer area MUST display "Esc again to interrupt"
- **AND** the running step MUST NOT be interrupted

#### Scenario: First Escape press when no step is running
- **WHEN** no step is running (`composer_mode != Working`) and the user presses Escape
- **THEN** no interrupt confirmation message MUST be displayed
- **AND** the Escape key MUST be handled by the browser's default behaviour

### Requirement: Second Escape press within 3 seconds interrupts the step
The system SHALL track the timestamp of the first Escape press. If the user presses Escape a second time within 3 seconds of the first press while a step is running, the system SHALL send an interrupt signal to cancel the running step.

#### Scenario: Second Escape press within 3 seconds
- **WHEN** the user presses Escape a second time within 3 seconds of the first press while a step is running
- **THEN** the system MUST send an `InterruptStep` signal to the backend
- **AND** the running step MUST terminate gracefully
- **AND** the composer MUST return to `Reply` mode
- **AND** the conversation MUST show a system entry indicating the step was interrupted

#### Scenario: Second Escape press after 3 seconds
- **WHEN** more than 3 seconds have elapsed since the first Escape press
- **THEN** the interrupt confirmation message ("Esc again to interrupt") MUST be cleared
- **AND** the next Escape press MUST be treated as a new first press

#### Scenario: Any key other than Escape clears the interrupt state
- **WHEN** the interrupt confirmation is showing ("Esc again to interrupt") and the user presses a key other than Escape
- **THEN** the interrupt confirmation message MUST be cleared
- **AND** the key MUST be handled normally (e.g., typed into the composer)

### Requirement: Cancellation token is checked between LLM turns
The running workflow SHALL poll a shared cancellation token between LLM turns. When the token is set, the current step MUST return a `StepInterrupted` result and exit cleanly.

#### Scenario: Cancellation token set between turns
- **WHEN** the cancellation token is set while a step is executing
- **THEN** the step MUST check the token before starting the next LLM turn
- **AND** if cancelled, the step MUST return a `StepInterrupted` result
- **AND** no further LLM calls MUST be made for that step

#### Scenario: Cancellation token not set
- **WHEN** the cancellation token is not set
- **THEN** the step MUST proceed normally without interruption