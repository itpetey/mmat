## ADDED Requirements

### Requirement: Scheduler enforces wall-clock time budgets
The system SHALL track time elapsed since each `TaskAssigned` event. If a role does not publish `TaskCompleted` or `TaskFailed` within its allocated time budget, the scheduler MUST publish a `TaskFailed` event with reason "budget exceeded: timeout".

#### Scenario: Task completes within budget
- **WHEN** a Worker completes a task within its 5-minute budget
- **THEN** the scheduler MUST NOT intervene
- **AND** the `TaskCompleted` event MUST be processed normally

#### Scenario: Task exceeds time budget
- **WHEN** a Worker has not completed its task within the allocated time budget
- **THEN** the scheduler MUST publish `TaskFailed { reason: "budget exceeded: timeout" }`
- **AND** the coordinator MUST NOT assign new tasks to that Worker until the failure is handled

### Requirement: Scheduler enforces token consumption budgets
The system SHALL track token usage across all LLM calls made during a task. Token counts MUST be summed from `ToolExecuted` events that capture completion responses. If cumulative tokens exceed the budget, the scheduler MUST publish `TaskFailed` with reason "budget exceeded: tokens".

#### Scenario: Token budget warning at 80%
- **WHEN** cumulative token usage reaches 80% of the budget
- **THEN** the scheduler MUST publish a `BudgetWarning` event
- **AND** the role MAY use this to adjust its strategy

#### Scenario: Token budget exceeded
- **WHEN** cumulative token usage exceeds the budget
- **THEN** the scheduler MUST publish `TaskFailed { reason: "budget exceeded: tokens" }`

### Requirement: Scheduler enforces retry count limits
The system SHALL track retry counts per task (each retry increments a counter). When the retry count exceeds the contract's `max_retries`, the scheduler MUST escalate to the registered escalation target rather than retrying again.

#### Scenario: Retry within limit
- **WHEN** a task fails and the retry count is less than `max_retries`
- **THEN** the scheduler MUST republish a `TaskAssigned` event for the same contract
- **AND** the retry count MUST be incremented

#### Scenario: Retry limit exhausted
- **WHEN** a task fails and the retry count equals `max_retries`
- **THEN** the scheduler MUST NOT retry
- **AND** instead MUST escalate to the registered escalation target

### Requirement: Scheduler routes escalations by severity
The system SHALL process `EscalationRequested` events by looking up the escalation path registered for the source role and severity. The scheduler MUST then publish a `TaskAssigned` event targeting the escalation recipient with the escalation context.

#### Scenario: Moderate escalation from Worker goes to Reviewer
- **WHEN** a Worker publishes `EscalationRequested { severity: Moderate, reason: "test failures" }`
- **AND** the Worker's escalation path maps Moderate → Reviewer
- **THEN** the scheduler MUST publish `TaskAssigned { target: Reviewer, contract: review_contract }`

#### Scenario: Critical escalation goes to Intent Lead
- **WHEN** any role publishes `EscalationRequested { severity: Critical }`
- **AND** no role-specific path exists for Critical
- **THEN** the scheduler MUST escalate to the Intent Lead (the default critical handler)

#### Scenario: Escalation is published as an auditable event
- **WHEN** the scheduler processes an escalation
- **THEN** it MUST publish an `EscalationAccepted` event linking the source escalation request to the new task
- **AND** the event MUST include the escalation chain depth (to prevent infinite loops)

### Requirement: Scheduler tracks role lifecycle state
The system SHALL maintain the current lifecycle state for each role instance. States MUST follow the transition model: `Idle → Running → Completed | Failed | Escalated`. State changes MUST be published as events.

#### Scenario: Role transitions from Idle to Running on task assignment
- **WHEN** a `TaskAssigned` event targets a role that is currently `Idle`
- **THEN** the scheduler MUST transition the role state to `Running`
- **AND** publish a `RoleStateChanged` event

#### Scenario: Role transitions from Running to Completed
- **WHEN** a role publishes `TaskCompleted` while in `Running` state
- **THEN** the scheduler MUST transition to `Completed`
- **AND** the role is now eligible for new task assignments (back to `Idle`)
