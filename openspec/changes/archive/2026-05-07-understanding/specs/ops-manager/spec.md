## ADDED Requirements

### Requirement: Ops Manager owns organisational quality systems
The system SHALL provide an `OpsManager` actor implementing the `Role` trait. It MUST maintain a library of Standard Operating Procedures (SOPs), coding standards, review rubrics, validation policies, deployment standards, and escalation rules. These MUST be stored as durable memories with `MemoryType::SOP` and `MemoryScope::Organisational`.

#### Scenario: Ops Manager creates an SOP
- **WHEN** the Ops Manager receives a task to define a process for database migrations
- **THEN** it MUST publish a `MemoryProposed` event with type `SOP`, scope `Organisational`
- **AND** the memory content MUST describe the step-by-step procedure
- **AND** the SOP MUST include: when to apply it, preconditions, postconditions, and rollback steps

#### Scenario: Ops Manager defines a review rubric
- **WHEN** the Ops Manager receives a task to define code review standards
- **THEN** it MUST publish a `ReviewRubric` artefact
- **AND** the rubric MUST include explicit dimensions: correctness, API design, cohesion, coupling, backwards compatibility, observability, error handling, concurrency, performance, security, test adequacy, and migration safety

#### Scenario: Existing SOPs influence new ones
- **WHEN** the Ops Manager creates a new SOP
- **THEN** it MUST query the memory store for related existing SOPs
- **AND** the new SOP MUST reference or explicitly supersede existing ones

### Requirement: Ops Manager maintains procedural memory
The system SHALL define procedural memory as SOP-type memories with trigger conditions. When a role queries memory for a situation (e.g., "starting a database migration"), the retrieval planner MUST return applicable SOPs. The Ops Manager SHALL periodically review procedural memory for staleness.

#### Scenario: Procedural memory is queryable by trigger condition
- **WHEN** a Worker queries memory with "database migration procedure"
- **THEN** the retrieval planner MUST return SOPs tagged with database migration triggers
- **AND** the most recent, non-superseded SOP MUST be ranked first

#### Scenario: Ops Manager reviews stale SOPs
- **WHEN** the Ops Manager's periodic review loop runs (default: weekly)
- **THEN** it MUST query for SOPs approaching their decay date
- **AND** for each, it MUST either confirm the SOP is still valid (updating `last_accessed_at`) or propose a replacement

### Requirement: Ops Manager defines validation policies
The system SHALL define `ValidationPolicy` as a structured artefact specifying: which tools to run (e.g., `cargo fmt`, `cargo clippy`, `cargo test`), pass criteria (exit code, output patterns), and failure handling (retry, escalate, or reject). Validation policies MUST be stored as project-scoped memories.

#### Scenario: Validation policy for Rust projects
- **WHEN** the Ops Manager defines a validation policy for Rust projects
- **THEN** it MUST specify: `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, `cargo test`
- **AND** the pass criteria MUST include exit code 0 for each command
- **AND** the failure handling MUST specify: retry once, then escalate to Reviewer

#### Scenario: Validation policy varies by project type
- **WHEN** a project is a CLI tool vs a web service vs an embedded crate
- **THEN** the Ops Manager MUST define different validation expectations
- **AND** the Project Manager MUST select the appropriate policy for each task

### Requirement: Ops Manager continuously improves processes
The system SHALL allow the Ops Manager to self-improve by analysing review findings, task failures, and human feedback. The Ops Manager MUST have a research budget for investigating better processes.

#### Scenario: Ops Manager learns from review failures
- **WHEN** the Reviewer repeatedly finds the same class of issue (e.g., missing error handling)
- **THEN** the Ops Manager MUST propose an updated review rubric that explicitly checks for that issue
- **AND** the update MUST be published as a superseding SOP memory

#### Scenario: Ops Manager researches external best practices
- **WHEN** the Ops Manager's research budget allows
- **THEN** it MUST use web search to find current best practices for the project's technology stack
- **AND** propose SOP updates if external practices differ from current ones

### Requirement: Ops Manager defines escalation rules
The system SHALL define `EscalationRules` as a structured artefact mapping failure classes to escalation targets. The rules MUST cover: implementation defects, architectural conflicts, missing knowledge, ambiguous intent, and broken processes.

#### Scenario: Escalation rules are published to coordinator
- **WHEN** the Ops Manager finalises escalation rules
- **THEN** they MUST be published as `DecisionRecorded` events with `MemoryType::SOP`
- **AND** the coordinator MUST use them to configure role escalation paths
