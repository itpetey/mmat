//! Organisation runtime managing role execution, event processing, and shutdown.

use std::{path::PathBuf, sync::Arc, time::Duration};

use mmat_event_stream::{
    event::{EventType, RoleId, SemanticEvent},
    event_bus::{EventBus, RecvError},
    event_store::EventStore,
};
use mmat_memory::{artefact_store::ArtefactStore, store::MemoryStore};
use tokio::{
    signal,
    sync::{broadcast, mpsc},
    time::{interval, timeout},
};
use tracing::{info, warn};

use crate::{
    error::{Error, Result},
    registry::RoleRegistry,
    retrieval::RetrievalPlanner,
    role::{CoordinatorHandle, CoordinatorMessage, RoleContext, RoleLifecycleState, SpawnableRole},
    scheduler::Scheduler,
};

/// Configuration for the organisation runtime.
#[derive(Clone, Debug)]
pub struct OrganisationConfig {
    /// Capacity of the internal event bus channel.
    pub event_bus_capacity: usize,
    /// Interval at which heartbeat events are published.
    pub heartbeat_interval: Duration,
    /// Grace period for role shutdown before forced abort.
    pub shutdown_grace_period: Duration,
    /// Postgres connection string for all durable runtime state.
    pub database_url: String,
    /// Host working directory where all project directories reside.
    pub host_work_dir: Option<PathBuf>,
}

/// Runtime that owns and orchestrates the entire organisation: roles, event bus,
/// memory store, scheduler, and shutdown coordination.
pub struct OrganisationRuntime {
    config: OrganisationConfig,
    bus: EventBus,
    event_store: Arc<EventStore>,
    memory_store: Arc<MemoryStore>,
    artefact_store: Arc<ArtefactStore>,
    registry: Arc<RoleRegistry>,
    scheduler: Arc<tokio::sync::Mutex<Scheduler>>,
    #[allow(dead_code)]
    retrieval_planner: RetrievalPlanner,
    roles: Vec<Arc<dyn SpawnableRole>>,
    coordinator_tx: mpsc::Sender<CoordinatorMessage>,
    shutdown_tx: broadcast::Sender<()>,
}

impl OrganisationConfig {
    /// Creates an organisation configuration using Postgres for durable state.
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            event_bus_capacity: 1024,
            heartbeat_interval: Duration::from_secs(30),
            shutdown_grace_period: Duration::from_secs(10),
            database_url: database_url.into(),
            host_work_dir: None,
        }
    }
}

impl OrganisationRuntime {
    /// Creates a new organisation runtime from the given configuration and registry.
    ///
    /// Opens the event store and memory store, validates the registry is non-empty,
    /// and initialises the scheduler.
    pub fn new(config: OrganisationConfig, registry: RoleRegistry) -> Result<Self> {
        if config.database_url.trim().is_empty() {
            return Err(Error::Runtime(
                "database_url is required for organisation runtime durability".into(),
            ));
        }

        let event_store = Arc::new(EventStore::empty());
        let bus = EventBus::new(config.event_bus_capacity).with_store(Arc::clone(&event_store));
        let memory_store = Arc::new(
            MemoryStore::new(&config.database_url)
                .map_err(|e| Error::Runtime(format!("failed to connect to Postgres: {e}")))?,
        );

        let artefact_store = Arc::new(
            ArtefactStore::new_postgres(&config.database_url)
                .map_err(|e| Error::Runtime(format!("failed to create artefact store: {e}")))?,
        );

        // Validate registry
        if registry.all_roles().is_empty() {
            return Err(Error::Runtime("role registry is empty".into()));
        }

        let (coordinator_tx, coordinator_rx) = mpsc::channel(128);
        let scheduler = Arc::new(tokio::sync::Mutex::new(Scheduler::new(
            bus.clone(),
            Arc::new(registry.clone()),
            coordinator_rx,
        )));

        let (shutdown_tx, _shutdown_rx) = broadcast::channel(1);

        Ok(Self {
            config,
            bus,
            event_store,
            memory_store,
            artefact_store,
            registry: Arc::new(registry),
            scheduler,
            retrieval_planner: RetrievalPlanner::new(),
            roles: Vec::new(),
            coordinator_tx,
            shutdown_tx,
        })
    }

    /// Adds a role to the runtime. The role will be spawned when [`run`](Self::run) is called.
    pub fn add_role<R: crate::role::Role>(&mut self, role: R) {
        self.roles
            .push(Arc::new(crate::role::RoleHandle::new(role)));
    }

    /// Returns a reference to the event bus.
    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    /// Returns a reference to the event store.
    pub fn event_store(&self) -> &Arc<EventStore> {
        &self.event_store
    }

    /// Returns a reference to the artefact store.
    pub fn artefact_store(&self) -> &Arc<ArtefactStore> {
        &self.artefact_store
    }

    /// Returns a reference to the memory store.
    pub fn memory_store(&self) -> &Arc<MemoryStore> {
        &self.memory_store
    }

    /// Returns a reference to the role registry.
    pub fn registry(&self) -> &Arc<RoleRegistry> {
        &self.registry
    }

    /// Returns a reference to the scheduler.
    pub fn scheduler(&self) -> &Arc<tokio::sync::Mutex<Scheduler>> {
        &self.scheduler
    }

    /// Request graceful shutdown of the runtime.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Returns a handle that can trigger graceful shutdown from another task.
    pub fn shutdown_handle(&self) -> broadcast::Sender<()> {
        self.shutdown_tx.clone()
    }

    /// Runs the organisation runtime.
    ///
    /// Replays stored events, publishes an organisation-started event, spawns all roles,
    /// and runs background tasks for the scheduler, coordinator message processing,
    /// budget monitoring, and heartbeats. Blocks until a shutdown signal is received
    /// (Ctrl+C or explicit [`shutdown`](Self::shutdown) call), then gracefully stops all roles.
    pub async fn run(mut self) -> Result<()> {
        self.hydrate_event_store_from_db().await?;

        // Startup replay
        self.replay_events().await?;

        let persistence_handle = self.spawn_event_persistence_task();

        // Publish organisation started
        self.bus
            .publish(SemanticEvent::new_organisation_started(RoleId::new(
                "coordinator",
            )))
            .map_err(|e| Error::Runtime(format!("failed to publish OrganisationStarted: {e}")))?;

        // Spawn all roles
        let mut handles = Vec::new();
        for role in &self.roles {
            let role_id = role.id();
            let subscriptions = role.subscriptions().to_vec();
            let receiver = self.bus.subscribe(&subscriptions);
            let ctx = RoleContext {
                bus: self.bus.clone(),
                receiver,
                memory_store: Arc::clone(&self.memory_store),
                artefact_store: Some(Arc::clone(&self.artefact_store)),
                coordinator: CoordinatorHandle::new(self.coordinator_tx.clone()),
                tools: Box::new(()),
                host_work_dir: self.config.host_work_dir.clone(),
            };
            let role_clone = Arc::clone(role);
            let role_id_for_task = role_id.clone();
            let handle = tokio::spawn(async move {
                info!("Role {} started", role_id_for_task);
                let result = role_clone.run(ctx).await;
                match &result {
                    Ok(_) => info!("Role {} completed", role_id_for_task),
                    Err(e) => warn!("Role {} failed: {}", role_id_for_task, e),
                }
                result
            });
            handles.push((role_id, handle));
        }

        // Spawn scheduler event loop
        let bus_clone = self.bus.clone();
        let scheduler_clone = Arc::clone(&self.scheduler);
        let scheduler_handle = tokio::spawn(async move {
            let mut rx = bus_clone.subscribe(&[
                EventType::TaskAssigned,
                EventType::TaskStarted,
                EventType::TaskCompleted,
                EventType::TaskFailed,
                EventType::EscalationRequested,
                EventType::ToolExecuted,
            ]);
            while let Ok(event) = rx.recv().await {
                let mut scheduler = scheduler_clone.lock().await;
                scheduler.handle_event(&event);
            }
        });

        // Spawn coordinator message processor
        let scheduler_clone2 = Arc::clone(&self.scheduler);
        let coordinator_handle = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(100));
            loop {
                ticker.tick().await;
                let mut scheduler = scheduler_clone2.lock().await;
                scheduler.process_coordinator_messages();
            }
        });

        // Spawn budget monitor
        let scheduler_clone4 = Arc::clone(&self.scheduler);
        let budget_handle = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(1));
            loop {
                ticker.tick().await;
                let mut scheduler = scheduler_clone4.lock().await;
                scheduler.check_budgets();
            }
        });

        // Heartbeat publisher
        let heartbeat_interval = self.config.heartbeat_interval;
        let bus_clone2 = self.bus.clone();
        let scheduler_clone3 = Arc::clone(&self.scheduler);
        let heartbeat_handle = tokio::spawn(async move {
            let mut ticker = interval(heartbeat_interval);
            loop {
                ticker.tick().await;
                let scheduler = scheduler_clone3.lock().await;
                let states = scheduler.role_states();
                let active = states
                    .values()
                    .filter(|s| matches!(s, RoleLifecycleState::Running))
                    .count() as u32;
                let completed = states
                    .values()
                    .filter(|s| matches!(s, RoleLifecycleState::Completed))
                    .count() as u32;
                let failed = states
                    .values()
                    .filter(|s| matches!(s, RoleLifecycleState::Failed))
                    .count() as u32;
                let _ = bus_clone2.publish(SemanticEvent::new_heartbeat(
                    RoleId::new("coordinator"),
                    active,
                    completed,
                    failed,
                ));
            }
        });

        // Main event loop: wait for shutdown signal
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        #[cfg(not(target_arch = "wasm32"))]
        {
            tokio::select! {
                _ = shutdown_rx.recv() => {},
                _ = signal::ctrl_c() => {},
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let _ = shutdown_rx.recv().await;
        }
        info!("Shutdown signal received");

        // Graceful shutdown
        self.bus
            .publish(SemanticEvent::new_organisation_stopped(
                RoleId::new("coordinator"),
                "shutdown signal",
            ))
            .map_err(|e| Error::Runtime(format!("failed to publish OrganisationStopped: {e}")))?;

        // Wait for running tasks up to grace period, then abort any that didn't finish
        let grace = self.config.shutdown_grace_period;
        for (role_id, handle) in &mut handles {
            match timeout(grace, async { (&mut *handle).await }).await {
                Ok(Ok(_)) => info!("Role {} shut down cleanly", role_id),
                Ok(Err(e)) => warn!("Role {} panicked: {}", role_id, e),
                Err(_) => {
                    warn!(
                        "Role {} did not shut down within grace period, aborting",
                        role_id
                    );
                }
            }
        }
        for (_, handle) in handles {
            handle.abort();
        }

        // Flush event store (EventStore syncs on each insert, so nothing extra needed)
        // Abort remaining background tasks
        scheduler_handle.abort();
        coordinator_handle.abort();
        budget_handle.abort();
        heartbeat_handle.abort();
        persistence_handle.abort();

        info!("Organisation runtime stopped");
        Ok(())
    }

    async fn hydrate_event_store_from_db(&self) -> Result<()> {
        let mut connection = mmat_db::connect(&self.config.database_url)
            .await
            .map_err(|e| Error::Runtime(format!("failed to connect to event database: {e}")))?;
        mmat_db::ensure_schema(&mut connection)
            .await
            .map_err(|e| Error::Runtime(format!("failed to initialise event schema: {e}")))?;
        let events = mmat_db::replay_events(&mut connection, 0, None)
            .await
            .map_err(|e| Error::Runtime(format!("failed to replay persisted events: {e}")))?;

        for event in events {
            self.event_store
                .insert(&event)
                .map_err(|e| Error::Runtime(format!("failed to hydrate event store: {e}")))?;
        }

        Ok(())
    }

    fn spawn_event_persistence_task(&self) -> tokio::task::JoinHandle<()> {
        let database_url = self.config.database_url.clone();
        let shutdown_tx = self.shutdown_tx.clone();
        let mut receiver = self.bus.subscribe(&[]);

        tokio::spawn(async move {
            let mut connection = match mmat_db::connect(&database_url).await {
                Ok(connection) => connection,
                Err(error) => {
                    warn!("failed to connect to event database for persistence: {error}");
                    let _ = shutdown_tx.send(());
                    return;
                }
            };

            if let Err(error) = mmat_db::ensure_schema(&mut connection).await {
                warn!("failed to initialise event database schema: {error}");
                let _ = shutdown_tx.send(());
                return;
            }

            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        if let Err(error) =
                            persist_semantic_event(&mut connection, event.as_ref()).await
                        {
                            warn!("failed to persist semantic event: {error}");
                            let _ = shutdown_tx.send(());
                            break;
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        warn!(
                            "event persistence subscriber lagged by {skipped} events; shutting down"
                        );
                        let _ = shutdown_tx.send(());
                        break;
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        })
    }

    async fn replay_events(&mut self) -> Result<()> {
        let events = self
            .event_store
            .replay(0, None)
            .map_err(|e| Error::Runtime(format!("failed to replay events: {e}")))?;

        let mut scheduler = self.scheduler.lock().await;
        for event in &events {
            if let SemanticEvent::RoleStateChanged {
                role_id, new_state, ..
            } = event
            {
                let state = match new_state.as_str() {
                    "Idle" => RoleLifecycleState::Idle,
                    "Running" => RoleLifecycleState::Running,
                    "Completed" => RoleLifecycleState::Completed,
                    "Failed" => RoleLifecycleState::Failed,
                    "Escalated" => RoleLifecycleState::Escalated,
                    _ => RoleLifecycleState::Idle,
                };
                scheduler.set_role_state_silent(role_id.clone(), state);
            }
            // Replay task events to rebuild budget/task state
            scheduler.replay_task_event(event);
        }
        Ok(())
    }
}

async fn persist_semantic_event(
    connection: &mut mmat_db::AsyncPgConnection,
    event: &SemanticEvent,
) -> std::result::Result<(), mmat_db::DbError> {
    if let SemanticEvent::LaneCreated {
        lane_id,
        name,
        purpose,
        parent_lane_id,
        source_event_id,
        source_message_id,
        source_agent,
        ..
    } = event
        && !event.context().project_id.is_empty()
    {
        let now = mmat_db::now_timestamp_string();
        let lane = mmat_db::models::NewLane {
            id: lane_id.clone(),
            project_id: event.context().project_id.clone(),
            title: name.clone(),
            summary: purpose.clone(),
            status: "active".to_string(),
            creator: source_agent.to_string(),
            parent_lane_id: parent_lane_id.clone(),
            origin_event_id: (*source_event_id).map(|event_id| event_id.0),
            origin_message_id: source_message_id.clone(),
            created_at: now.clone(),
            updated_at: now,
            archived_at: None,
        };
        mmat_db::create_lane_with_event(connection, lane, event.clone()).await?;
    } else {
        mmat_db::append_event(connection, event).await?;
    }

    Ok(())
}
