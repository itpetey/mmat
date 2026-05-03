## ADDED Requirements

### Requirement: Pipeline composes typed phases with declared routes
The system SHALL provide a `Pipeline` orchestrator that composes typed `Phase`s via declared routes, supporting linear advancement, conditional branching, parallel fan-out, and cycles for backflow.

#### Scenario: Linear pipeline advances through phases
- **WHEN** a Pipeline declares `Route::Next(phase_id)` for a phase
- **THEN** the pipeline executor MUST run the next phase with the current phase's output as input

#### Scenario: Conditional branching routes to different phases
- **WHEN** a Pipeline declares `Route::Switch(fn)` for a phase
- **THEN** the switch function MUST receive the phase output and return the next phase ID
- **AND** the pipeline executor MUST route to the returned phase

#### Scenario: Parallel phases execute concurrently
- **WHEN** a Pipeline declares `Route::Parallel(phase_ids)` for a phase
- **THEN** all target phases MUST execute concurrently
- **AND** their outputs MUST be joined before the pipeline proceeds

#### Scenario: Cycle routes enable backflow
- **WHEN** a `Route::Switch` returns a phase ID that precedes the current phase in execution order
- **THEN** the pipeline MUST re-enter that phase with the current phase's output
- **AND** cycle depth MUST be tracked to prevent infinite loops

#### Scenario: Pipeline halts when no route is declared
- **WHEN** a phase has no declared route or the route returns `Route::Halt`
- **THEN** pipeline execution MUST terminate with that phase's output

### Requirement: Pipeline validates route type compatibility
The system SHALL validate that route targets accept compatible input types at Pipeline construction time.

#### Scenario: Incompatible route is rejected
- **WHEN** a `Route::Next` or `Route::Switch` targets a phase whose input type does not match the source phase's output type
- **THEN** Pipeline construction MUST fail with a type-mismatch error

#### Scenario: Compatible route is accepted
- **WHEN** a `Route::Next` targets a phase whose input type matches the source phase's output type
- **THEN** Pipeline construction MUST succeed

### Requirement: Phase wraps Step for retry semantics
The system SHALL allow a `Phase` implementation to internally wrap a NAAF `Step` for retry, validation, materialisation, and repair.

#### Scenario: Phase delegates to Step internally
- **WHEN** a Phase implementation wraps a Step
- **THEN** the Phase's `run` method MAY delegate to `Step::run_traced()` to benefit from retry/repair loops

#### Scenario: Phase operates without Step
- **WHEN** a Phase handles a deterministic operation (e.g., serialisation, materialisation)
- **THEN** the Phase MAY implement its own logic without wrapping a Step

### Requirement: Pipeline supports checkpointing for resume
The system SHALL support checkpointing Pipeline execution so that long-running pipelines can be paused and resumed after restart. Pipeline MUST checkpoint after each phase completes.

#### Scenario: Pipeline checkpoints after each phase
- **WHEN** a phase completes successfully
- **THEN** the Pipeline MUST save a checkpoint containing the current phase ID, phase output, and execution metadata

#### Scenario: Pipeline resumes from checkpoint
- **WHEN** a Pipeline is constructed with a checkpoint from a previous run
- **THEN** it MUST resume from the checkpointed phase rather than starting from the initial phase

#### Scenario: Pipeline supports both checkpointed and non-checkpointed execution
- **WHEN** a Pipeline is constructed without a checkpointer
- **THEN** it MUST run without checkpointing overhead; no checkpoints are saved or loaded

### Requirement: Step loses DAG composition methods
The system SHALL remove `.then()`, `.join()`, `.reconcile()`, `.reconcile_task()`, and `.zip()` from `Step`. Step MUST remain a pure unit of work with retry semantics.

#### Scenario: Step is used only as a unit of work after migration
- **WHEN** a consumer needs to compose multiple Steps
- **THEN** the consumer MUST use `Pipeline` for composition, not Step combinators

#### Scenario: Step retains intra-step combinators
- **WHEN** a consumer needs to transform a single Step's input or output
- **THEN** `.map()`, `.map_input()`, `.map_with_input()`, and `.map_findings()` MUST remain available

### Requirement: Workflow is removed from naaf_core
The system SHALL remove the `Workflow` primitive (`graph.rs`) and all related types from `naaf_core`. Pipeline MUST subsume all Workflow capabilities (dynamic scheduling, parallelism, checkpointing).

#### Scenario: Workflow types are no longer accessible
- **WHEN** a consumer imports `naaf_core`
- **THEN** `Workflow`, `WorkflowNode`, `StepNode`, `NodeSpec`, `NodeInput`, `NodeOutcome`, `GraphPatch`, `EdgeSpec`, and related checkpoint types MUST NOT be present

#### Scenario: Pipeline handles previously Workflow-driven use cases
- **WHEN** a consumer needs dynamic scheduling with parallel execution and checkpointing
- **THEN** Pipeline MUST be the sole mechanism for achieving this

### Requirement: Route::Parallel respects max depth
The system SHALL enforce a configurable maximum cycle depth per Pipeline execution.

#### Scenario: Pipeline halts at max depth
- **WHEN** a cycle has been traversed more than the configured maximum depth
- **THEN** the pipeline MUST halt with a `MaxDepthExceeded` error rather than looping indefinitely
