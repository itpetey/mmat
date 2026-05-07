## Context

The event stream delivers structured events and the memory core provides durable knowledge. But without deterministic coordination, the system is just roles emitting events into a void — no contracts, no budgets, no escalation, no lifecycle. The coordinator is the governance layer. It does not implement role behaviour — it governs the organisation by ensuring every role invocation has a typed contract, an authority scope, a resource budget, and a defined escalation path.

The conversation design is explicit: "The coordinator is crucial. Do not let roles freely chat forever. Give them: input contract, output schema, authority limits, budget, stop condition, escalation path." This is the "organisation simulator with deterministic governance and auditable artefacts."

## Goals / Non-Goals

**Goals:**
- Define a `Role` trait that every role must implement — identity, subscriptions, lifecycle
- Provide a `RoleRegistry` cataloguing all role types with their contracts, budgets, and escalation paths
- Define typed `Contract<I, O>` structs for inter-role handoffs
- Implement budget enforcement: wall-clock timeouts, token limits, retry counts
- Implement escalation routing: when a role escalates, determine the target based on severity and registered paths
- Provide a retrieval planner that assembles stage-appropriate memory context for each role
- Implement the main runtime: boots the organisation, starts roles, dispatches events, tracks lifecycle

**Non-Goals:**
- Role behaviour implementations (Intent Lead, Scholar, etc.) — those are separate changesets
- LLM invocation or tool calling — roles call LLM/tools themselves, the coordinator doesn't mediate it
- Human interaction — the Intent Lead role handles that
- Distributed coordination — v1 is single-process

## Decisions

### Decision 1: Role trait as actor contract, not function contract

**Chosen**: The `Role` trait exposes a `run()` method that the coordinator calls once at startup. The role runs its own internal loop, consuming from its event bus subscription and publishing results. The coordinator does NOT call `role.execute(input)` per task.

**Rationale**: Roles are long-lived actors, not stateless functions. A Worker may handle dozens of task assignments over its lifetime. The coordinator shouldn't micro-manage each task invocation — it governs at the role level (budgets, escalation) and lets the role manage its own work loop.

```rust
#[async_trait]
pub trait Role: Send + Sync + 'static {
    fn id(&self) -> RoleId;
    fn spec(&self) -> RoleSpec;
    fn subscriptions(&self) -> &'static [EventType];

    async fn run(
        self: Arc<Self>,
        ctx: RoleContext,
    ) -> Result<(), RoleError>;
}
```

**Alternative considered**: Synchronous `execute(input) -> output` per task. Rejected because roles need to maintain state across invocations (the Scholar accumulates knowledge, the Ops Manager evolves SOPs). A per-task function model would force all state through the memory store, which is too slow for operational state.

### Decision 2: Escalation as event-driven, not direct function call

**Chosen**: A role publishes an `EscalationRequested` event with a severity and reason. The coordinator's scheduler picks up the event, looks up the escalation path registered for that role+severity combination, and publishes a `TaskAssigned` event targeting the escalation recipient.

**Rationale**: This keeps roles decoupled — a Worker never needs to know who handles its escalations. The coordinator is the single point of routing. It also makes escalation auditable: every escalation is an event in the stream with full provenance.

### Decision 3: Budgets enforced at the coordinator, not the role

**Chosen**: The coordinator tracks per-role budgets (wall time, token count) via events. When a role exceeds its budget, the coordinator publishes a `TaskFailed` event with reason "budget exceeded". Roles are not required to self-enforce budgets.

**Rationale**: Trust the coordinator, not the role. A misbehaving LLM-driven role might ignore its own budget. The coordinator has access to the event stream and can observe when a `TaskStarted` event was published without a corresponding `TaskCompleted` within the time budget.

**Trade-off**: Token counting requires the coordinator to observe tool execution events and sum token usage from completion responses. This couples the coordinator to the LLM event format but is necessary for enforcement.

### Decision 4: Retrieval planner as a stateless function, not a role

**Chosen**: The retrieval planner is a module within the coordinator crate, not a separate role. It's a pure function of `(role_id, task_context) -> Vec<Memory>`. Roles call it when they need context. No event subscription needed.

**Rationale**: Memory retrieval is a synchronous query against the memory store. It doesn't need an event loop, subscription, or lifecycle. Making it a role would add overhead for no benefit. The retrieval planner is infrastructure, like the event store — roles consume it directly.

### Decision 5: Role lifecycle state machine with auto-restart

**Chosen**: Failed roles auto-restart up to N times (default 3, configurable per role type). On the Nth failure, the coordinator escalates to the human via `HumanFeedbackRequested` rather than restarting again.

**Rationale**: Autonomous recovery for transient failures (e.g., LLM API timeout, tool execution error). Human intervention for persistent failures (e.g., misconfigured role, systemic bug). The retry count resets on successful task completion — only consecutive failures trigger escalation.

**Chosen**: Roles transition through a defined state machine: `Idle → Running → Completed | Failed | Escalated`. State transitions are recorded as events in the stream. The coordinator tracks the current state for each role instance in memory (rebuilt from events on restart).

```
    Idle ──► Running ──► Completed
                │
                ├──► Failed (retryable) ──► Running
                │
                └──► Escalated
```

**Rationale**: This is simpler than a full task state machine (which would be per-task-card). Each role invocation (triggered by a `TaskAssigned` event) is a task instance. The role lifecycle tracks the role itself, not individual tasks.

## Risks / Trade-offs

- **[Risk] Coordinator becomes a bottleneck** → Mitigation: The coordinator only routes events and enforces budgets — it doesn't process role logic. If throughput becomes an issue, budget enforcement can be offloaded to a separate task.
- **[Risk] Dead roles don't report failure** → Mitigation: The coordinator monitors heartbeat events. If a role hasn't published any event within its timeout window, the coordinator marks it as Failed and restarts it.
- **[Risk] Token counting is approximate** → Mitigation: Tokens are counted from completion response `usage` fields. Some providers report approximate counts. The budget uses a generous margin (1.2× the stated limit) to avoid false positives.
- **[Trade-off] Single coordinator (no consensus)** → v1 is single-process so this is correct. Multi-process would require leader election or sharding.

## Resolved Questions

- **Failed role restart**: Auto-restart up to N times (default 3, configurable per role type), then escalate to human via `HumanFeedbackRequested`. Balances autonomy with safety.
- **Retrieval planner caching**: No cache — always query the memory store fresh. Memory may be superseded mid-task. SQLite index scans are fast enough that caching isn't needed. Correctness over speed.
