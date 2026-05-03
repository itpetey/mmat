## 1. NAAF Pipeline primitive

- [ ] 1.1 Define `Phase` trait (typed Input/Output, single async run method) in `naaf_core/src/pipeline.rs`
- [ ] 1.2 Define `Pipeline` struct with phase registration and route declarations
- [ ] 1.3 Define `Route` enum: `Next(PhaseId)`, `Switch(Box<dyn Fn(&O) -> PhaseId>)`, `Parallel(Vec<PhaseId>)`, `Halt`
- [ ] 1.4 Implement Pipeline execution: start at initial phase, follow routes, handle cycles with configurable depth limit
- [ ] 1.5 Implement `Route::Parallel` — concurrent phase execution with join
- [ ] 1.6 Implement `Route::Switch` — conditional branching based on phase output
- [ ] 1.7 Add typed route compatibility validation at Pipeline construction time
- [ ] 1.8 Implement Pipeline checkpointing: save/resume phase state using existing `Checkpointer` trait, checkpoint after each phase completes
- [ ] 1.9 Add Pipeline unit tests (linear, switch, parallel, cycle, max depth, checkpoint/resume)
- [ ] 1.10 Add Pipeline integration tests (multi-phase with Step-wrapping phases, checkpoint round-trip)
- [ ] 1.11 Re-export Pipeline, Phase, Route from `naaf_core::lib.rs`

## 2. Remove Step DAG combinators and Workflow from NAAF

- [ ] 2.1 Remove `.then()` from `Step`
- [ ] 2.2 Remove `.join()` from `Step`
- [ ] 2.3 Remove `.reconcile()` and `.reconcile_task()` from `Step`
- [ ] 2.4 Remove `.zip()` from `Step`
- [ ] 2.5 Remove `NeverFinding` re-export (only used by join/reconcile)
- [ ] 2.6 Remove `Workflow` (`graph.rs`) entirely — delete module and all related types: `Workflow`, `WorkflowNode`, `WorkflowRunReport`, `NodeContext`, `NodeInput`, `NodeOutcome`, `NodeSpec`, `NodeSummary`, `GraphPatch`, `EdgeSpec`, `StepNode`, `RunnerRegistry`, `NodeCheckpoint`, and checkpoint types
- [ ] 2.7 Remove Workflow-related re-exports from `naaf_core::lib.rs`

## 3. Migrate NAAF examples to Pipeline

- [ ] 3.1 Rewrite `examples/composed-workflow` to use Pipeline
- [ ] 3.2 Rewrite `examples/join-reconcile` to use Pipeline
- [ ] 3.3 Rewrite `examples/dynamic-workflow` to use Pipeline (replaces Workflow usage)
- [ ] 3.4 Audit remaining examples for Step DAG combiner or Workflow usage; migrate or remove
- [ ] 3.5 Verify all NAAF examples compile and pass

## 4. MMAT domain-mapped planning

- [ ] 4.1 Define `DomainTree`, `DomainNode`, `DomainNodeId`, `DomainTreeConfig` types in `src/plan/domain_map.rs`
- [ ] 4.2 Implement configurable maximum tree depth via `DomainTreeConfig::max_depth` (default: 3)
- [ ] 4.3 Implement recursive sub-divide discovery: LLM-driven domain decomposition with max depth enforcement
- [ ] 4.4 Implement per-sub-domain Pipeline construction for plan stages (discovery → knowledge → solutions → architect)
- [ ] 4.5 Implement cross-sub-domain knowledge sharing with `Public`/`Private` visibility on knowledge groups
- [ ] 4.6 Implement per-sub-domain solution branch generation and selection
- [ ] 4.7 Add domain map unit tests (tree construction, depth capping, leaf detection, config validation)
- [ ] 4.8 Add domain map integration tests (recursive sub-divide with scripted LLM)

## 5. MMAT graph-based delivery

- [ ] 5.1 Define `DeliveryGraph`, `DeliveryBatch`, `DeliveryNode` types in `src/deliver/delivery_graph.rs`
- [ ] 5.2 Implement topological sort that produces dependency-ordered batches
- [ ] 5.3 Implement parallel batch execution in `deliver/queue.rs` (separate worktrees for concurrent jobs)
- [ ] 5.4 Update `BuildJob` to reference sub-domain node IDs
- [ ] 5.5 Implement batch-level progress tracking and reporting
- [ ] 5.6 Add delivery graph unit tests (topological sort, batch grouping, cycle detection)
- [ ] 5.7 Add delivery graph integration tests (multi-job execution, parallel batch verification)

## 6. MMAT architectural backflow

- [ ] 6.1 Define `BackflowEvent`, `BackflowSeverity` types in `src/plan/backflow.rs`
- [ ] 6.2 Implement severity-based routing in delivery Pipeline (Route::Switch back to architect/solutions/discovery)
- [ ] 6.3 Implement cascade logic: Critical backflow marks dependent sub-domains for replanning
- [ ] 6.4 Implement configurable backflow cascade depth via `DomainTreeConfig::max_cascade_depth` (default: 3); escalate to human review when exhausted
- [ ] 6.5 Implement knowledge group cleanup on sub-domain replanning: delete orphaned groups before re-materialising
- [ ] 6.6 Wire backflow into `BuildEngine::final_review` result handling
- [ ] 6.7 Add backflow unit tests (severity routing, cascade, depth capping, knowledge cleanup)
- [ ] 6.8 Add backflow integration tests (end-to-end: discovery → plan → deliver → backflow → replan)

## 7. MMAT pipeline migration

- [ ] 7.1 Replace `build_greenfield_step` static Step chain with Pipeline-based orchestrator
- [ ] 7.2 Migrate discovery step to Phase-wrapping-Step
- [ ] 7.3 Migrate knowledge planning/materialisation steps to Phase-wrapping-Step
- [ ] 7.4 Migrate solution branch/collect/choice steps to Phase-wrapping-Step
- [ ] 7.5 Migrate architect step to Phase-wrapping-Step
- [ ] 7.6 Remove all `.then()`, `.join()`, `.map_input()` chain usage from plan module
- [ ] 7.7 Remove any Workflow (`graph.rs`) usage from MMAT if present
- [ ] 7.8 Verify existing plan tests still pass after migration

## 8. MMAT UI updates

- [ ] 8.1 Implement tab-based sub-domain discovery in LiveView UI
- [ ] 8.2 Implement domain tree visualisation component
- [ ] 8.3 Implement delivery graph progress visualisation component
- [ ] 8.4 Implement backflow notification in UI
- [ ] 8.5 Add UI tests for tab management, domain tree navigation

## 9. Verification

- [ ] 9.1 Run `cargo fmt --all` in NAAF
- [ ] 9.2 Run `cargo clippy -- -D warnings` in NAAF
- [ ] 9.3 Run `cargo test` in NAAF
- [ ] 9.4 Run `cargo fmt --all` in MMAT
- [ ] 9.5 Run `cargo clippy -- -D warnings` in MMAT
- [ ] 9.6 Run `cargo test` in MMAT
- [ ] 9.7 End-to-end test: greenfield a multi-sub-domain project through the full pipeline
