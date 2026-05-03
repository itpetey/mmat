## Why

MMAT's plan-and-deliver pipeline is a single linear funnel: discovery narrows to one solution-ready context, which feeds one delivery job. This works for MVP-scoped projects but cannot express maximalist projects that span multiple interacting sub-systems with complex dependency graphs.

Three structural gaps make this impossible without an architectural change:

1. **No orchestration primitive for cyclic/conditional routing.** NAAF's `Step` is a linear, typed, forward-only unit of work. Composing Steps via `.then()`, `.join()`, and `.map()` builds immutable DAGs at compile time. Backflow — routing delivery findings into a prior planning stage — requires either cloning the entire graph or building a new one from scratch. Neither scales.

2. **No concept of sub-domains or multi-system delivery.** The pipeline produces one `DesignHandoff` and queues one `BuildJob`. A project with three interacting sub-systems (e.g., Auth, API Gateway, Data Layer) has no way to plan each sub-system independently, deliver them in dependency order, or run independent sub-systems in parallel.

3. **No architectural backflow from delivery to planning.** The delivery engine's `FinalReview → remediation_items` loop stays within the same job. When implementation reveals that a sub-domain's interface, boundary, or solution approach was wrong, there is no path back to the architect, solution selection, or discovery stages.

The NAAF project (also ours) needs a new `Pipeline` primitive that declares typed phases and explicit routes — including cycles — while keeping `Step` as the pure unit of work. MMAT then consumes Pipeline to implement domain-mapped, graph-based, backflow-capable pipelines.

## What Changes

### NAAF (`naaf_core`)

- **Add `Pipeline` primitive.** A new orchestrator that composes typed `Phase`s with declared routes (`Next`, `Switch`, `Parallel`, `Halt`). Supports cycles for backflow, conditional branching, and concurrent phase execution. Each `Phase` can wrap a `Step` internally for retry/validation/repair.

- **Remove static DAG helpers from `Step`.** `.then()`, `.join()`, `.reconcile()`, `.reconcile_task()`, `.zip()` are removed. `Step` becomes a pure unit of work: one task with an optional retry loop. Composition moves entirely to `Pipeline`.

- **Keep `Workflow` as a complementary primitive.** The dynamic additive-graph scheduler (`graph.rs`) serves exploratory agentic tasks. `Pipeline` serves declared, state-machine-driven workflows. They are complementary, not competing.

### MMAT

- **Domain-mapped planning.** Discovery produces a recursive `DomainTree` instead of a single solution-ready context. Each node represents a sub-domain. Internal nodes are decomposed further; leaf nodes proceed to solution generation. Each sub-domain runs its own planning pipeline. Discovery runs in parallel across sub-domains via tabbed-UI sessions.

- **Graph-based delivery.** Replace the single-job queue with a dependency-graph delivery scheduler. Sub-domain handoffs become build jobs. Jobs execute in topological batches: all jobs in a batch run in parallel (separate worktrees); each batch starts only after the previous completes.

- **Architectural backflow.** Delivery jobs emit `BackflowEvent`s when problems exceed remediation. Events carry severity (`Minor` stays within the job; `Moderate` → architect; `Major` → solution selection; `Critical` → domain mapping) and cascade to dependent sub-domains. Backflow is implemented as Pipeline's `Route::Switch` returning an earlier phase.

- **Cross-sub-domain knowledge sharing.** Knowledge groups gain a `Public`/`Private` visibility. When sub-domain B depends on A, B's knowledge session includes A's public groups.

- **Migrate `build_greenfield_step`.** Remove the static Step chain and replace it with a Pipeline-based orchestrator.

- **Tab-based UI.** The LiveView UI supports parallel sub-domain discovery in separate tabs, domain tree visualisation, delivery graph progress, and backflow notifications.

## Capabilities

### New Capabilities

- `pipeline-orchestrator` — NAAF: Typed phase-based pipeline with declared routes, cycle support, and conditional branching.
- `domain-mapped-planning` — MMAT: Recursive sub-divide domain mapping with per-sub-domain planning pipelines.
- `graph-based-delivery` — MMAT: Multi-job delivery with dependency-graph scheduling and parallel execution.
- `architectural-backflow` — MMAT: Severity-based backflow from delivery to plan stages with cascade support.

### Modified Capabilities

- `discovery-workflow` — Extended to support recursive sub-divide domain mapping and parallel tab-based sessions.
- `scoped-knowledge-groups` — Extended with `Public`/`Private` visibility for cross-sub-domain knowledge sharing.
- `solution-branch-selection` — Runs per sub-domain instead of once per project.

## Impact

- **NAAF `naaf_core`**: New `Pipeline` module. `Step` loses DAG combinators. `Workflow` (graph.rs) preserved as-is.
- **MMAT `src/plan/`**: New `domain_map.rs` module. `mod.rs` rewritten from static Step chain to Pipeline orchestrator. All stage modules migrated from Step-only to Phase-wrapping-Step.
- **MMAT `src/deliver/`**: `queue.rs` gains dependency-graph scheduling and parallel batch execution. `engine.rs` gains backflow event emission. New `delivery_graph.rs` module.
- **MMAT `src/liveview/`**: Tab-based sub-domain discovery UI. Domain tree and delivery graph visualisation. Backflow notification UI.
- **Breaking**: MMAT no longer uses Step `.then()`/`.join()` combinator chains. All pipeline composition uses `Pipeline`.
