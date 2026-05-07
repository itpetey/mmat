## 1. Crate Setup

- [ ] 1.1 Scaffold `crates/coordinator/Cargo.toml` with dependencies (event-stream, memory, serde, serde_json, tokio with sync+macros+time+signal, uuid with serde+v4, thiserror, tracing, parking_lot, async-trait)
- [ ] 1.2 Add `crates/coordinator` to workspace members in root `Cargo.toml`
- [ ] 1.3 Create `src/lib.rs` with module declarations: `role`, `registry`, `contract`, `scheduler`, `retrieval`, `runtime`

## 2. Role Trait

- [ ] 2.1 Define `RoleType` enum (IntentLead, Scholar, OpsManager, Architect, ProjectManager, Worker, Reviewer, Auditor, Librarian) with Serialize/Deserialize
- [ ] 2.2 Define `RoleId` as a string-based newtype (e.g., `"worker-1"`, `"scholar"`) with Serialize/Deserialize, Clone, Display
- [ ] 2.3 Define `AuthorityScope` enum (IntentOnly, Architecture, Planning, Implementation, Review, Audit, FullAccess) with a `can_publish(event_type)` method
- [ ] 2.4 Define `RoleSpec` struct with fields: role_type, authority_scope, default_budget, escalation_paths (HashMap<Severity, RoleId>), input_contract (EventType), output_contract (Vec<EventType>)
- [ ] 2.5 Define `RoleLifecycleState` enum (Idle, Running, Completed, Failed, Escalated) with transition validation
- [ ] 2.6 Define `Role` trait with `async fn run(self: Arc<Self>, ctx: RoleContext) -> Result<(), RoleError>` using #[async_trait]
- [ ] 2.7 Define `RoleContext` struct holding EventBus sender, EventBus receiver for this role, MemoryStore handle, CoordinatorHandle, and role-specific ToolRegistry (generic over Tool type)
- [ ] 2.8 Define `RoleError` enum (Internal, BudgetExceeded, ContractViolation, EscalationRequired)
- [ ] 2.9 Define `CoordinatorHandle` for roles to report status and request escalation (using tokio::mpsc internally)

## 3. Role Registry

- [ ] 3.1 Implement `RoleRegistry` struct with `HashMap<RoleId, RoleSpec>` and `HashMap<EventType, Vec<RoleId>>` dispatch index
- [ ] 3.2 Implement `RoleRegistry::register(spec: RoleSpec)` — validates spec, checks for duplicate RoleId, builds dispatch index, validates escalation path contract compatibility
- [ ] 3.3 Implement `RoleRegistry::get(id: RoleId)` — returns `Option<&RoleSpec>`
- [ ] 3.4 Implement `RoleRegistry::get_by_type(role_type: RoleType)` — returns `Vec<&RoleSpec>`
- [ ] 3.5 Implement `RoleRegistry::subscribers_for(event_type: EventType)` — returns roles that subscribe to this event type
- [ ] 3.6 Implement `RoleRegistry::escalation_target(role_id: RoleId, severity: Severity)` — walks escalation paths, falling back to higher severities if no exact match
- [ ] 3.7 Implement contract compatibility validation: when role A escalates to role B, B's input_contract must include the escalation event type

## 4. Contract System

- [ ] 4.1 Define `ContractId` as `uuid::Uuid` newtype with Serialize/Deserialize
- [ ] 4.2 Define `Contract<I, O>` generic struct with fields: contract_id, input_schema (type name string), output_schema (type name string), authority_scope, completion_criteria, max_retries, retrieval_override (optional RetrievalProfile)
- [ ] 4.3 Define `CompletionCriteria` enum (AllChecksPassed, ArtefactProduced, HumanApproved, Timeout(Duration))
- [ ] 4.4 Implement `Contract::new(...)` builder with default values and validation
- [ ] 4.5 Implement `Contract::is_satisfied(&self, events: &[SemanticEvent]) -> bool` — evaluates completion criteria against a set of events
- [ ] 4.6 Define `TaskContext` struct carrying contract_id, source_task_event, and any accumulated events during execution

## 5. Scheduler

- [ ] 5.1 Implement `Scheduler` struct with per-role state tracking (HashMap<RoleId, RoleLifecycleState>), task tracker (HashMap<ContractId, TaskContext>), and budget tracker (HashMap<ContractId, BudgetState>)
- [ ] 5.2 Define `BudgetState` struct with time_budget (Instant started + Duration limit), token_budget (u64 used, u64 limit), retry_count (u32 current, u32 max)
- [ ] 5.3 Implement `Scheduler::run(bus)` actor loop — subscribes to TaskAssigned, TaskStarted, TaskCompleted, TaskFailed, EscalationRequested, ToolExecuted events
- [ ] 5.4 Implement budget monitoring: spawn a tokio::time::interval task that checks all active BudgetStates for timeout and token overrun
- [ ] 5.5 Implement timeout enforcement: if time elapsed > budget, publish TaskFailed
- [ ] 5.6 Implement token enforcement: sum tokens from ToolExecuted events within a task, publish BudgetWarning at 80%, publish TaskFailed when exceeded
- [ ] 5.7 Implement retry logic: on TaskFailed, if retry_count < max_retries, republish TaskAssigned with incremented retry metadata
- [ ] 5.8 Implement retry exhaustion: when retry_count == max_retries, escalate instead of retrying
- [ ] 5.9 Implement escalation handling: consume EscalationRequested, look up target from registry, publish TaskAssigned to target with escalation context
- [ ] 5.10 Implement EscalationAccepted event publishing with chain depth tracking
- [ ] 5.11 Implement role lifecycle tracking: update state on TaskAssigned (Idle→Running), TaskCompleted (Running→Completed), TaskFailed (Running→Failed/Escalated)
- [ ] 5.12 Publish RoleStateChanged events on state transitions
- [ ] 5.13 Implement heartbeat monitoring for dead role detection (no events within timeout → mark Failed)

## 6. Retrieval Planner

- [ ] 6.1 Define `RetrievalProfile` struct with fields: allowed_scopes (Vec<MemoryScope>), allowed_types (Vec<MemoryType>), min_authority (Authority), max_age (Option<Duration>), result_limit (usize)
- [ ] 6.2 Define default retrieval profiles per role type:
  - Worker: scopes=[Project], types=[Constraint,Decision,Fact,SOP], min_authority=ReviewFindings
  - Scholar: scopes=all, types=all, min_authority=SpeculativeReasoning
  - Architect: scopes=[Project,Organisational], types=[Decision,Constraint,Risk,Lesson], min_authority=LLMInference
  - PM: scopes=[Project], types=[Constraint,Decision,Fact,Risk], min_authority=ReviewFindings
  - Reviewer: scopes=[Project,Organisational], types=[SOP,Constraint,Decision], min_authority=ReviewFindings
  - Auditor: scopes=all, types=[ClaimMade-equivalent], min_authority=CompilerOutput
  - IntentLead: scopes=[Project], types=[Preference,Constraint,OpenQuestion], min_authority=UserInstruction
  - OpsManager: scopes=[Organisational], types=[SOP,Lesson,Incident], min_authority=AcceptedADR
  - Librarian: scopes=all, types=all, min_authority=SpeculativeReasoning
- [ ] 6.3 Implement `RetrievalPlanner::retrieve(&self, memory_store: &MemoryStore, profile: &RetrievalProfile, task_context: &str) -> Vec<Memory>`
- [ ] 6.4 Implement structured query: apply scope filter, type filter, authority filter, age filter via MemoryStore methods
- [ ] 6.5 Implement semantic query: if task_context is non-empty, call MemoryStore::search_similar with embedded query text
- [ ] 6.6 Implement result merging: combine structured and semantic results, deduplicate by MemoryId, sort by relevance (semantic score > recency), apply result_limit

## 7. Runtime

- [ ] 7.1 Define `OrganisationConfig` struct with event_bus_capacity, heartbeat_interval, shutdown_grace_period, event_store_path, memory_store_path
- [ ] 7.2 Implement `OrganisationRuntime` struct holding EventBus, EventStore, MemoryStore, RoleRegistry, Scheduler, RetrievalPlanner
- [ ] 7.3 Implement `OrganisationRuntime::new(config, registry)` — initialises all components in order, validates registry
- [ ] 7.4 Implement `OrganisationRuntime::run(self)` — the main entry point: publish OrganisationStarted, spawn all role actors, enter main event loop, handle shutdown
- [ ] 7.5 Implement role spawning: for each registered role, create a RoleContext with bus subscription, spawn as tokio::task
- [ ] 7.6 Implement main event loop: listen for shutdown signal (tokio::signal::ctrl_c), publish periodic Heartbeat events, track role task JoinHandles
- [ ] 7.7 Implement Heartbeat event with active/completed/failed role counts
- [ ] 7.8 Implement graceful shutdown: on signal, stop accepting new tasks, wait for running tasks up to grace_period, publish OrganisationStopped, flush event store, abort remaining tasks
- [ ] 7.9 Implement startup replay: on restart, replay events from event store to rebuild scheduler state (RoleStateChanged events) and budget state

## 8. Integration

- [ ] 8.1 Write integration test: register a mock role, start runtime, publish TaskAssigned → mock role receives event → publishes TaskCompleted → scheduler updates state
- [ ] 8.2 Write integration test: task exceeds time budget → scheduler publishes TaskFailed → retry count incremented → on exhaustion, escalation triggered
- [ ] 8.3 Write integration test: role escalates → escalation target receives TaskAssigned with context
- [ ] 8.4 Write integration test: retrieval planner returns different results for Worker vs Scholar with same task context
- [ ] 8.5 Write integration test: runtime restart from event store recovers role states correctly

## 9. Validation

- [ ] 9.1 `cargo fmt --all` passes
- [ ] 9.2 `cargo clippy -- -D warnings` passes on all crates
- [ ] 9.3 `cargo test` passes all tests including doc tests
