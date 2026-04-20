# Implementation Backlog

This backlog captures the agreed outcomes from `RECOMMENDATION.md`, `EVALUATION.md`, and a direct review of the current MMAT and NAAF codebases.

## Source Material

- `RECOMMENDATION.md`
- `EVALUATION.md`
- `README.md`
- `src/workflow.rs`
- `src/models.rs`
- `src/prompts.rs`
- `../naaf/main/crates/core/src/graph.rs`
- `../naaf/main/crates/core/src/step.rs`
- `../naaf/main/crates/persistence-fs/src/lib.rs`
- `../naaf/main/crates/knowledge/src/lib.rs`

## Current Assessment

MMAT is already a disciplined staged workflow, but it is still centred on LLM stages rather than durable project artifacts. The main missing capabilities are:

- immutable contract memory
- durable on-disk run artifacts
- evidence-carrying task execution
- explicit claimed-vs-proven validation
- resumable runs wired into MMAT itself
- non-interactive execution outside the TUI

NAAF is a good substrate and should be evolved, not replaced. It already has reusable persistence and resume primitives that MMAT is not yet using.

## Objections To The External Evaluation

These points should shape implementation sequencing:

1. NAAF already has resumable workflow machinery.
   `naaf-core` already supports workflow checkpointing, runner registries, step persistence, and resume. The gap is adoption in MMAT, not a greenfield persistence design.

2. NAAF already has a first-class knowledge subsystem.
   The next step is compiling knowledge into project artifacts, not building a generic retrieval layer from scratch.

3. The first refactor should happen in MMAT, not NAAF.
   We should prove the artifact model in the application before extracting stable abstractions into the framework.

## Delivery Principles

- Centre the system on durable artifacts, not prompt-stage summaries.
- Preserve user intent through immutable contract artifacts and explicit change requests.
- Require executable evidence before a task or release can be marked complete.
- Prefer fewer solution branches by default.
- Keep the TUI as a view over the run, not the only embodiment of the run.

## Target Artifact Set

Each run should eventually persist these artifacts under a run directory:

- `intent-brief.json`
- `project-contract.json`
- `execution-plan.json`
- `task-cards/*.json`
- `task-results/*.json`
- `decision-log.json`
- `evidence-log.json`
- `release-assessment.json`
- `run-summary.json`

Recommended run root:

- `.mmat/runs/<run-id>/`

## Recommended Sequencing

1. Build the MMAT artifact model and run directory.
2. Refactor clarification into bounded intent capture.
3. Add an explicit immutable contract phase.
4. Refactor planning and execution around task cards and task results.
5. Add a second validation lane for contract and behavioural proof.
6. Wire MMAT into NAAF checkpoint and resume support.
7. Add CLI and non-interactive execution.
8. Compile repository, project, and external knowledge into artifacts.
9. Extract proven generic concepts back into NAAF only after MMAT stabilises.

## Milestone 1: Run Directory And Artifact Persistence

### Outcome

Every MMAT run is recorded on disk as structured artifacts, independent of the TUI.

### Tasks

1. Add artifact model types for run-level records.
2. Add a run store responsible for directory creation and JSON serialisation.
3. Add a run identifier and root path to runtime state.
4. Persist the major stage outputs as soon as each stage completes.
5. Add tests for artifact round-tripping and run directory layout.

### Likely MMAT Files

- `src/models.rs`
- `src/runtime.rs`
- `src/workflow.rs`
- new `src/artifacts.rs`
- new `src/run_store.rs`
- `src/main.rs`

### Notes

- Start with simple JSON-on-disk persistence in MMAT.
- Do not block on NAAF extraction in this milestone.

### Validation

- unit tests for serialisation and file layout
- `cargo test`

## Milestone 2: Intent Brief And Bounded Clarification

### Outcome

Discovery becomes a bounded clarification stage that records questions, assumptions, and a best-guess spec instead of looping indefinitely.

### Tasks

1. Replace `DiscoveryBrief` with a richer `IntentBrief`.
2. Add explicit fields for:
   - goals
   - non-goals
   - constraints
   - assumptions
   - ambiguities
   - risks
   - acceptance criteria
   - default assumptions
   - ranked clarification questions
3. Add a clarification budget, such as a maximum pass count.
4. Proceed under recorded defaults when the budget is exhausted.
5. Persist each clarification result to the run directory.

### Likely MMAT Files

- `src/models.rs`
- `src/prompts.rs`
- `src/workflow.rs`

### Validation

- workflow tests for capped clarification loops
- tests proving defaults are recorded and reused
- `cargo test`

## Milestone 3: Project Contract And Change Requests

### Outcome

MMAT stops treating proposal approval as the primary source of truth and instead freezes an explicit `ProjectContract`.

### Tasks

1. Add `ProjectContract` model.
2. Add contract generation stage between approval and planning.
3. Add human approval for the contract artifact.
4. Add `ChangeRequest` model for later scope changes.
5. Ensure downstream planning and execution reference the contract, not the proposal alone.
6. Persist approved contract and any change requests to disk.

### Contract Fields

- problem statement
- user goals
- non-goals
- constraints
- assumptions
- approved tech choices
- explicit exclusions
- acceptance criteria
- definition of done
- demo scenarios

### Likely MMAT Files

- `src/models.rs`
- `src/prompts.rs`
- `src/workflow.rs`

### Validation

- tests proving implementation cannot start without an approved contract
- tests proving later deviations require a change-request artifact
- `cargo test`

## Milestone 4: Reduce Solution Branching By Default

### Outcome

Solution exploration becomes cheaper and less noisy.

### Tasks

1. Replace the hardcoded five-branch default with a smaller default set.
2. Recommended default branch set:
   - conservative
   - recommended
   - ambitious
3. Allow expanded branch sets only when discovery marks the task as unusually architectural.
4. Update README and prompts to reflect the new behaviour.

### Likely MMAT Files

- `src/models.rs`
- `src/prompts.rs`
- `src/workflow.rs`
- `README.md`

### Validation

- workflow tests for default branch counts
- `cargo test`

## Milestone 5: Execution Plan And Task Cards

### Outcome

Planning becomes contract-driven and execution-ready instead of milestone summaries plus loosely structured worklists.

### Tasks

1. Expand `ImplementationPlan` into an `ExecutionPlan`.
2. Add `TaskCard` artifacts with stable ids.
3. Include for each task:
   - contract references
   - expected files
   - verification commands
   - dependencies
   - rollback notes
   - acceptance criteria
4. Generate task cards before implementation starts.
5. Persist the full plan and all task cards to disk.

### Likely MMAT Files

- `src/models.rs`
- `src/prompts.rs`
- `src/workflow.rs`

### Validation

- tests for stable task ids and task card serialisation
- tests proving planning output includes verification commands
- `cargo test`

## Milestone 6: Evidence-Carrying Task Results

### Outcome

Implementation stops narrating progress and starts producing machine-readable proof.

### Tasks

1. Replace or extend `ImplementationItemResult` into `TaskResult`.
2. Record for each completed item:
   - task id
   - goal
   - contract refs
   - files changed
   - commands run
   - command results
   - reviewer findings
   - manual or scenario checks
   - known gaps
   - scope deviation
   - provenance metadata
3. Persist each task result under `task-results/`.
4. Aggregate accepted evidence into `evidence-log.json`.

### Likely MMAT Files

- `src/models.rs`
- `src/workflow.rs`
- new `src/evidence.rs`

### Validation

- tests for task result generation
- tests for evidence aggregation
- `cargo test`

## Milestone 7: Dual Validation Lanes

### Outcome

MMAT validates both code health and project intent.

### Tasks

1. Keep the existing Rust health lane:
   - `cargo fmt --all`
   - `cargo check`
   - `cargo test`
   - `cargo clippy -- -D warnings`
2. Add an intent validation lane that checks:
   - contract conformance
   - claimed vs proven behaviour
   - scenario or demo completion
   - stub and placeholder detection
3. Feed both lanes into task acceptance and release decisions.
4. Persist validator outputs as evidence.

### Likely MMAT Files

- `src/models.rs`
- `src/prompts.rs`
- `src/workflow.rs`

### Validation

- tests proving a clean build alone is not enough for acceptance
- tests proving contract failures block release
- `cargo test`

## Milestone 8: Adversarial Release Assessment

### Outcome

The final review becomes a release judge over artifacts and evidence, not a friendly summary over stage outputs.

### Tasks

1. Replace or extend `FinalReview` into `ReleaseAssessment`.
2. Require the final judge to answer only:
   - what contract items are satisfied
   - what remains incomplete
   - what was claimed but not proven
   - whether the result is releasable
   - residual risks
3. Ensure the release stage consumes:
   - project contract
   - execution plan
   - task results
   - evidence log
4. Persist the release assessment to disk.

### Likely MMAT Files

- `src/models.rs`
- `src/prompts.rs`
- `src/workflow.rs`

### Validation

- tests for release disposition logic
- tests proving incomplete contract coverage is surfaced explicitly
- `cargo test`

## Milestone 9: NAAF Checkpoint And Resume Integration

### Outcome

MMAT runs are resumable and inspectable using NAAF's existing workflow persistence APIs.

### Tasks

1. Add stable runner keys to MMAT dynamic workflow nodes.
2. Attach a NAAF checkpointer to MMAT workflows.
3. Save workflow and step checkpoints during execution.
4. Add MMAT resume support from an existing run id.
5. Keep MMAT artifact persistence and NAAF checkpoint persistence aligned.

### Likely MMAT Files

- `src/workflow.rs`
- `src/main.rs`
- `Cargo.toml`

### Likely NAAF Files

- possibly no framework changes for initial adoption
- add fixes only if MMAT exposes real gaps

### Notes

- Prefer consuming existing NAAF persistence first.
- Only extend NAAF if MMAT proves an actual missing abstraction.

### Validation

- integration tests for interrupted and resumed runs
- `cargo test`

## Milestone 10: CLI And Non-Interactive Execution

### Outcome

MMAT can run without the TUI and the TUI becomes an observer over persisted runs.

### Tasks

1. Add CLI flags for:
   - `--prompt`
   - `--project-root`
   - `--run-dir`
   - `--resume`
   - `--export-artifacts`
2. Allow non-interactive approval and revision flows when artifacts are pre-supplied.
3. Preserve the current TUI workflow as a first-class interface.
4. Update the README with TUI and CLI usage.

### Likely MMAT Files

- `src/main.rs`
- `src/runtime.rs`
- `README.md`

### Validation

- CLI parsing tests
- run-directory smoke tests
- `cargo test`

## Milestone 11: Knowledge As Compiled Artifacts

### Outcome

Repository facts, project memory, and external evidence become durable inputs to delivery, not transient prompt stuffing.

### Tasks

1. Define three knowledge channels:
   - repository knowledge
   - project memory
   - external evidence
2. Compile retrieved knowledge into artifacts that can be cited by later stages.
3. Record provenance for imported knowledge.
4. Keep knowledge retrieval optional and subordinate to artifact quality.

### Likely MMAT Files

- `src/models.rs`
- `src/workflow.rs`
- `src/runtime.rs`

### Likely NAAF Files

- `../naaf/main/crates/knowledge/*` only if new capabilities are genuinely needed

### Validation

- tests proving knowledge artifacts are serialised and cited
- `cargo test`

## Milestone 12: Extract Stable Concepts Back Into NAAF

### Outcome

Only after MMAT stabilises, extract generic artifact and provenance concepts into NAAF.

### Candidate Extractions

- generic artifact persistence traits
- provenance envelope types
- run manifest helpers
- checkpoint and artifact coordination helpers

### Constraint

Do not do this early. MMAT should be the proving ground.

## Cross-Cutting Concerns

### Provenance

Every durable artifact should eventually record:

- producer kind: model, tool, human, or process
- producer identifier
- timestamp
- related run id
- related task id
- optional commit or worktree reference

### Parallelism

- Do not maximise parallelism before interfaces are frozen.
- Keep the current worktree parallelism, but tighten its inputs around task cards and contract refs.

### Safety

- Do not allow silent scope mutation after contract approval.
- Do not let final review rely on model claims without evidence artifacts.
- Do not mark tasks complete on build health alone.

## Suggested Initial Implementation Slice

The first concrete coding slice should be:

1. add run directory support
2. add artifact model module
3. persist discovery, proposal, approval, plan, and final review as JSON
4. update runtime with run id and run path
5. add tests for artifact persistence

This is the smallest slice that creates durable value immediately and sets up the remaining refactor safely.

## Definition Of Done For This Programme

The programme is complete when MMAT can:

1. capture a vague prompt into an intent brief with bounded clarification
2. freeze an approved project contract on disk
3. generate an execution plan and task cards that cite the contract
4. execute tasks while recording evidence-carrying task results
5. judge release readiness from artifacts and evidence
6. resume an interrupted run from persisted state
7. operate through both TUI and non-interactive CLI modes

## Mandatory Checks Before Commit Or PR

Run these before landing any backlog item:

- `cargo fmt --all`
- `cargo clippy -- -D warnings`
- `cargo test`
