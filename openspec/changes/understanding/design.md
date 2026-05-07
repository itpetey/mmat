## Context

The coordinator provides the governance framework (Role trait, contracts, budgets, scheduling). The understanding roles are the first concrete role implementations — they consume events, use LLMs and tools, and produce structured artefacts. These three roles (Intent Lead, Scholar, Ops Manager) share a common pattern: they gather context before delivery begins. They are "upstream" roles that the Architect and PM depend on.

All three are persistent actors — the Scholar and Ops Manager accumulate institutional knowledge across runs. The Intent Lead is more run-scoped but may persist between sessions to maintain a stakeholder model.

## Goals / Non-Goals

**Goals:**
- Implement Intent Lead as a `Role` that interrogates the human, produces intent briefs, and dispatches other roles
- Implement Scholar as a `Role` that gathers evidence, studies repos, and produces research briefs
- Implement Ops Manager as a `Role` that manages SOPs, rubrics, and procedural memory
- Each role uses the LLM and tool crates directly, not through adapter projections

**Non-Goals:**
- Architect, PM, Worker, Reviewer, Auditor — separate changesets
- Human interaction UI (Intent Lead uses the event stream; the actual UI is out of scope)
- Knowledge materialisation (memory-core handles that; Scholar feeds it)
- Project orchestration tools (separate `project` crate, not yet created)

## Decisions

### Decision 1: Roles crate (`crates/roles/`) with modules per role

**Chosen**: A single `roles` crate with `src/intent_lead.rs`, `src/scholar.rs`, `src/ops_manager.rs`, each exporting a struct implementing the `Role` trait. Not separate crates per role.

**Rationale**: Roles share dependencies (event-stream, memory, coordinator, llm, process). Separate crates per role would create 8+ tiny crates with near-identical `Cargo.toml` files. A single crate with modules keeps the dependency graph simple while preserving module-level separation.

**Alternative considered**: Roles in the `coordinator` crate. Rejected because the coordinator is governance infrastructure — it should not depend on specific role implementations. The dependency should be: `roles` depends on `coordinator`, not vice versa.

### Decision 2: Intent Lead dispatches via events, not direct calls

**Chosen**: The Intent Lead publishes `TaskAssigned` events targeting Scholars and Ops Managers. It does not directly call their `run()` methods.

**Rationale**: The coordinator owns the dispatch. The Intent Lead expresses intent ("dispatch a Scholar to research X") by publishing an event. The coordinator routes it. This keeps roles decoupled and makes dispatch auditable.

### Decision 3: Scholar uses tool-calling LLM with repository access

**Chosen**: The Scholar is configured with an `LlmClient` + `Executor` + `ToolRegistry` containing: file reading, web search, knowledge base query, and git history tools. The Scholar's LLM prompt instructs it to gather evidence, not make decisions.

**Rationale**: The Scholar's job is epistemic — answering "what appears true?" The tool set reflects this: read-only access to the repo and internet. No write tools. The LLM prompt reinforces the epistemic stance.

### Decision 4: Ops Manager stores SOPs as Memory items with SOP type

**Chosen**: The Ops Manager publishes `MemoryProposed` events with `MemoryType::SOP` and `MemoryScope::Organisational`. The librarian accepts these into the memory store. The Ops Manager queries existing SOPs when creating new ones to avoid duplication.

**Rationale**: SOPs are durable institutional knowledge. They should go through the same write gates as all other memory. The Ops Manager is a producer of SOPs, not a separate SOP database.

### Decision 5: Procedural memory as retrieval-triggered rules

**Chosen**: The Ops Manager maintains procedural memory as SOP-type memories with trigger conditions encoded in content. When a role encounters a situation (e.g., "starting a database migration"), it queries memory for relevant SOPs. The Ops Manager's value is in creating and maintaining these SOPs, not in actively enforcing them — enforcement belongs to the Reviewer.

**Rationale**: Pushing enforcement to every role invocation would require the Ops Manager to subscribe to all events. Instead, roles pull SOPs from memory when they need them. The Reviewer checks whether the Worker followed the applicable SOPs.

### Decision 6: Per-role LLM model configuration

**Chosen**: Every role is composable with its own LLM model, base URL, and API key via `RoleContext`. There is no global default model. The Intent Lead may use a conversational model, the Scholar a high-context research model, and the Worker a code-generation model.

**Rationale**: Different roles have different capability needs. A Scholar researching prior art needs a high context window and low temperature for factual accuracy. A Worker generating code benefits from a model optimised for code completion. Forcing one model on all roles compromises every use case.

### Decision 7: Human-accessible Ops Manager through conversational interface

**Chosen**: SOPs are created and updated through the Ops Manager's LLM-mediated process for consistency, but the human can converse with the Ops Manager to recommend changes, additions, deletions, and query choices. The Ops Manager publishes `HumanFeedbackRequested` when it needs clarification on process requirements, and consumes `HumanFeedbackReceived` to incorporate human guidance.

**Rationale**: LLM-mediated creation ensures SOPs follow the standard format and pass through librarian write gates. Conversational access gives the human a natural interface to shape organisational processes without bypassing governance.

## Risks / Trade-offs

- **[Risk] Intent Lead hallucinates user requirements** → Mitigation: Intent Lead publishes `HumanFeedbackRequested` before finalising any IntentBrief. The human must approve. The brief includes confidence annotations and open questions.
- **[Risk] Scholar overwhelms memory store with low-value facts** → Mitigation: The attention engine and librarian filter Scholar-produced MemoryProposed events. The Scholar's research budget prevents unbounded exploration.
- **[Risk] Ops Manager SOPs become stale** → Mitigation: SOPs use `DecayPolicy::StaleAfterDays` with a generous window (180 days). The Ops Manager's periodic review loop checks for stale SOPs and proposes updates.

## Resolved Questions

- **Scholar LLM model**: Separate model per role — every role is composable with its own model configuraiton. The Scholar may use a high-context research-optimised model while the Worker uses a code-generation model.
- **Human-editable SOPs**: LLM-mediated creation through the Ops Manager ensures consistency. The human can converse with the Ops Manager to recommend changes, additions, deletions, and query choices. SOPs remain governed by librarian write gates regardless of how they were initiated.
