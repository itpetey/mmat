## Context

MMAT is a rewrite of the previous implementation in `../main/`. The old project preserved a useful high-level workflow shape, but its implementation grouped code by generic artefact type such as prompts, tasks, models, and steps. That made it difficult to trace one workflow stage end-to-end and encouraged tight coupling between orchestration and the browser frontend.

The new implementation needs to stay broadly faithful to the old workflow sequence while changing two core architectural assumptions:

1. Workflow code is grouped by subject under `src/workflow/`, so each stage owns its own types, prompts, step construction, and orchestration helpers.
2. Knowledge is planned and materialised explicitly, then exposed narrowly per stage so each LLM call sees only the relevant evidence.

The agreed workflow shape for this change is:

1. Discovery
2. Knowledge Planning
3. Knowledge Materialisation
4. Solution Branch Fan-out
5. Solution Collect + Recommend + User Choice
6. Software Architect
7. Implementation Planning
8. Execution

The first pass will support live-only human questioning. Pending prompts do not need reconnect-safe persistence in this change.

## Goals / Non-Goals

**Goals:**
- Establish a subject-oriented workflow layout under `src/workflow/`.
- Implement live recursive discovery that can ask the user follow-up questions until the gathered context is ready for solution generation.
- Separate knowledge planning from knowledge materialisation so proposed knowledge groups are validated and materialised deterministically.
- Persist knowledge-group metadata in SQLite using `naaf-persistence-sqlite`.
- Generate conservative, recommended, and ambitious solution branches concurrently from the gathered context.
- Add a collect step that presents candidate branches, recommends one or a hybrid, and asks the user to choose before handing off to a downstream Software Architect stage.
- Keep knowledge exposure scoped per stage rather than attaching one large repository-wide context to every LLM request.
- Record upstream NAAF gaps that should be fixed in NAAF instead of embedded as MMAT-specific workarounds.

**Non-Goals:**
- Recreate the previous frontend architecture or Dioxus-specific UI structure.
- Add reconnect-safe persistence for live human prompts in this first pass.
- Solve all upstream NAAF knowledge acquisition and duplicate-detection limitations inside MMAT.
- Design the full execution/worktree pipeline in this change beyond the interfaces required to hand off from architect/planning stages.

## Decisions

### 1. Group workflow code by subject, not by artefact type

MMAT will organise workflow code under stage-specific modules such as `workflow/discovery`, `workflow/knowledge`, and `workflow/solutions`. Each subject module will own the local types, prompts, step builders, and orchestration helpers needed for that stage. `workflow/mod.rs` will contain only workflow-wide coordination code and shared runtime glue.

Why:
- It makes one stage readable end-to-end.
- It matches how the user reasons about the workflow.
- It avoids the previous spread of one stage across `models`, `prompts`, `steps`, and `tasks` files.

Alternative considered:
- Keep the old split by prompts/tasks/models/steps. Rejected because it obscures stage ownership and makes workflow tracing harder.

### 2. Keep discovery recursion explicit in workflow orchestration

Discovery will be implemented as a live recursive loop at the workflow level, backed by one or more NAAF steps for individual turns. The system will allow the model to ask the human live questions through `QuestionTool`, then re-enter discovery with the updated context until the output is ready for solution generation.

Why:
- Human turn boundaries remain explicit.
- Live questioning is easier to surface in the UI/runtime boundary.
- It avoids hiding repeated live prompts inside opaque repair loops.

Alternative considered:
- Encode the full recursion only as a single retrying NAAF step. Rejected because it makes human interaction less explicit and complicates knowledge side effects.

### 3. Split knowledge into planning and materialisation

The workflow will first produce a `KnowledgePlan` describing zero or more candidate knowledge groups, their source types, and intended downstream consumers. A separate deterministic materialisation stage will validate that plan, persist group metadata in SQLite, initialise backing knowledge stores, and ingest available sources.

Why:
- The planning LLM can suggest groups without being trusted to mutate state directly.
- Materialisation becomes deterministic and testable.
- The same plan can be inspected, audited, or re-materialised.

Alternative considered:
- Allow the planning LLM to create knowledge groups directly. Rejected because it blurs authority boundaries and makes downstream reasoning about available context harder.

### 4. Use controlled knowledge-group templates plus run-specific instances

The system will not create unconditional default groups for every run. Instead, knowledge planning will work from a controlled template vocabulary such as workspace code, workspace docs, discovery transcript, web research, and papers, and will produce concrete run-scoped group instances only when they are useful.

Why:
- This keeps the planning model from inventing arbitrary uncontrolled group shapes.
- It preserves flexibility without forcing the same context into every run.
- It aligns with the goal of exposing only the knowledge each stage needs.

Alternative considered:
- Pre-create a fixed set of default groups on every run. Rejected because it encourages unnecessary ingestion and prompt bloat.

### 5. Scope knowledge exposure per stage

Each workflow stage will declare or receive the specific materialised knowledge groups it may use. LLM sessions will be built from only those groups, rather than from a single global knowledge context.

Why:
- It reduces irrelevant prompt/tool context.
- It makes downstream learning and evidence provenance easier to reason about.
- It aligns with the user’s stated goal of preventing prompt overload.

Alternative considered:
- Expose all known groups to every stage. Rejected because it increases noise and weakens stage-specific grounding.

### 6. Use SQLite for knowledge-group metadata persistence

Knowledge-group metadata will be stored via `naaf-persistence-sqlite::SqliteKnowledgeGroupStore` rather than the filesystem store.

Why:
- SQLite provides a better fit for structured run metadata and future queryability.
- It avoids adding new file-based persistence patterns for group metadata.

Alternative considered:
- Filesystem-backed group metadata. Rejected by explicit user preference and because SQLite is a better long-term fit.

### 7. Keep a distinct downstream Software Architect stage

After the user selects a solution branch or hybrid, MMAT will pass the selected solution and scoped knowledge into a dedicated Software Architect stage. That stage will then hand off to implementation planning.

Why:
- It preserves the old workflow’s architecture/planning separation.
- It gives the architect stage a clear responsibility: refine the chosen direction into an execution-ready architecture.
- It creates a clean point to further narrow or expand knowledge exposure before planning.

Alternative considered:
- Collapse architect review into implementation planning. Rejected because it weakens the separation between architectural judgement and execution breakdown.

### 8. Treat missing NAAF knowledge features as upstream work, not MMAT-only hacks

Where `naaf-knowledge` lacks required functionality, such as richer web/paper ingestion or duplicate detection, MMAT will record those needs as upstream NAAF changes instead of baking in ad hoc MMAT-only substitutes.

Why:
- The limitation is platform-level, not application-specific.
- MMAT should consume a coherent knowledge abstraction, not reimplement it.

Alternative considered:
- Build bespoke MMAT-side substitutes for missing NAAF capabilities. Rejected except for temporary seams that are explicitly marked for replacement.

## Risks / Trade-offs

- [Live-only discovery questions are not reconnect-safe] → Keep the first pass live-only, isolate the prompt transport behind runtime interfaces, and avoid baking reconnect assumptions into workflow logic.
- [Knowledge planning may over-propose groups] → Use controlled templates, deterministic validation, and explicit stage consumers in the `KnowledgePlan`.
- [Upstream NAAF gaps can block full knowledge ambitions] → Capture the required upstream changes in the design and keep MMAT interfaces thin so they can adopt NAAF improvements later.
- [Stage-specific scoping can become operationally fiddly] → Make group selection an explicit part of each stage input/output contract rather than an implicit runtime side effect.
- [Rewriting the workflow shape while the app is still sparse can lead to speculative abstractions] → Keep initial modules small and stage-oriented, and defer generic abstractions until at least two stages need them.

## Migration Plan

1. Initialise OpenSpec and capture the rewrite contract in specs, design, and tasks.
2. Introduce the subject-oriented workflow directory structure under `src/workflow/`.
3. Implement the discovery and knowledge planning/materialisation stages first so later stages have stable inputs.
4. Implement solution branch generation and user selection next, then connect the selected output to a dedicated Software Architect stage.
5. Integrate implementation planning and later execution stages onto the new handoff contracts.
6. Keep the old `../main` code as a reference only; do not port its module structure forward.

Rollback is straightforward during development because the rewrite currently has only stub workflow code. If a stage design proves incorrect, the individual stage module can be rewritten without preserving the previous module layout.

## Open Questions

- Which upstream NAAF change should be implemented first to support this workflow best: richer knowledge acquisition, duplicate detection, or both together?
- How should the Software Architect stage express its selected knowledge-group needs for implementation planning and execution stages?
