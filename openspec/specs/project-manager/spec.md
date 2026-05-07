## ADDED Requirements

### Requirement: Project Manager decomposes work into task cards
The system SHALL provide a `ProjectManager` actor that receives ADRs and intent briefs, then produces a `DeliveryGraph` and set of `TaskCard`s. Each card MUST specify: what to build, dependencies on other cards, the contract (expected output type), the relevant ADR references, and acceptance criteria.

#### Scenario: PM produces a delivery graph
- **WHEN** the PM receives ADRs for a system with modules A, B, and C where B depends on A, and C depends on both
- **THEN** it MUST produce a `DeliveryGraph` with dependency edges A→B and A→C, B→C
- **AND** the graph MUST be published as an `ArtefactProduced` event

#### Scenario: Task cards reference their architectural context
- **WHEN** the PM creates a task card for implementing module A
- **THEN** the card MUST reference the ADR that defines module A's interface
- **AND** the card MUST specify the Ops Manager's validation policy to apply

### Requirement: PM sequences work respecting dependencies
The system SHALL ensure task cards are assigned in dependency order. A task with unsatisfied dependencies MUST NOT be assigned to a Worker.

#### Scenario: Dependent task waits for dependency
- **WHEN** task B depends on task A, and A is not yet complete
- **THEN** the PM MUST NOT publish a `TaskAssigned` event for B
- **AND** task B MUST remain in `Pending` state until A publishes `TaskCompleted`

### Requirement: PM manages scope and tracks progress
The system SHALL track the status of every task card (Pending, Assigned, Running, Completed, Failed). The PM MUST publish `Milestone` events when groups of related tasks complete.

#### Scenario: Milestone published on dependency group completion
- **WHEN** all tasks for the "data layer" module complete
- **THEN** the PM MUST publish a `Milestone` event
- **AND** dependents on the data layer MUST become eligible for assignment

### Requirement: PM escalates blockers
The system SHALL allow the PM to escalate when dependencies cannot be satisfied, scope cannot be met within constraints, or progress stalls.

#### Scenario: PM escalates stalled progress
- **WHEN** a task has been in `Assigned` state for longer than its time budget without completion
- **THEN** the PM MUST escalate to the Reviewer for status investigation
