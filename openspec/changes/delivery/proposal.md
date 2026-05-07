## Why

The understanding roles (Intent Lead, Scholar, Ops Manager) establish what the human wants, what is true about the domain, and what standards apply. But the organisation still cannot design, plan, implement, or review anything. The delivery roles — Architect, Project Manager, Worker, and Reviewer — form the execution chain that transforms intent and evidence into concrete code changes, validated against standards and architectural constraints.

## What Changes

- **New: Architect role** — Persistent actor that evaluates tradeoffs, selects architecture, defines boundaries and contracts, reasons about scaling/coupling/maintainability, chooses abstractions, and defines migration strategies. Publishes `ADRs`, architecture diagrams, `InterfaceSpecs`, and `DependencyRules` as durable artefacts. Depends on Intent Briefs (Intent Lead) and Research Briefs (Scholar). Reports to the Ops Manager for architectural policies.

- **New: Project Manager role** — Persistent actor that decomposes work, sequences tasks, manages dependencies, manages scope, tracks progress, and assigns execution. Publishes `DeliveryGraph`, `Milestone` plans, and `TaskCard`s. Depends on ADRs (Architect), Intent Briefs (Intent Lead), and SOPs (Ops Manager). Does NOT invent architecture — executes against established architectural decisions.

- **New: Worker role** — Disposable execution context per task card. Reads the task contract, inspects the repository (via worktree isolation), implements bounded scope, emits evidence (tool executions, claims), and publishes implementation artefacts. Workers are stateless — they do not accumulate memory or personality across tasks. Each Worker invocation runs in a fresh context with scoped tools.

- **New: Reviewer role** — Persistent actor that reviews Worker implementations against architectural compliance, code quality, and the Ops Manager's review rubrics. Publishes `ReviewCompleted` events with findings and acceptance decisions. Classifies failures and determines escalation paths: implementation defect → Worker retry, architectural conflict → Architect, missing knowledge → Scholar, ambiguous intent → Intent Lead, broken process → Ops Manager.

## Capabilities

### New Capabilities

- `architect`: Architecture decision records, tradeoff evaluation, contract/interface definition, dependency rule specification, migration strategy definition. Implements the `Role` trait with `AuthorityScope::Architecture`.
- `project-manager`: Work decomposition, task sequencing, dependency management, scope control, progress tracking, task assignment, delivery graph maintenance. Implements the `Role` trait with `AuthorityScope::Planning`.
- `worker`: Bounded implementation within worktree isolation, tool-assisted code generation and modification, test execution, evidence emission, artefact production. Implements the `Role` trait with `AuthorityScope::Implementation`. Workers are disposable — each task card spawns a fresh context.
- `reviewer`: Multi-dimensional code review against rubrics, architectural compliance validation, failure classification, escalation routing, rework request publication. Implements the `Role` trait with `AuthorityScope::Review`.

### Modified Capabilities

None — fifth changeset in greenfield workspace.

## Impact

- **Extends** `crates/roles/` with new modules: `architect.rs`, `project_manager.rs`, `worker.rs`, `reviewer.rs`
- **Dependencies**: All existing crates (event-stream, memory, coordinator, llm, process) plus the `project` crate (for worktree isolation — to be created as part of this changeset or separately)
- **`project` crate**: Provides worktree isolation, repository discovery, directory scaffolding, and git operations. Consumed by Worker for sandboxed execution and by PM for project setup. LLM tools exposed for conversational project bootstrapping.
- **Event stream**: Architect publishes `DecisionRecorded`; PM publishes `TaskAssigned`; Worker publishes `ClaimMade`, `ToolExecuted`, `ArtefactProduced`; Reviewer publishes `ReviewCompleted`, `EscalationRequested`
