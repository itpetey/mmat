## ADDED Requirements

### Requirement: Contracts type inter-role handoffs
The system SHALL define a `Contract<I, O>` struct that specifies the expected input type, expected output type, authority boundaries, completion criteria, and a unique `ContractId`. Contracts MUST be serializable and embedded in `TaskAssigned` events.

#### Scenario: TaskAssigned event carries a contract
- **WHEN** the coordinator publishes a `TaskAssigned` event
- **THEN** the event payload MUST include a serialized `Contract`
- **AND** the `Contract` MUST specify the expected input and output types
- **AND** the `ContractId` MUST be unique

#### Scenario: TaskCompleted event references its contract
- **WHEN** a role publishes a `TaskCompleted` event
- **THEN** it MUST reference the `ContractId` from the triggering `TaskAssigned` event
- **AND** the output MUST match the contract's output type

### Requirement: Authority boundaries are enforced in contracts
The system SHALL define `AuthorityScope` within contracts — a role MUST NOT produce output that exceeds its authority. For example, a Worker MUST NOT publish `DecisionRecorded` events (architecture decisions belong to the Architect).

#### Scenario: Worker within authority bounds
- **WHEN** a Worker's contract specifies `AuthorityScope::Implementation`
- **THEN** it MUST only publish `ClaimMade` and `ToolExecuted` events (not `DecisionRecorded`)

#### Scenario: Architect within authority bounds
- **WHEN** an Architect's contract specifies `AuthorityScope::Architecture`
- **THEN** it MUST be allowed to publish `DecisionRecorded` events (ADRs)

### Requirement: Completion criteria define task done-ness
The system SHALL define `CompletionCriteria` within contracts specifying when a task is considered complete. Criteria MUST support: all checks passed, artefact produced, human approved, or timeout. The scheduler MUST evaluate criteria against the event stream.

#### Scenario: Task completes when all checks pass
- **WHEN** a contract specifies `CompletionCriteria::AllChecksPassed`
- **AND** the associated reviewer publishes `ReviewCompleted { accepted: true }`
- **THEN** the scheduler MUST mark the task as complete

#### Scenario: Task fails on timeout
- **WHEN** a contract specifies a wall-clock timeout
- **AND** no `TaskCompleted` event is published within that duration
- **THEN** the scheduler MUST publish a `TaskFailed` event with reason "timeout"

### Requirement: Contract ID links all events in a task chain
The system SHALL ensure that every event in a task's execution chain (TaskAssigned → ClaimMade → ToolExecuted → ReviewCompleted → TaskCompleted) carries the same `ContractId`, enabling end-to-end traceability.

#### Scenario: All events in a task share the contract ID
- **WHEN** a task is executed
- **THEN** every event produced during that task MUST include the `ContractId` from the original `TaskAssigned` event
- **AND** the provenance engine MUST be able to trace all events for a task by `ContractId`
