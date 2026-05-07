## ADDED Requirements

### Requirement: Worker executes bounded implementation in isolation
The system SHALL provide a `Worker` actor that, on receiving a `TaskAssigned` event with a `TaskCard`, creates an isolated worktree, implements the specified scope, and publishes evidence. Workers MUST be stateless — each task invocation uses a fresh LLM context and scoped tools.

#### Scenario: Worker implements a task card
- **WHEN** a Worker receives a `TaskAssigned` event with a task card to "add error handling to module X"
- **THEN** it MUST create an isolated worktree from the current repository
- **AND** use LLM with file reading/editing tools scoped to that worktree
- **AND** run the validation commands specified by the Ops Manager's policy
- **AND** publish `ClaimMade` events for results with `ToolExecuted` evidence

#### Scenario: Worker publishes implementation artefacts
- **WHEN** the Worker completes implementation
- **THEN** it MUST publish an `ArtefactProduced` event with the diff/patch
- **AND** publish a `TaskCompleted` event referencing the task's `ContractId`

#### Scenario: Worker does not retain state between tasks
- **WHEN** a Worker completes one task and receives another
- **THEN** it MUST start with a fresh LLM conversation context
- **AND** it MUST NOT retain any state from the previous task

### Requirement: Worker tools are scoped to the worktree
The system SHALL ensure the Worker's tool set (file read, file write, shell commands) operates within the isolated worktree. Tools MUST NOT access files outside the worktree except for read-only access to the original repository for reference.

#### Scenario: Worker cannot modify files outside worktree
- **WHEN** a Worker's tool registry is constructed for a task
- **THEN** file write tools MUST be scoped to the worktree path
- **AND** attempting to write outside the worktree MUST be rejected

#### Scenario: Worker can read original repo for reference
- **WHEN** the Worker needs to understand existing code
- **THEN** file read tools MUST have read-only access to the original repository
- **AND** changes MUST only be made in the worktree

### Requirement: Worker emits tool execution evidence
The system SHALL ensure every tool invocation by the Worker is published as a `ToolExecuted` event with stdout, stderr, and exit code. Claims made by the Worker MUST reference these tool execution events as evidence.

#### Scenario: Cargo test results are evidenced
- **WHEN** the Worker runs `cargo test` as part of validation
- **THEN** a `ToolExecuted` event MUST be published with the command output
- **AND** any `ClaimMade` about test results MUST reference the `ToolExecuted` event ID

#### Scenario: Claim without evidence is detected by Auditor
- **WHEN** the Worker publishes `ClaimMade { claim: "all tests pass" }` without referencing a `ToolExecuted` event
- **THEN** the Auditor MUST flag this as an unsubstantiated claim
