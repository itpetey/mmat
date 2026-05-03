## Context

MMAT currently builds its entire greenfield pipeline as a static chain of `Step` combinators in `build_greenfield_step` (`src/plan/mod.rs:502`). This produces one `DesignHandoff` → one `BuildJob`. The underlying NAAF framework provides `Step` (typed unit of work with retry loops) and `Workflow` (dynamic DAG scheduler with additive node spawning in `graph.rs`).

`Step` provides `.then()`, `.join()`, `.reconcile()`, `.zip()` for static DAG composition. These are the "static DAG helpers" to be removed. `Workflow` handles dynamic graph expansion, dependency-based scheduling, and parallel execution, but it is imperative (nodes spawn children at runtime via `GraphPatch`), type-erased (all inter-node data is `serde_json::Value`), and explicitly rejects cycles.

Neither primitive supports:
- Declared state machines with typed inter-phase communication
- Cycles (routing back to earlier phases)
- Conditional branching based on phase output (beyond imperative child spawning)

The `Pipeline` primitive subsumes both: typed phases, declared routes, cycle support for backflow, and internal Step-based retry semantics. `Workflow` (`graph.rs`) is removed entirely — Pipeline replaces it.

## Goals / Non-Goals

**Goals:**
- Add `Pipeline` to `naaf_core` as a state-machine orchestrator with typed phases and declared routes.
- Remove `.then()`, `.join()`, `.reconcile()`, `.reconcile_task()`, `.zip()` from `Step`. Step remains a unit of work.
- Replace MMAT's `build_greenfield_step` static chain with a Pipeline-based orchestrator.
- Implement recursive sub-divide domain mapping that produces a `DomainTree`.
- Implement per-sub-domain planning pipelines (knowledge → solutions → architect per node).
- Implement graph-based multi-job delivery with dependency-order batching and parallel execution.
- Implement architectural backflow with severity-based routing and cascade support.
- Add cross-sub-domain knowledge sharing via public/private visibility.
- Tab-based UI for parallel sub-domain discovery.

**Non-Goals:**
- Recreating the Dioxus frontend from scratch. UI changes are incremental — adding a 3-column shell, in-app tabs, and new visualisation components while preserving all existing conversation rendering, state management, and single-project flows.
- Automatic sub-domain boundary detection from code. Domain mapping is LLM-driven, not static analysis.

## Decisions

### 1. Pipeline replaces Workflow, not coexists with it

**Choice**: Build `Pipeline` as the sole orchestrator primitive in `naaf_core`. Remove `Workflow` (`graph.rs`) entirely. Pipeline subsumes Workflow's scheduling, parallelism, and checkpointing capabilities while adding typed inter-phase communication, declared routing, and cycle support.

**Rationale**: Keeping both would create an unnecessary choice for consumers — when to use Pipeline vs. Workflow? Pipeline's declared-route model can express everything Workflow's imperative-spawning model can, plus cycles and conditional branching. Removing Workflow simplifies the API surface and forces consolidation on a single primitive.

Workflow's type-erased JSON model for inter-node communication is a liability: it loses compile-time type safety and forces every consumer to write serde boilerplate. Pipeline keeps typed I/O between phases — each phase declares its input and output types, and the pipeline ensures route compatibility.

**Alternatives considered**:
- Keep both as complementary primitives. Rejected by explicit decision — Pipeline subsumes Workflow's use cases.
- Extend Workflow to support cycles and conditional routing. Rejected because the imperative `GraphPatch` spawning model is fundamentally different from declared route-based composition, and the type-erased JSON model cannot be fixed without breaking changes anyway.

### 2. Keep Step, remove only its DAG combinators

**Choice**: Remove `.then()`, `.join()`, `.reconcile()`, `.reconcile_task()`, `.zip()` from `Step`. Keep everything else: `Step::builder()`, `Step::task()`, `.validate()`, `.materialise()`, `.repair_with()`, `.retry_policy()`, `.build()`, `.build_persistent()`, `.run()`, `.run_traced()`, `.map()`, `.map_input()`, `.map_with_input()`, `.map_findings()`.

**Rationale**: The DAG combinators are the leaky abstraction — they blur the line between unit of work and composition. Step's retry/repair/validation machinery is battle-tested and reused inside Pipeline phases. Removing only the DAG combinators is the minimal change that forces Pipeline adoption without breaking Step's core value.

The intra-step combinators (`.map()`, `.map_input()`, `.map_with_input()`, `.map_findings()`) are preserved because they transform a single Step's I/O — they don't compose Steps together. These are still useful when wrapping a Step inside a Phase.

**Alternatives considered**:
- Remove Step entirely, replace with Pipeline Phase. Rejected because Step's retry/repair loop would need to be reimplemented inside Phase. Step is already well-tested and serves a distinct purpose.

### 3. Pipeline uses declared Route enum, not imperative spawning

**Choice**: Pipeline phases declare their routes via `Route::Next(phase_id)`, `Route::Switch(fn)`, `Route::Parallel(phase_ids)`, `Route::Halt`. The pipeline executor follows routes deterministically.

**Rationale**: Declared routes make the pipeline graph statically analyzable. You can inspect a Pipeline and see every possible transition. This is essential for reasoning about backflow (does a cycle exist? is it finite?) and for debugging.

**Alternatives considered**:
- Imperative spawning (like `Workflow::GraphPatch`). Rejected because it makes cycles and conditional routing implicit and harder to verify.

### 4. Recursive sub-divide domain mapping

**Choice**: Discovery produces a `DomainTree`. Root nodes are broad; internal nodes decompose further; leaf nodes are concrete enough for solution generation. Discovery is recursive: each node's discovery session can identify sub-nodes. The process continues until all branches reach leaves.

**Rationale**: This mirrors how humans decompose large problems. Each sub-domain gets focused, sequential discovery. The human engages with one sub-domain at a time (in a UI tab). The tree structure preserves the "outer edge" of the domain while keeping individual discovery sessions manageable.

**Alternatives considered**:
- Two-phase (flat list then per-item discovery). Rejected by user preference for recursive sub-divide.
- Single flat discovery session. Rejected — the original problem this change solves.

### 5. Graph-based delivery with topological batching

**Choice**: Sub-domain handoffs become build jobs. Jobs execute in batches: all jobs whose dependencies are complete within a batch run in parallel. Each batch completes before the next starts. Topological sort determines batch membership.

**Rationale**: This is the simplest model that delivers both sequential dependence and parallel independence. Batch boundaries are natural checkpoints where the user can inspect results before proceeding.

**Alternatives considered**:
- Fully sequential. Rejected — independent sub-domains can and should build in parallel.
- Fully parallel with dynamic dependency resolution. Rejected — batch boundaries provide clearer progress tracking.

### 6. Backflow as Route::Switch cycles in Pipeline

**Choice**: A delivery phase's `Route::Switch` examines the `BackflowEvent` and returns the phase ID to route to. Minor severity → same delivery phase (retry). Moderate → architect phase. Major → solution selection phase. Critical → discovery phase (with cascade to dependent sub-domains).

**Rationale**: Backflow is just cycle routing. No special machinery needed beyond Pipeline's existing Route model. This keeps the backflow implementation trivial and the Pipeline abstraction clean.

**Alternatives considered**:
- Separate backflow queue/message bus. Rejected — adds infrastructure complexity for something Pipeline already handles.

### 7. In-app tab-based parallel discovery UI with 3-column shell

**Choice**: Each active sub-domain discovery session renders in its own in-app tab within a 3-column shell. The left sidebar shows the domain tree (navigable, with status badges) and a mini delivery graph. The centre panel has a tab bar with one tab per active sub-domain conversation. The right panel shows contextual per-node details. Projects without a domain tree continue to use the existing single-column layout.

**Rationale**: In-app tabs share a single Dioxus LiveView session (one WebSocket), giving all tabs access to the domain tree, delivery graph, and shared state via `Arc<UiState>`. This avoids the complexity of cross-session state sharing that browser tabs would require in a LiveView architecture. The 3-column shell keeps project-level context (domain tree, delivery progress) visible while the user focuses on one sub-domain at a time in the centre panel.

**Alternatives considered**:
- Browser tabs (one per sub-domain discovery session). Rejected — Dioxus LiveView requires a separate WebSocket per browser tab, making cross-tab state sharing complex and losing the persistent sidebar context. In-app tabs provide the same UX benefit (preserved per-sub-domain state) without the LiveView session overhead.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|---|
| Removing Workflow (`graph.rs`) breaks any NAAF consumers using it | Audit all NAAF examples and MMAT for Workflow usage. Migrate to Pipeline. This change removes Workflow entirely so there is no deprecation window — consumers must migrate. |
| Removing `.then()`/`.join()` from Step breaks any existing NAAF consumers | Audit NAAF examples and MMAT for all Step DAG usage. Migrate to Pipeline as part of this change. |
| Recursive domain mapping may never terminate (infinite decomposition) | Maximum tree depth is configurable per project (default: 3 levels). Discovery prompt instructs the LLM to stop when sub-domains are concrete enough for implementation. |
| Parallel delivery in separate worktrees may cause merge conflicts | Batches are dependency-ordered so nodes that modify the same files are never in the same batch. Cross-batch conflicts are resolved during merge. |
| Backflow cascades could cause infinite re-planning loops | Backflow cascade depth is configurable per project (default: 3). After exhausting, escalate to human review. |
| Pipeline's Switch routes lose compile-time type checking between phases | Enforce route compatibility at Pipeline construction time (not compile time). Validate that a Switch target's input type is compatible with the source's output type. Integration tests cover backflow routing. |
| Orphaned knowledge groups after sub-domain deletion/replanning | Knowledge groups scoped to a deleted or replanned sub-domain are deleted during the replanning phase. No manual cleanup required. |

## Migration Plan

1. **Add Pipeline to NAAF `naaf_core`** — New `pipeline.rs` module with `Phase`, `Pipeline`, `Route`, checkpointing support. No changes to existing code yet.
2. **Migrate NAAF examples** — Rewrite examples that use Step DAG combinators or Workflow to use Pipeline.
3. **Remove Workflow** — Delete `graph.rs` and all related types (`Workflow`, `WorkflowNode`, `StepNode`, `NodeSpec`, `NodeInput`, `NodeOutcome`, `GraphPatch`, checkpointer types).
4. **Remove Step DAG combinators** — Delete `.then()`, `.join()`, `.reconcile()`, `.reconcile_task()`, `.zip()` from `Step`.
5. **Add domain mapping to MMAT** — New `domain_map.rs`. Does not yet replace existing pipeline.
6. **Build per-sub-domain pipelines in MMAT** — Pipeline-based orchestrator for sub-domain planning.
7. **Migrate `build_greenfield_step`** — Remove the static Step chain, replace with Pipeline.
8. **Implement graph-based delivery** — Multi-job scheduling in `deliver/queue.rs`.
9. **Implement architectural backflow** — BackflowEvent types and Pipeline routing, including cascading replanning that deletes orphaned knowledge groups.
10. **Update MMAT UI** — Extend `UiState`/`UiSnapshot` with domain tree, delivery graph, and backflow fields. Add CSS for 3-column shell. Build in-app tab components, domain tree sidebar, delivery graph mini-view, backflow banner, pipeline phase indicator, and right detail panel. Update `RootApp` to render conditionally.
11. **Verify** — Run all tests, lint, format. Validate end-to-end flow with a multi-sub-domain test project.

Rollback is possible at any stage because Pipeline is additive. The old Step chain and Workflow can be preserved until the Pipeline replacement is verified.

## Resolved Questions

- **Should Pipeline support checkpointing in v1?** Yes. Pipeline supports checkpointing so long-running workflows can resume after restart. Pipeline phases use the same `Checkpointer` trait that Workflow used, applied per-phase execution rather than per-node.
- **Should `Workflow` (`graph.rs`) be deprecated or removed?** Removed entirely. No deprecation period. Pipeline subsumes all Workflow capabilities (scheduling, parallelism, checkpointing). Consumers migrate directly from Workflow to Pipeline.
- **Maximum domain tree depth?** Configurable per project via `DomainTreeConfig::max_depth`, defaulting to 3 levels.
- **Backflow cascade depth?** Configurable per project via `DomainTreeConfig::max_cascade_depth`, defaulting to 3 levels. After exhausting, the pipeline halts and surfaces the issue for human review.
- **Knowledge group cleanup after sub-domain deletion/replanning?** Orphaned knowledge groups are deleted during the replanning phase. When a sub-domain is replanned (due to backflow), its existing knowledge groups are dropped before new ones are materialised.
