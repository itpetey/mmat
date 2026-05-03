## 1. NAAF Pipeline primitive

- [x] 1.1 Define `Phase` trait (typed Input/Output, single async run method) in `naaf_core/src/pipeline.rs`
- [x] 1.2 Define `Pipeline` struct with phase registration and route declarations
- [x] 1.3 Define `Route` enum: `Next(PhaseId)`, `Switch(Box<dyn Fn(&O) -> PhaseId>)`, `Parallel(Vec<PhaseId>)`, `Halt`
- [x] 1.4 Implement Pipeline execution: start at initial phase, follow routes, handle cycles with configurable depth limit
- [x] 1.5 Implement `Route::Parallel` — concurrent phase execution with join
- [x] 1.6 Implement `Route::Switch` — conditional branching based on phase output
- [x] 1.7 Add typed route compatibility validation at Pipeline construction time
- [x] 1.8 Implement Pipeline checkpointing: save/resume phase state using existing `Checkpointer` trait, checkpoint after each phase completes
- [x] 1.9 Add Pipeline unit tests (linear, switch, parallel, cycle, max depth, checkpoint/resume)
- [x] 1.10 Add Pipeline integration tests (multi-phase with Step-wrapping phases, checkpoint round-trip)
- [x] 1.11 Re-export Pipeline, Phase, Route from `naaf_core::lib.rs`

## 2. Remove Step DAG combinators and Workflow from NAAF

- [x] 2.1 Remove `.then()` from `Step`
- [x] 2.2 Remove `.join()` from `Step`
- [x] 2.3 Remove `.reconcile()` and `.reconcile_task()` from `Step`
- [x] 2.4 Remove `.zip()` from `Step`
- [x] 2.5 Remove `NeverFinding` re-export (only used by join/reconcile)
- [x] 2.6 Remove `Workflow` (`graph.rs`) entirely — delete module and all related types: `Workflow`, `WorkflowNode`, `WorkflowRunReport`, `NodeContext`, `NodeInput`, `NodeOutcome`, `NodeSpec`, `NodeSummary`, `GraphPatch`, `EdgeSpec`, `StepNode`, `RunnerRegistry`, `NodeCheckpoint`, and checkpoint types
- [x] 2.7 Remove Workflow-related re-exports from `naaf_core::lib.rs`

## 3. Migrate NAAF examples to Pipeline

- [x] 3.1 Rewrite `examples/composed-workflow` to use Pipeline
- [x] 3.2 Rewrite `examples/join-reconcile` to use Pipeline
- [x] 3.3 Rewrite `examples/dynamic-workflow` to use Pipeline (replaces Workflow usage)
- [x] 3.4 Audit remaining examples for Step DAG combiner or Workflow usage; migrate or remove
- [x] 3.5 Verify all NAAF examples compile and pass

## 4. MMAT domain-mapped planning

- [x] 4.1 Define `DomainTree`, `DomainNode`, `DomainNodeId`, `DomainTreeConfig` types in `src/plan/domain_map.rs`
- [x] 4.2 Implement configurable maximum tree depth via `DomainTreeConfig::max_depth` (default: 3)
- [x] 4.3 Implement recursive sub-divide discovery: LLM-driven domain decomposition with max depth enforcement
- [x] 4.4 Implement per-sub-domain Pipeline construction for plan stages (discovery → knowledge → solutions → architect)
- [x] 4.5 Implement cross-sub-domain knowledge sharing with `Public`/`Private` visibility on knowledge groups
- [x] 4.6 Implement per-sub-domain solution branch generation and selection
- [x] 4.7 Add domain map unit tests (tree construction, depth capping, leaf detection, config validation)
- [x] 4.8 Add domain map integration tests (recursive sub-divide with scripted LLM)

## 5. MMAT graph-based delivery

- [x] 5.1 Define `DeliveryGraph`, `DeliveryBatch`, `DeliveryNode` types in `src/deliver/delivery_graph.rs`
- [x] 5.2 Implement topological sort that produces dependency-ordered batches
- [x] 5.3 Implement parallel batch execution in `deliver/queue.rs` (separate worktrees for concurrent jobs)
- [x] 5.4 Update `BuildJob` to reference sub-domain node IDs
- [x] 5.5 Implement batch-level progress tracking and reporting
- [x] 5.6 Add delivery graph unit tests (topological sort, batch grouping, cycle detection)
- [x] 5.7 Add delivery graph integration tests (multi-job execution, parallel batch verification)

## 6. MMAT architectural backflow

- [x] 6.1 Define `BackflowEvent`, `BackflowSeverity` types in `src/plan/backflow.rs`
- [x] 6.2 Implement severity-based routing in delivery Pipeline (Route::Switch back to architect/solutions/discovery)
- [x] 6.3 Implement cascade logic: Critical backflow marks dependent sub-domains for replanning
- [x] 6.4 Implement configurable backflow cascade depth via `DomainTreeConfig::max_cascade_depth` (default: 3); escalate to human review when exhausted
- [x] 6.5 Implement knowledge group cleanup on sub-domain replanning: delete orphaned groups before re-materialising
- [x] 6.6 Wire backflow into `BuildEngine::final_review` result handling
- [x] 6.7 Add backflow unit tests (severity routing, cascade, depth capping, knowledge cleanup)
- [x] 6.8 Add backflow integration tests (end-to-end: discovery → plan → deliver → backflow → replan)

## 7. MMAT pipeline migration

- [x] 7.1 Replace `build_greenfield_step` static Step chain with Pipeline-based orchestrator
- [x] 7.2 Migrate discovery step to Phase-wrapping-Step
- [x] 7.3 Migrate knowledge planning/materialisation steps to Phase-wrapping-Step
- [x] 7.4 Migrate solution branch/collect/choice steps to Phase-wrapping-Step
- [x] 7.5 Migrate architect step to Phase-wrapping-Step
- [x] 7.6 Remove all `.then()`, `.join()`, `.map_input()` chain usage from plan module
- [x] 7.7 Remove any Workflow (`graph.rs`) usage from MMAT if present
- [x] 7.8 Verify existing plan tests still pass after migration

## 8. MMAT UI updates

**Strategy**: In-app tabs with 3-column shell (left sidebar, centre tabbed content, right detail panel). Projects with a domain tree render the multi-column layout; projects without render the existing single-column layout. All existing conversation rendering and state management patterns are preserved.

- [x] 8.1 Extend `UiState` and `UiSnapshot` with domain tree, delivery graph, sub-domain state, backflow notifications, and tab management fields. All new fields are optional so existing single-project flow works unchanged.
- [x] 8.2 Add new `FrontendEvent` variants: `DomainTreeUpdated`, `DomainNodePhaseChanged`, `BackflowStarted`, `BackflowCascade`, `BackflowResolved`, `BackflowHalting`, `DeliveryGraphUpdated`, `DeliveryBatchStarted`, `DeliveryBatchCompleted`.
- [x] 8.3 Add CSS for multi-domain 3-column shell (`.mmat-multi-domain` class), tab bar, domain tree with status badges, delivery graph with batch layers, backflow banner with severity colours, and right detail panel.
- [x] 8.4 Implement `TabBar` and `TabPanel` components — in-app tabs for sub-domain conversations with tab ordering, close support, backflow highlighting, and state preservation across tab switches.
- [x] 8.5 Implement `DomainTree` sidebar component — nested indent tree with per-node status badges, click-to-focus-tab navigation, and empty-tree placeholder.
- [x] 8.6 Implement `DeliveryGraph` mini-view component — batch layers with colour-coded job nodes, active batch highlighting, and pending placeholder.
- [x] 8.7 Implement `BackflowBanner` component — severity-coloured alert above affected sub-domain's conversation with cascade info and halt-on-exhausted notice.
- [x] 8.8 Implement `PipelinePhaseIndicator` breadcrumb component — shows per-sub-domain pipeline stage with current highlight, completed/pending states, and backflow retrace path.
- [x] 8.9 Implement `RightDetailPanel` component — collapsible contextual panel showing node status, phase, depth, knowledge group counts, dependents, and backflow history.
- [x] 8.10 Update `RootApp` to conditionally render multi-domain shell or single-column shell based on presence of `domain_tree`.
- [x] 8.11 Add UI tests for tab management (open, close, switch, preserve state), domain tree navigation, backflow banner display, and multi-column vs single-column layout switch.

## 9. Verification

- [x] 9.1 Run `cargo fmt --all` in NAAF
- [x] 9.2 Run `cargo clippy -- -D warnings` in NAAF
- [x] 9.3 Run `cargo test` in NAAF
- [x] 9.4 Run `cargo fmt --all` in MMAT
- [x] 9.5 Run `cargo clippy -- -D warnings` in MMAT
- [x] 9.6 Run `cargo test` in MMAT
- [x] 9.7 End-to-end test: greenfield a multi-sub-domain project through the full pipeline
