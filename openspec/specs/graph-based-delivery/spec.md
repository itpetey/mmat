## ADDED Requirements

### Requirement: Delivery executes jobs in topological batches
The system SHALL sort sub-domain build jobs into dependency-ordered batches. All jobs within a batch MUST run in parallel. Each batch MUST complete before the next batch starts.

#### Scenario: Independent jobs execute concurrently
- **WHEN** two sub-domain build jobs have no dependency relationship
- **THEN** they MUST be placed in the same batch and execute in parallel with separate worktrees

#### Scenario: Dependent jobs wait for upstream completion
- **WHEN** sub-domain B depends on sub-domain A
- **THEN** B's build job MUST be placed in a batch after A's batch
- **AND** B's delivery MUST have access to A's build outputs (public knowledge groups, generated interfaces)

#### Scenario: Delivery graph rejects cycles
- **WHEN** a dependency graph contains a cycle (A → B → A)
- **THEN** the delivery scheduler MUST reject the graph with a cycle-detected error before any job execution

### Requirement: Each sub-domain delivery job is independent
The system SHALL treat each sub-domain build job as an independent unit with its own worktree, execution plan, and final review, using the existing delivery engine mechanics.

#### Scenario: Sub-domain job uses existing BuildEngine
- **WHEN** a sub-domain delivery job executes
- **THEN** it MUST reuse the existing `BuildEngine` implementation for planning, task execution, validation, and review
- **AND** the job's knowledge scope MUST be limited to the sub-domain's own knowledge groups plus public groups from its dependencies

### Requirement: Delivery graph tracks progress per batch
The system SHALL report delivery progress at the batch level, indicating which batch is currently executing and the status of each job within it.

#### Scenario: Batch progress is observable
- **WHEN** a delivery batch is executing
- **THEN** the system MUST report which batch is active and the status of each job (pending, running, succeeded, failed)
