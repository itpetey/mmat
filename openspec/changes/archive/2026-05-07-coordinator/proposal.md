## Why

With the event stream delivering structured cognitive events and the memory core providing durable institutional knowledge, the system still lacks the thing that makes it an organisation instead of a swarm: deterministic coordination. Without a coordinator, roles would freely consume and emit events with no budgets, no escalation paths, no enforced contracts, and no lifecycle management. The coordinator is the governance layer that prevents the system from becoming LLM bureaucracy — every role invocation is bounded by a typed contract, an authority scope, a resource budget, and a defined escalation path.

## What Changes

- **New: `coordinator` crate** — The deterministic role runtime. Provides role registration, typed contract enforcement, budget management (time, tokens, retries), escalation routing, stop condition evaluation, and stage-aware memory retrieval planning. Does NOT implement any roles — it governs them.
- **New: Role Registry** — Central catalog of all role types known to the organisation. Each entry includes the role's input contract type, output contract type, authority scope, default budget, escalation targets, and subscription filter (which event variants the role consumes).
- **New: Contract System** — Every role invocation is framed by a typed `Contract<I, O>` specifying expected input and output schemas, authority limits, and completion criteria. Contracts are serializable — the coordinator publishes `TaskAssigned` events with the contract embedded, and roles must emit `TaskCompleted` with output matching the contract.
- **New: Scheduler** — Enforces budgets per role invocation: wall-clock timeout, token consumption limit, retry count. Handles escalation routing: when a role publishes `EscalationRequested`, the scheduler determines the target role based on severity and registered escalation paths. Evaluates stop conditions to determine when a role's work is complete.
- **New: Retrieval Planner** — Stage-aware memory assembly. Given a role type and task context, queries the memory store for relevant memories filtered by scope, type, authority range, and recency. Different roles see different subsets of institutional knowledge — a Worker sees project memory, while a Scholar sees everything.
- **New: Runtime** — The main event loop that ties everything together. Boots the event bus and memory store, starts all registered roles as long-lived actors, dispatches events to roles, tracks role lifecycle, handles graceful shutdown, and publishes `OrganisationStarted` and `OrganisationStopped` lifecycle events.

## Capabilities

### New Capabilities

- `role-registry`: Registration of role types with typed contracts, authority scopes, default budgets, escalation targets, and event subscription filters. The registry is the source of truth for what roles exist in the organisation.
- `contract-system`: Typed `Contract<I, O>` structs with serializable input and output schemas, authority boundaries, and completion criteria. Every inter-role handoff uses contracts, not free-form messages.
- `scheduler`: Budget enforcement (wall-clock timeout, token limit, retry count), escalation routing based on severity and registered targets, stop condition evaluation, and role lifecycle state machine (idle → assigned → running → completed/failed/escalated).
- `retrieval-planner`: Stage-aware memory retrieval from the memory store. Given a `RoleId` and task context, returns a filtered set of memories appropriate for that role — different scopes, types, and authority ranges per role.
- `runtime`: Main event loop that boots the organisation, starts role actors, dispatches events, tracks lifecycle, and handles shutdown. The runtime is the single entry point for the organisation simulator.

### Modified Capabilities

None — third changeset in greenfield workspace.

## Impact

- **New crate**: `crates/coordinator/` added to workspace members
- **Dependencies**: `event-stream` (consumes/publishes events, uses EventBus and EventStore), `memory` (queries MemoryStore for retrieval planner), `serde`, `serde_json`, `tokio`, `uuid`, `thiserror`, `tracing`, `parking_lot`
- **Crate ordering**: Depends on `event-stream` and `memory`. Does NOT depend on `llm` or `process`.
- **Role implementations**: The coordinator knows about role types (their contracts, budgets, escalation paths) but does NOT implement their behavior. Role behavior lives in subsequent changesets (understanding, delivery, governance). The coordinator crate exports the `Role` trait that role implementations must satisfy.
