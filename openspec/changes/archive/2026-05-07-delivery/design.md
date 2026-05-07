## Context

The understanding roles provide intent, evidence, and standards. The delivery roles convert those into concrete code changes. This changeset implements four roles that form the execution chain: Architect (design), PM (plan), Worker (implement), Reviewer (validate). Together they form the core delivery loop: design → plan → implement → review → rework or accept.

The `project` crate is introduced here to provide worktree isolation for Workers — each task card gets an isolated git worktree where changes are made, tested, and reviewed before merging.

## Goals / Non-Goals

**Goals:**
- Implement Architect with ADR production, tradeoff evaluation, and dependency rule definition
- Implement PM with delivery graphs, task cards, dependency management, and progress tracking
- Implement Worker with worktree isolation, tool-assisted implementation, and evidence emission
- Implement Reviewer with rubric-based review, failure classification, and escalation routing
- Create `project` crate for worktree isolation and repo operations

**Non-Goals:**
- Git merge or PR creation (Worker produces patches; merge is a separate concern)
- CI/CD integration (Reviewer runs local checks; external CI is out of scope)
- Multi-repo orchestration (v1 is single-repo)

## Decisions

### Decision 1: Worker is disposable — fresh LLM context per task

**Chosen**: Each `TaskAssigned` event targeting a Worker spawns a new Worker context with a fresh LLM conversation, scoped tools, and isolated worktree. The Worker does not maintain state between tasks.

**Rationale**: The conversation design explicitly says "Workers should not accumulate long-lived memory/personality." A fresh context per task prevents context pollution, makes workers parallelisable, and ensures each task starts from a known state (the contract + retrieved memory).

### Decision 2: DeliveryGraph as event-native replacement for DomainTree

**Chosen**: The old MMAT's `DomainTree` becomes `DeliveryGraph` — a DAG of task nodes with dependencies, not a hierarchical domain decomposition. The PM builds it from Architect's ADRs.

**Rationale**: The old DomainTree assumed a top-down domain decomposition. The new architecture is event-driven — the PM receives ADRs defining system components and their interfaces, then builds a dependency-ordered task graph. The graph is published as a `DeliveryGraph` artefact and updated via events as tasks complete.

### Decision 3: Reviewer classifies failures with structured escalation

**Chosen**: The Reviewer publishes `ReviewCompleted` with `findings` (list of issues) and `accepted` (boolean). If not accepted, `findings` includes a `failure_class` enum: `ImplementationDefect`, `ArchitecturalConflict`, `MissingKnowledge`, `AmbiguousIntent`, `BrokenProcess`. The Reviewer also publishes `EscalationRequested` with the appropriate severity mapped from the failure class.

**Rationale**: This maps exactly to the conversation's escalation design. The coordinator routes the escalation based on the Reviewer's classification, not the Reviewer's own judgment of where to escalate.

### Decision 4: `project` crate as utility for Worker and PM

**Chosen**: A new `crates/project/` crate providing: `WorktreeHandle` (create, apply patches, run commands, delete), `RepoDiscovery` (find existing projects, detect language/framework), `ProjectScaffold` (create new projects), and LLM tool implementations for conversational project setup.

**Rationale**: The user explicitly requested formalised project orchestration. This crate is consumed by Worker (worktree isolation) and can be exposed as LLM tools for Intent Lead or PM to use conversationally.

### Decision 5: Worktree retention on failure only

**Chosen**: Successful task worktrees are cleaned up immediately. Failed task worktrees are retained for a configurable duration (0 to infinite hours, set globally and per change request via Ops Manager policy). A CLI provides query and manual cleanup of retained worktrees.

**Rationale**: Successful tasks don't need debugging. Failed tasks benefit from worktree inspection. Configurable time limits prevent disk exhaustion while enabling forensic analysis. The CLI gives operators direct control without touching the event stream.

## Risks / Trade-offs

- **[Risk] Worker produces broken code** → Mitigation: Reviewer gate catches issues before merge. Worker retries on implementation defects. Worktree isolation prevents repo corruption.
- **[Risk] Architect produces unbounded designs** → Mitigation: Architect works against Intent Brief constraints and Scholar evidence. ADRs must reference both.
- **[Risk] PM's task decomposition is wrong** → Mitigation: Tasks reference ADRs. Reviewer escalates architectural conflicts back to Architect. Backflow from old MMAT conceptually preserved via escalation.
- **[Trade-off] Disposable Workers lose context** → The task card + retrieval planner provide the necessary context. Previous task outputs are available as artefacts in the event stream.

## Resolved Questions

- **Worktree cleanup**: Retain on failure only. Configurable time limit (0 to infinite hours), set globally and per change request via Ops Manager policy. CLI available to query and manually clean retained worktrees.
- **Parallel execution**: Parallel by default — concurrent Workers where DAG dependencies allow. Worktree isolation makes this safe. The coordinator's scheduler handles concurrent role execution naturally.
