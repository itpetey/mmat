use std::{sync::Arc, time::Duration};

use mmat_coordinator::{
    AuthorityScope, Budget, OrganisationConfig, OrganisationRuntime, RetrievalPlanner, Role,
    RoleContext, RoleError, RoleLifecycleState, RoleRegistry, RoleSpec, RoleType, Severity,
};
use mmat_db::AsyncPgConnection;
use mmat_event_stream::event::{ArtefactRef, EventType, RoleId, SemanticEvent, TaskContract};
use mmat_memory::{
    error::Result as MemoryResult,
    store::MemoryStore,
    types::{Authority, Confidence, Memory, MemoryId, MemoryScope, MemoryType},
    vector_backend::VectorMemoryBackend,
};
use qdrant_client::qdrant::Value;

type PgPool = mmat_db::Pool<AsyncPgConnection>;

const CONTRACT_1: &str = "00000000-0000-0000-0000-000000000001";
const CONTRACT_ESCALATION: &str = "00000000-0000-0000-0000-000000000003";
const CONTRACT_RETRY: &str = "00000000-0000-0000-0000-000000000005";
const CONTRACT_TIMEOUT: &str = "00000000-0000-0000-0000-000000000002";
const CONTRACT_TOKEN: &str = "00000000-0000-0000-0000-000000000006";
const CONTRACT_VIOLATION: &str = "00000000-0000-0000-0000-000000000007";

struct MockRole {
    id: RoleId,
    spec: RoleSpec,
}

struct SlowMockRole {
    id: RoleId,
    spec: RoleSpec,
}

struct EscalatingRole {
    id: RoleId,
    spec: RoleSpec,
}

struct FailingRole {
    id: RoleId,
    spec: RoleSpec,
}

struct TokenHungryRole {
    id: RoleId,
    spec: RoleSpec,
}

struct FakeVectorBackend {
    results: Vec<(MemoryId, f32)>,
}

impl MockRole {
    fn new(id: impl Into<String>, spec: RoleSpec) -> Self {
        Self {
            id: RoleId::new(id),
            spec,
        }
    }
}

#[async_trait::async_trait]
impl Role for MockRole {
    fn id(&self) -> RoleId {
        self.id.clone()
    }

    fn spec(&self) -> RoleSpec {
        self.spec.clone()
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::TaskAssigned]
    }

    async fn run(self: Arc<Self>, mut ctx: RoleContext) -> std::result::Result<(), RoleError> {
        while let Ok(event) = ctx.receiver.recv().await {
            if let SemanticEvent::TaskAssigned {
                task_id,
                worker_id,
                contract_ref,
                ..
            } = event.as_ref()
                && worker_id == &self.id
            {
                let _ = ctx.bus.publish(SemanticEvent::new_task_started(
                    self.id.clone(),
                    task_id.clone(),
                    self.id.clone(),
                ));
                let _ = ctx.bus.publish(SemanticEvent::new_task_completed(
                    self.id.clone(),
                    task_id.clone(),
                    contract_ref.contract_id.clone(),
                    ArtefactRef {
                        artefact_type: "test".into(),
                        reference: "done".into(),
                    },
                ));
            }
        }
        Ok(())
    }
}

impl SlowMockRole {
    fn new(id: impl Into<String>, spec: RoleSpec) -> Self {
        Self {
            id: RoleId::new(id),
            spec,
        }
    }
}

#[async_trait::async_trait]
impl Role for SlowMockRole {
    fn id(&self) -> RoleId {
        self.id.clone()
    }

    fn spec(&self) -> RoleSpec {
        self.spec.clone()
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::TaskAssigned]
    }

    async fn run(self: Arc<Self>, mut ctx: RoleContext) -> std::result::Result<(), RoleError> {
        while let Ok(event) = ctx.receiver.recv().await {
            if let SemanticEvent::TaskAssigned {
                task_id, worker_id, ..
            } = event.as_ref()
                && worker_id == &self.id
            {
                // Publish TaskStarted but never TaskCompleted, so budget will timeout
                let _ = ctx.bus.publish(SemanticEvent::new_task_started(
                    self.id.clone(),
                    task_id.clone(),
                    self.id.clone(),
                ));
                // Sleep indefinitely to force timeout
                tokio::time::sleep(Duration::from_secs(600)).await;
            }
        }
        Ok(())
    }
}

impl EscalatingRole {
    fn new(id: impl Into<String>, spec: RoleSpec) -> Self {
        Self {
            id: RoleId::new(id),
            spec,
        }
    }
}

#[async_trait::async_trait]
impl Role for EscalatingRole {
    fn id(&self) -> RoleId {
        self.id.clone()
    }

    fn spec(&self) -> RoleSpec {
        self.spec.clone()
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::TaskAssigned]
    }

    async fn run(self: Arc<Self>, mut ctx: RoleContext) -> std::result::Result<(), RoleError> {
        while let Ok(event) = ctx.receiver.recv().await {
            if let SemanticEvent::TaskAssigned { worker_id, .. } = event.as_ref()
                && worker_id == &self.id
            {
                let _ = ctx
                    .coordinator
                    .request_escalation(self.id.clone(), Severity::Medium, "test escalation")
                    .await;
            }
        }
        Ok(())
    }
}

impl FailingRole {
    fn new(id: impl Into<String>, spec: RoleSpec) -> Self {
        Self {
            id: RoleId::new(id),
            spec,
        }
    }
}

#[async_trait::async_trait]
impl Role for FailingRole {
    fn id(&self) -> RoleId {
        self.id.clone()
    }

    fn spec(&self) -> RoleSpec {
        self.spec.clone()
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::TaskAssigned]
    }

    async fn run(self: Arc<Self>, mut ctx: RoleContext) -> std::result::Result<(), RoleError> {
        while let Ok(event) = ctx.receiver.recv().await {
            if let SemanticEvent::TaskAssigned {
                worker_id,
                contract_ref,
                ..
            } = event.as_ref()
                && worker_id == &self.id
            {
                let _ = ctx.bus.publish(SemanticEvent::new_task_failed(
                    self.id.clone(),
                    contract_ref.contract_id.clone(),
                    "intentional failure",
                ));
            }
        }
        Ok(())
    }
}

impl TokenHungryRole {
    fn new(id: impl Into<String>, spec: RoleSpec) -> Self {
        Self {
            id: RoleId::new(id),
            spec,
        }
    }
}

#[async_trait::async_trait]
impl Role for TokenHungryRole {
    fn id(&self) -> RoleId {
        self.id.clone()
    }

    fn spec(&self) -> RoleSpec {
        self.spec.clone()
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::TaskAssigned]
    }

    async fn run(self: Arc<Self>, mut ctx: RoleContext) -> std::result::Result<(), RoleError> {
        while let Ok(event) = ctx.receiver.recv().await {
            if let SemanticEvent::TaskAssigned { worker_id, .. } = event.as_ref()
                && worker_id == &self.id
            {
                let _ = ctx.bus.publish(SemanticEvent::new_tool_executed(
                    self.id.clone(),
                    "llm",
                    "{}",
                    0,
                    "",
                    "",
                    10_000,
                ));
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl VectorMemoryBackend for FakeVectorBackend {
    async fn upsert(
        &self,
        _id: MemoryId,
        _embedding: Vec<f32>,
        _payload: std::collections::HashMap<String, Value>,
    ) -> MemoryResult<()> {
        Ok(())
    }

    async fn search(
        &self,
        _query_embedding: Vec<f32>,
        limit: u64,
    ) -> MemoryResult<Vec<(MemoryId, f32)>> {
        Ok(self.results.iter().copied().take(limit as usize).collect())
    }

    async fn delete(&self, _id: MemoryId) -> MemoryResult<()> {
        Ok(())
    }
}

fn test_config() -> Option<OrganisationConfig> {
    Some(OrganisationConfig {
        event_bus_capacity: 128,
        heartbeat_interval: Duration::from_secs(30),
        shutdown_grace_period: Duration::from_secs(2),
        database_url: std::env::var("MMAT_DB_URL").ok()?,
        host_work_dir: None,
    })
}

async fn test_runtime(
    config: OrganisationConfig,
    registry: RoleRegistry,
) -> Option<OrganisationRuntime> {
    OrganisationRuntime::new(config, registry).ok()
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

async fn postgres_test_database(prefix: &str) -> Option<(PgPool, String)> {
    let base_url = std::env::var("MMAT_DB_URL").ok()?;
    let schema = format!("{}_{}", prefix, now_nanos());
    let admin_pool = mmat_db::new_pool(&base_url).await.ok()?;
    let mut conn = admin_pool.get().await.ok()?;
    mmat_db::execute_sql(&mut conn, &format!("CREATE SCHEMA \"{schema}\""))
        .await
        .ok()?;
    let separator = if base_url.contains('?') { '&' } else { '?' };
    let database_url = format!("{base_url}{separator}options=-c%20search_path%3D{schema}");
    let pool = mmat_db::new_pool(&database_url).await.ok()?;
    let migrate_pool = pool.clone();
    let mut migrator_conn = migrate_pool.get().await.ok()?;
    mmat_db::execute_sql(
        &mut migrator_conn,
        include_str!("../../db/migrations/2026-05-14-000001_init/up.sql"),
    )
    .await
    .ok()?;
    Some((pool, schema))
}

async fn drop_postgres_schema(pool: &PgPool, schema: &str) {
    if let Ok(mut conn) = pool.get().await {
        let _ = mmat_db::execute_sql(
            &mut conn,
            &format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"),
        )
        .await;
    }
}

#[tokio::test]
async fn test_escalation_routing() {
    let Some(config) = test_config() else {
        return;
    };
    let mut registry = RoleRegistry::new();

    let mut worker_spec = worker_spec();
    worker_spec
        .escalation_paths
        .insert(Severity::Medium, RoleId::new("reviewer"));
    registry.register(worker_spec.clone()).unwrap();

    let reviewer_spec = RoleSpec {
        id: RoleId::new("reviewer"),
        role_type: RoleType::Reviewer,
        authority_scope: AuthorityScope::Review,
        default_budget: Budget::default(),
        escalation_paths: std::collections::HashMap::new(),
        input_contract: EventType::TaskAssigned,
        output_contract: vec![
            EventType::TaskStarted,
            EventType::TaskCompleted,
            EventType::ReviewCompleted,
        ],
    };
    registry.register(reviewer_spec.clone()).unwrap();

    let Some(mut runtime) = test_runtime(config.clone(), registry).await else {
        return;
    };
    runtime.add_role(EscalatingRole::new("worker", worker_spec));
    runtime.add_role(MockRole::new("reviewer", reviewer_spec));

    let bus = runtime.bus().clone();
    let scheduler = runtime.scheduler().clone();
    let shutdown_tx = runtime.shutdown_handle();
    let pool = mmat_db::new_pool(&config.database_url).await.unwrap();
    let handle = tokio::spawn(async move { runtime.run(&pool).await });

    tokio::time::sleep(Duration::from_millis(300)).await;

    bus.publish(SemanticEvent::new_task_assigned(
        RoleId::new("test"),
        "task-escalation",
        RoleId::new("worker"),
        TaskContract {
            contract_id: CONTRACT_ESCALATION.into(),
            description: "escalation test".into(),
        },
        vec![],
    ))
    .unwrap();

    // Wait for escalation to be processed
    tokio::time::sleep(Duration::from_millis(1200)).await;

    // Verify reviewer received the escalated task and completed it
    let scheduler_guard = scheduler.lock().await;
    let reviewer_state = scheduler_guard.get_role_state(&RoleId::new("reviewer"));
    assert!(
        matches!(reviewer_state, RoleLifecycleState::Completed),
        "expected reviewer to be Completed after escalation, got {:?}",
        reviewer_state
    );
    drop(scheduler_guard);

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}

#[tokio::test]
async fn test_mock_role_lifecycle() {
    let Some(config) = test_config() else {
        return;
    };
    let mut registry = RoleRegistry::new();
    registry.register(worker_spec()).unwrap();

    let Some(mut runtime) = test_runtime(config.clone(), registry).await else {
        return;
    };
    runtime.add_role(MockRole::new("worker", worker_spec()));

    // Spawn runtime in background
    let bus = runtime.bus().clone();
    let scheduler = runtime.scheduler().clone();
    let shutdown_tx = runtime.shutdown_handle();
    let pool = mmat_db::new_pool(&config.database_url).await.unwrap();
    let handle = tokio::spawn(async move { runtime.run(&pool).await });

    // Give runtime time to start roles
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Publish TaskAssigned
    bus.publish(SemanticEvent::new_task_assigned(
        RoleId::new("test"),
        "task-1",
        RoleId::new("worker"),
        TaskContract {
            contract_id: CONTRACT_1.into(),
            description: "test task".into(),
        },
        vec![],
    ))
    .unwrap();

    // Wait for role to process
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Check scheduler state
    let scheduler_guard = scheduler.lock().await;
    let state = scheduler_guard.get_role_state(&RoleId::new("worker"));
    assert!(
        matches!(state, RoleLifecycleState::Completed),
        "expected role to be Completed, got {:?}",
        state
    );
    drop(scheduler_guard);

    // Graceful shutdown
    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}

#[tokio::test]
async fn test_output_contract_violation_marks_role_failed() {
    let Some(config) = test_config() else {
        return;
    };
    let mut registry = RoleRegistry::new();

    let mut spec = worker_spec();
    spec.output_contract = vec![EventType::TaskStarted];
    registry.register(spec.clone()).unwrap();

    let Some(mut runtime) = test_runtime(config.clone(), registry).await else {
        return;
    };
    runtime.add_role(MockRole::new("worker", spec));

    let bus = runtime.bus().clone();
    let scheduler = runtime.scheduler().clone();
    let shutdown_tx = runtime.shutdown_handle();
    let pool = mmat_db::new_pool(&config.database_url).await.unwrap();
    let handle = tokio::spawn(async move { runtime.run(&pool).await });

    tokio::time::sleep(Duration::from_millis(300)).await;
    bus.publish(SemanticEvent::new_task_assigned(
        RoleId::new("test"),
        "task-violation",
        RoleId::new("worker"),
        TaskContract {
            contract_id: CONTRACT_VIOLATION.into(),
            description: "contract violation".into(),
        },
        vec![],
    ))
    .unwrap();

    tokio::time::sleep(Duration::from_millis(600)).await;
    let scheduler_guard = scheduler.lock().await;
    assert!(matches!(
        scheduler_guard.get_role_state(&RoleId::new("worker")),
        RoleLifecycleState::Failed
    ));
    drop(scheduler_guard);

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}

#[tokio::test]
async fn test_retrieval_planner_profiles() {
    let Some((pool, schema)) = postgres_test_database("retrieval_planner").await else {
        return;
    };
    let store = MemoryStore::new_with_pool(pool.clone());

    // Insert memories of different scopes and types
    let project_fact = Memory::builder()
        .memory_type(MemoryType::Fact)
        .content("Project fact")
        .scope(MemoryScope::Project)
        .authority(Authority::ReviewFindings)
        .confidence(Confidence::new(0.9).unwrap())
        .source_agent(RoleId::new("test"))
        .build()
        .unwrap();

    let org_lesson = Memory::builder()
        .memory_type(MemoryType::Lesson)
        .content("Organisational lesson")
        .scope(MemoryScope::Organisational)
        .authority(Authority::AcceptedADR)
        .confidence(Confidence::new(0.9).unwrap())
        .source_agent(RoleId::new("test"))
        .build()
        .unwrap();

    store.insert(&project_fact).unwrap();
    store.insert(&org_lesson).unwrap();

    let planner = RetrievalPlanner::new();

    let worker_profile = mmat_coordinator::default_profile_for_role_type(RoleType::Worker);
    let worker_results = planner.retrieve(&store, &worker_profile, "");
    assert!(
        worker_results.iter().any(|m| m.content == "Project fact"),
        "Worker should see project facts"
    );
    assert!(
        !worker_results
            .iter()
            .any(|m| m.content == "Organisational lesson"),
        "Worker should NOT see organisational lessons"
    );

    let scholar_profile = mmat_coordinator::default_profile_for_role_type(RoleType::Scholar);
    let scholar_results = planner.retrieve(&store, &scholar_profile, "");
    assert!(
        scholar_results.iter().any(|m| m.content == "Project fact"),
        "Scholar should see project facts"
    );
    assert!(
        scholar_results
            .iter()
            .any(|m| m.content == "Organisational lesson"),
        "Scholar should see organisational lessons"
    );

    drop_postgres_schema(&pool, &schema).await;
}

#[tokio::test]
async fn test_retrieval_semantic_search() {
    let Some((pool, schema)) = postgres_test_database("retrieval_search").await else {
        return;
    };
    let store = MemoryStore::new_with_pool(pool.clone());

    let project_fact = Memory::builder()
        .memory_type(MemoryType::Fact)
        .content("Database migration patterns for large tables")
        .scope(MemoryScope::Project)
        .authority(Authority::ReviewFindings)
        .confidence(Confidence::new(0.9).unwrap())
        .source_agent(RoleId::new("test"))
        .build()
        .unwrap();

    let unrelated = Memory::builder()
        .memory_type(MemoryType::Fact)
        .content("Frontend component styling guide")
        .scope(MemoryScope::Project)
        .authority(Authority::ReviewFindings)
        .confidence(Confidence::new(0.9).unwrap())
        .source_agent(RoleId::new("test"))
        .build()
        .unwrap();

    store.insert(&project_fact).unwrap();
    store.insert(&unrelated).unwrap();

    let planner = RetrievalPlanner::new();
    let profile = mmat_coordinator::default_profile_for_role_type(RoleType::Worker);
    let qdrant = FakeVectorBackend {
        results: vec![(project_fact.id, 0.99), (unrelated.id, 0.10)],
    };

    let results = planner
        .retrieve_async(&store, &profile, "database migration", Some(&qdrant))
        .await;
    assert_eq!(
        results.first().map(|m| m.content.as_str()),
        Some("Database migration patterns for large tables"),
        "semantic result should be ranked before structured fallback results"
    );
    assert_eq!(
        results.len(),
        2,
        "structured and semantic results should merge"
    );

    drop_postgres_schema(&pool, &schema).await;
}

#[tokio::test]
async fn test_retry_exhaustion_escalates() {
    let Some(config) = test_config() else {
        return;
    };
    let mut registry = RoleRegistry::new();

    let mut worker_spec = worker_spec();
    worker_spec.default_budget.max_retries = 1;
    worker_spec
        .escalation_paths
        .insert(Severity::High, RoleId::new("reviewer"));
    registry.register(worker_spec.clone()).unwrap();

    let reviewer_spec = RoleSpec {
        id: RoleId::new("reviewer"),
        role_type: RoleType::Reviewer,
        authority_scope: AuthorityScope::Review,
        default_budget: Budget::default(),
        escalation_paths: std::collections::HashMap::new(),
        input_contract: EventType::TaskAssigned,
        output_contract: vec![EventType::TaskStarted, EventType::TaskCompleted],
    };
    registry.register(reviewer_spec.clone()).unwrap();

    let Some(mut runtime) = test_runtime(config.clone(), registry).await else {
        return;
    };
    runtime.add_role(FailingRole::new("worker", worker_spec));
    runtime.add_role(MockRole::new("reviewer", reviewer_spec));

    let bus = runtime.bus().clone();
    let scheduler = runtime.scheduler().clone();
    let shutdown_tx = runtime.shutdown_handle();
    let pool = mmat_db::new_pool(&config.database_url).await.unwrap();
    let handle = tokio::spawn(async move { runtime.run(&pool).await });

    tokio::time::sleep(Duration::from_millis(300)).await;
    bus.publish(SemanticEvent::new_task_assigned(
        RoleId::new("test"),
        "task-retry",
        RoleId::new("worker"),
        TaskContract {
            contract_id: CONTRACT_RETRY.into(),
            description: "retry exhaustion".into(),
        },
        vec![],
    ))
    .unwrap();

    let mut worker_state = RoleLifecycleState::Running;
    let mut reviewer_state = RoleLifecycleState::Idle;
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let scheduler_guard = scheduler.lock().await;
        worker_state = scheduler_guard.get_role_state(&RoleId::new("worker"));
        reviewer_state = scheduler_guard.get_role_state(&RoleId::new("reviewer"));
        if matches!(worker_state, RoleLifecycleState::Escalated)
            && matches!(reviewer_state, RoleLifecycleState::Completed)
        {
            break;
        }
    }
    assert!(
        matches!(worker_state, RoleLifecycleState::Escalated),
        "expected worker to be Escalated, got {worker_state:?}"
    );
    assert!(
        matches!(reviewer_state, RoleLifecycleState::Completed),
        "expected reviewer to be Completed, got {reviewer_state:?}"
    );

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}

#[tokio::test]
async fn test_time_budget_enforcement() {
    let Some(config) = test_config() else {
        return;
    };
    let mut registry = RoleRegistry::new();
    let mut spec = worker_spec();
    spec.default_budget.time_limit_seconds = 1;
    registry.register(spec.clone()).unwrap();

    let Some(mut runtime) = test_runtime(config.clone(), registry).await else {
        return;
    };
    runtime.add_role(SlowMockRole::new("worker", spec.clone()));

    let bus = runtime.bus().clone();
    let scheduler = runtime.scheduler().clone();
    let shutdown_tx = runtime.shutdown_handle();
    let pool = mmat_db::new_pool(&config.database_url).await.unwrap();
    let handle = tokio::spawn(async move { runtime.run(&pool).await });

    tokio::time::sleep(Duration::from_millis(300)).await;

    bus.publish(SemanticEvent::new_task_assigned(
        RoleId::new("test"),
        "task-timeout",
        RoleId::new("worker"),
        TaskContract {
            contract_id: CONTRACT_TIMEOUT.into(),
            description: "will timeout".into(),
        },
        vec![],
    ))
    .unwrap();

    // Wait for budget monitor to detect timeout and process
    let mut state = RoleLifecycleState::Running;
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let scheduler_guard = scheduler.lock().await;
        state = scheduler_guard.get_role_state(&RoleId::new("worker"));
        if matches!(state, RoleLifecycleState::Failed) {
            break;
        }
    }
    assert!(
        matches!(state, RoleLifecycleState::Failed),
        "expected role to be Failed after timeout, got {:?}",
        state
    );

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}

#[tokio::test]
async fn test_token_budget_exhaustion_escalates() {
    let Some(config) = test_config() else {
        return;
    };
    let mut registry = RoleRegistry::new();

    let mut worker_spec = worker_spec();
    worker_spec.default_budget.token_limit = 1;
    worker_spec.default_budget.max_retries = 1;
    worker_spec
        .escalation_paths
        .insert(Severity::High, RoleId::new("reviewer"));
    registry.register(worker_spec.clone()).unwrap();

    let reviewer_spec = RoleSpec {
        id: RoleId::new("reviewer"),
        role_type: RoleType::Reviewer,
        authority_scope: AuthorityScope::Review,
        default_budget: Budget::default(),
        escalation_paths: std::collections::HashMap::new(),
        input_contract: EventType::TaskAssigned,
        output_contract: vec![EventType::TaskStarted, EventType::TaskCompleted],
    };
    registry.register(reviewer_spec.clone()).unwrap();

    let Some(mut runtime) = test_runtime(config.clone(), registry).await else {
        return;
    };
    runtime.add_role(TokenHungryRole::new("worker", worker_spec));
    runtime.add_role(MockRole::new("reviewer", reviewer_spec));

    let bus = runtime.bus().clone();
    let scheduler = runtime.scheduler().clone();
    let shutdown_tx = runtime.shutdown_handle();
    let pool = mmat_db::new_pool(&config.database_url).await.unwrap();
    let handle = tokio::spawn(async move { runtime.run(&pool).await });

    tokio::time::sleep(Duration::from_millis(300)).await;
    bus.publish(SemanticEvent::new_task_assigned(
        RoleId::new("test"),
        "task-token",
        RoleId::new("worker"),
        TaskContract {
            contract_id: CONTRACT_TOKEN.into(),
            description: "token exhaustion".into(),
        },
        vec![],
    ))
    .unwrap();

    // Wait for budget monitor to detect token overrun and process
    let mut worker_state = RoleLifecycleState::Running;
    let mut reviewer_state = RoleLifecycleState::Idle;
    for _ in 0..15 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let scheduler_guard = scheduler.lock().await;
        worker_state = scheduler_guard.get_role_state(&RoleId::new("worker"));
        reviewer_state = scheduler_guard.get_role_state(&RoleId::new("reviewer"));
        if matches!(worker_state, RoleLifecycleState::Escalated)
            && matches!(reviewer_state, RoleLifecycleState::Completed)
        {
            break;
        }
    }
    assert!(
        matches!(worker_state, RoleLifecycleState::Escalated),
        "expected worker to be Escalated, got {worker_state:?}"
    );
    assert!(
        matches!(reviewer_state, RoleLifecycleState::Completed),
        "expected reviewer to be Completed, got {reviewer_state:?}"
    );

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}

fn worker_spec() -> RoleSpec {
    RoleSpec {
        id: RoleId::new("worker"),
        role_type: RoleType::Worker,
        authority_scope: AuthorityScope::Implementation,
        default_budget: Budget {
            time_limit_seconds: 5,
            token_limit: 1000,
            max_retries: 1,
        },
        escalation_paths: std::collections::HashMap::new(),
        input_contract: EventType::TaskAssigned,
        output_contract: vec![
            EventType::TaskStarted,
            EventType::TaskCompleted,
            EventType::TaskFailed,
            EventType::ToolExecuted,
            EventType::EscalationRequested,
        ],
    }
}
