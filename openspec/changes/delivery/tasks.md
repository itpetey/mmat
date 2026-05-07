## 1. Roles Crate Extension

- [ ] 1.1 Create `src/architect.rs`, `src/project_manager.rs`, `src/worker.rs`, `src/reviewer.rs` in `crates/roles/`
- [ ] 1.2 Add module declarations to `crates/roles/src/lib.rs`
- [ ] 1.3 Define delivery artefact types in `src/artefacts.rs`: `ADR`, `InterfaceSpec`, `DependencyRules`, `DeliveryGraph`, `TaskCard`, `Milestone`, `ImplementationPatch`, `ReviewFindings`, `FailureClass`

## 2. Project Crate

- [ ] 2.1 Scaffold `crates/project/Cargo.toml` with dependencies (process, tokio, thiserror, tracing)
- [ ] 2.2 Implement `WorktreeHandle`: create isolated worktree from repo, apply patches, run commands within worktree, delete worktree
- [ ] 2.3 Implement `RepoDiscovery`: detect existing project (find Cargo.toml, package.json, etc.), identify language/framework
- [ ] 2.4 Implement `ProjectScaffold`: create new project directory with language-specific scaffolding
- [ ] 2.5 Implement LLM tool wrappers for project operations (for conversational use by Intent Lead/PM)
- [ ] 2.6 Add `crates/project` to workspace members

## 3. Architect

- [ ] 3.1 Implement `Architect` struct with LLM client, Executor, tool registry (knowledge search, file read), memory store handle
- [ ] 3.2 Implement `Role` trait: `id()`, `spec()` with `AuthorityScope::Architecture`, subscriptions to `TaskAssigned`
- [ ] 3.3 Implement `Architect::run()` actor loop: receive task with IntentBrief + ResearchBrief, produce ADRs
- [ ] 3.4 Implement ADR template: decision, context, alternatives considered, tradeoffs, consequences, references
- [ ] 3.5 Implement tradeoff evaluation: prompt LLM to generate at least two alternatives, compare against constraints
- [ ] 3.6 Implement `InterfaceSpec` generation: module name, input/output types, error modes, backwards compatibility
- [ ] 3.7 Implement `DependencyRules` generation: allowed/forbidden dependency directions
- [ ] 3.8 Publish ADRs as `DecisionRecorded` events; publish InterfaceSpecs and DependencyRules as `ArtefactProduced` events
- [ ] 3.9 Implement constraint validation: check every ADR against IntentBrief constraints; escalate on contradiction

## 4. Project Manager

- [ ] 4.1 Implement `ProjectManager` struct with memory store handle, coordinator handle, delivery graph state
- [ ] 4.2 Implement `Role` trait: `id()`, `spec()` with `AuthorityScope::Planning`, subscriptions to `TaskAssigned`, `TaskCompleted`, `TaskFailed`
- [ ] 4.3 Implement `DeliveryGraph` DAG data structure: nodes (task cards), edges (dependencies), topological sort
- [ ] 4.4 Implement `TaskCard` struct: description, contract, dependencies (list of ContractId), ADR references, validation policy, acceptance criteria
- [ ] 4.5 Implement work decomposition: on receiving ADRs, generate task cards with dependency ordering
- [ ] 4.6 Implement task assignment: when a task's dependencies are satisfied, publish `TaskAssigned` to a Worker
- [ ] 4.7 Implement progress tracking: maintain `HashMap<ContractId, TaskStatus>` updated from TaskCompleted/TaskFailed events
- [ ] 4.8 Implement `Milestone` publishing: when all tasks in a dependency group complete, publish milestone event
- [ ] 4.9 Implement blocker detection: timeout on Assigned tasks without completion → escalate

## 5. Worker

- [ ] 5.1 Implement `Worker` struct with LLM client, Executor, tool registry (file read/write scoped to worktree, shell commands), project crate handle
- [ ] 5.2 Implement `Role` trait: `id()`, `spec()` with `AuthorityScope::Implementation`, subscriptions to `TaskAssigned`
- [ ] 5.3 Implement `Worker::run()` actor loop: on TaskAssigned, spawn fresh execution context, implement, emit evidence, publish results
- [ ] 5.4 Implement worktree creation: on task start, create isolated worktree via `project::WorktreeHandle`
- [ ] 5.5 Implement scoped tool construction: file read tools have read-only access to original repo + worktree; file write tools scoped to worktree only
- [ ] 5.6 Implement implementation loop: LLM-driven code changes with tool calling, iterative until task scope is met or budget exceeded
- [ ] 5.7 Implement validation: run Ops Manager's validation commands (cargo fmt, clippy, test) from task card's validation policy
- [ ] 5.8 Publish `ToolExecuted` events for every tool invocation (file reads, file writes, shell commands)
- [ ] 5.9 Publish `ClaimMade` events for every assertion, referencing ToolExecuted evidence
- [ ] 5.10 Publish `ArtefactProduced` with the implementation patch/diff
- [ ] 5.11 Publish `TaskCompleted` referencing the task's ContractId
- [ ] 5.12 Implement worktree cleanup: on task completion or failure, delete worktree (or retain for debug window)

## 6. Reviewer

- [ ] 6.1 Implement `Reviewer` struct with LLM client, memory store handle, coordinator handle
- [ ] 6.2 Implement `Role` trait: `id()`, `spec()` with `AuthorityScope::Review`, subscriptions to `ReviewRequested`, `TaskCompleted` (from Workers)
- [ ] 6.3 Implement `Reviewer::run()` actor loop: on Worker completion, review implementation against rubrics
- [ ] 6.4 Implement rubric dimensions check: correctness, API design, cohesion, coupling, backwards compatibility, observability, error handling, concurrency, performance, security, test adequacy, migration safety
- [ ] 6.5 Implement architectural compliance check: validate implementation against Architect's ADRs and InterfaceSpecs
- [ ] 6.6 Implement `FailureClass` classification: ImplementationDefect, ArchitecturalConflict, MissingKnowledge, AmbiguousIntent, BrokenProcess
- [ ] 6.7 Implement `ReviewCompleted` event publishing with findings list and acceptance decision
- [ ] 6.8 Implement rework loop: on ImplementationDefect, publish rework instructions; coordinator republishes TaskAssigned to Worker
- [ ] 6.9 Implement escalation routing by failure class: ArchitecturalConflict → Architect, MissingKnowledge → Scholar, AmbiguousIntent → IntentLead, BrokenProcess → OpsManager
- [ ] 6.10 Implement retry limit enforcement: if rework count equals max_retries, escalate Critical instead of rework

## 7. Integration

- [ ] 7.1 Write integration test: Architect receives IntentBrief → produces ADR → PM receives ADR → produces TaskCards → Worker implements → Reviewer accepts
- [ ] 7.2 Write integration test: Worker implementation fails review → Reviewer classifies as ImplementationDefect → Worker retries → passes
- [ ] 7.3 Write integration test: Reviewer detects ArchitecturalConflict → escalates to Architect → Architect revises ADR → PM updates tasks
- [ ] 7.4 Write integration test: Worker's tools are scoped to worktree — cannot modify files outside
- [ ] 7.5 Write integration test: PM respects dependency ordering — dependent task not assigned until dependency completes
- [ ] 7.6 Write integration test: retry limit exhausted → Reviewer escalates Critical instead of requesting another rework

## 8. Validation

- [ ] 8.1 `cargo fmt --all` passes
- [ ] 8.2 `cargo clippy -- -D warnings` passes on all crates
- [ ] 8.3 `cargo test` passes all tests including doc tests
