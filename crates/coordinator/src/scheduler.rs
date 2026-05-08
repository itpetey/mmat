//! Task scheduling, budget tracking, and role lifecycle management.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mmat_event_stream::event::{
    EscalationSeverity, EventId, EventType, RoleId, SemanticEvent, TaskContract,
};
use mmat_event_stream::event_bus::EventBus;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::contract::{ContractId, TaskContext};
use crate::registry::RoleRegistry;
use crate::role::{Budget, CoordinatorMessage, RoleLifecycleState, Severity};

/// Tracks the resource budget usage for a single contract.
#[derive(Clone, Debug)]
pub struct BudgetState {
    /// When the budget tracking started.
    pub started: Instant,
    /// Maximum wall-clock time allowed.
    pub time_limit: Duration,
    /// Number of tokens consumed so far.
    pub tokens_used: u64,
    /// Maximum number of tokens allowed.
    pub token_limit: u64,
    /// Current retry attempt count.
    pub retry_current: u32,
    /// Maximum permitted retry attempts.
    pub retry_max: u32,
}

/// Core scheduler that manages role lifecycle, task dispatch, budget enforcement,
/// escalation routing, and heartbeat monitoring.
#[allow(dead_code)]
pub struct Scheduler {
    role_states: HashMap<RoleId, RoleLifecycleState>,
    task_tracker: HashMap<ContractId, TaskContext>,
    budget_tracker: HashMap<ContractId, BudgetState>,
    role_contracts: HashMap<RoleId, Vec<ContractId>>,
    contract_roles: HashMap<ContractId, RoleId>,
    timeout_flagged: HashSet<ContractId>,
    bus: EventBus,
    registry: Arc<RoleRegistry>,
    coordinator_rx: mpsc::Receiver<CoordinatorMessage>,
    heartbeat_timeout: Duration,
    last_event_time: HashMap<RoleId, Instant>,
}

impl BudgetState {
    /// Creates a new budget state initialised from a [`Budget`] specification.
    pub fn new(budget: &Budget) -> Self {
        Self {
            started: Instant::now(),
            time_limit: Duration::from_secs(budget.time_limit_seconds),
            tokens_used: 0,
            token_limit: budget.token_limit,
            retry_current: 0,
            retry_max: budget.max_retries,
        }
    }

    /// Returns `true` if the time limit has been exceeded.
    pub fn is_timeout(&self) -> bool {
        self.started.elapsed() > self.time_limit
    }

    /// Returns `true` if the token limit has been exceeded.
    pub fn is_token_exceeded(&self) -> bool {
        self.tokens_used > self.token_limit
    }

    /// Returns token usage as a percentage (0–100).
    pub fn token_usage_percent(&self) -> u8 {
        if self.token_limit == 0 {
            return 0;
        }
        ((self.tokens_used as f64 / self.token_limit as f64) * 100.0).min(100.0) as u8
    }

    /// Returns `true` if the contract has remaining retry attempts.
    pub fn can_retry(&self) -> bool {
        self.retry_current < self.retry_max
    }
}

impl Scheduler {
    /// Creates a new scheduler with the given event bus, registry, and coordinator receiver.
    pub fn new(
        bus: EventBus,
        registry: Arc<RoleRegistry>,
        coordinator_rx: mpsc::Receiver<CoordinatorMessage>,
    ) -> Self {
        Self {
            role_states: HashMap::new(),
            task_tracker: HashMap::new(),
            budget_tracker: HashMap::new(),
            role_contracts: HashMap::new(),
            contract_roles: HashMap::new(),
            timeout_flagged: HashSet::new(),
            bus,
            registry,
            coordinator_rx,
            heartbeat_timeout: Duration::from_secs(60),
            last_event_time: HashMap::new(),
        }
    }

    /// Sets the heartbeat timeout duration for dead-role detection.
    pub fn with_heartbeat_timeout(mut self, timeout: Duration) -> Self {
        self.heartbeat_timeout = timeout;
        self
    }

    /// Transitions a role to a new lifecycle state, publishing a state change event.
    pub fn set_role_state(&mut self, role_id: RoleId, state: RoleLifecycleState) {
        let old_state = self
            .role_states
            .get(&role_id)
            .cloned()
            .unwrap_or(RoleLifecycleState::Idle);

        if old_state.can_transition_to(&state) {
            let _ = self.bus.publish(SemanticEvent::new_role_state_changed(
                RoleId::new("coordinator"),
                role_id.clone(),
                old_state.to_string(),
                state.to_string(),
            ));
            self.role_states.insert(role_id.clone(), state.clone());
            self.last_event_time.insert(role_id, Instant::now());
        }
    }

    /// Set role state without publishing an event. Used during startup replay.
    pub fn set_role_state_silent(&mut self, role_id: RoleId, state: RoleLifecycleState) {
        let old_state = self
            .role_states
            .get(&role_id)
            .cloned()
            .unwrap_or(RoleLifecycleState::Idle);

        if old_state.can_transition_to(&state) {
            self.role_states.insert(role_id.clone(), state.clone());
            self.last_event_time.insert(role_id, Instant::now());
        }
    }

    /// Returns the current lifecycle state of a role.
    pub fn get_role_state(&self, role_id: &RoleId) -> RoleLifecycleState {
        self.role_states
            .get(role_id)
            .cloned()
            .unwrap_or(RoleLifecycleState::Idle)
    }

    /// Returns a reference to all role lifecycle states.
    pub fn role_states(&self) -> &HashMap<RoleId, RoleLifecycleState> {
        &self.role_states
    }

    /// Returns a reference to the budget tracker.
    pub fn budget_tracker(&self) -> &HashMap<ContractId, BudgetState> {
        &self.budget_tracker
    }

    /// Checks all active budgets for time or token overruns.
    ///
    /// Expired contracts are failed, and the affected roles transitioned to `Failed`.
    pub fn check_budgets(&mut self) {
        let expired: Vec<(RoleId, ContractId)> = self
            .budget_tracker
            .iter()
            .filter(|(id, state)| state.is_timeout() && !self.timeout_flagged.contains(id))
            .filter_map(|(id, _)| self.contract_roles.get(id).map(|r| (r.clone(), *id)))
            .collect();

        for (role_id, contract_id) in expired {
            warn!("Contract {} exceeded time budget", contract_id);
            self.timeout_flagged.insert(contract_id);
            // Publish TaskFailed with the actual role_id so the handler can find the budget
            let _ = self.bus.publish(SemanticEvent::new_task_failed(
                role_id.clone(),
                contract_id.to_string(),
                "budget exceeded: timeout",
            ));
            self.set_role_state(role_id, RoleLifecycleState::Failed);
        }

        let token_exceeded: Vec<(RoleId, ContractId)> = self
            .budget_tracker
            .iter()
            .filter(|(id, state)| state.is_token_exceeded() && !self.timeout_flagged.contains(id))
            .filter_map(|(id, _)| self.contract_roles.get(id).map(|r| (r.clone(), *id)))
            .collect();

        for (role_id, contract_id) in token_exceeded {
            warn!("Contract {} exceeded token budget", contract_id);
            self.timeout_flagged.insert(contract_id);
            let _ = self.bus.publish(SemanticEvent::new_task_failed(
                role_id.clone(),
                contract_id.to_string(),
                "budget exceeded: tokens",
            ));
            self.set_role_state(role_id, RoleLifecycleState::Failed);
        }
    }

    /// Processes a semantic event, updating role state, budgets, and escalations.
    pub fn handle_event(&mut self, event: &SemanticEvent) {
        debug!("Scheduler handling event: {:?}", event.variant_name());

        if !self.validate_role_output(event) {
            return;
        }

        match event {
            SemanticEvent::TaskAssigned {
                worker_id,
                contract_ref,
                ..
            } => {
                self.set_role_state(worker_id.clone(), RoleLifecycleState::Running);
                let Ok(contract_uuid) = uuid::Uuid::parse_str(&contract_ref.contract_id) else {
                    let _ = self.bus.publish(SemanticEvent::new_task_failed(
                        RoleId::new("coordinator"),
                        contract_ref.contract_id.clone(),
                        "invalid contract id",
                    ));
                    self.set_role_state(worker_id.clone(), RoleLifecycleState::Failed);
                    return;
                };
                let contract_id = ContractId(contract_uuid);
                if let Some(spec) = self.registry.get(worker_id.clone()) {
                    self.budget_tracker
                        .entry(contract_id)
                        .and_modify(|state| {
                            state.started = Instant::now();
                            state.tokens_used = 0;
                        })
                        .or_insert_with(|| BudgetState::new(&spec.default_budget));
                    self.role_contracts
                        .entry(worker_id.clone())
                        .or_default()
                        .push(contract_id);
                    self.contract_roles.insert(contract_id, worker_id.clone());
                    self.timeout_flagged.remove(&contract_id);
                }
            }
            SemanticEvent::TaskStarted { worker_id, .. } => {
                self.last_event_time
                    .insert(worker_id.clone(), Instant::now());
            }
            SemanticEvent::TaskCompleted {
                source_agent,
                contract_id,
                ..
            } => {
                self.set_role_state(source_agent.clone(), RoleLifecycleState::Completed);
                self.last_event_time
                    .insert(source_agent.clone(), Instant::now());
                let cid = uuid::Uuid::parse_str(contract_id).map(ContractId).ok();
                if let Some(contract_id) = cid {
                    self.remove_contract(contract_id);
                } else if let Some(contracts) = self.role_contracts.get(source_agent)
                    && let Some(contract_id) = contracts.first().copied()
                {
                    self.remove_contract(contract_id);
                }
            }
            SemanticEvent::TaskFailed { source_agent, .. } => {
                self.last_event_time
                    .insert(source_agent.clone(), Instant::now());
                let budget_opt = self
                    .find_budget_for_role(source_agent)
                    .map(|(id, b)| (id, b.clone()));
                if let Some((contract_id, budget)) = budget_opt {
                    if budget.can_retry() {
                        let mut new_budget = budget.clone();
                        new_budget.retry_current += 1;
                        new_budget.started = Instant::now();
                        new_budget.tokens_used = 0;
                        self.budget_tracker.insert(contract_id, new_budget);
                        self.timeout_flagged.remove(&contract_id);
                        let _ = self.bus.publish(SemanticEvent::new_task_assigned(
                            RoleId::new("coordinator"),
                            contract_id.to_string(),
                            source_agent.clone(),
                            TaskContract {
                                contract_id: contract_id.to_string(),
                                description: format!("retry {}", budget.retry_current + 1),
                            },
                            vec![],
                        ));
                        self.set_role_state(source_agent.clone(), RoleLifecycleState::Running);
                    } else {
                        self.remove_contract(contract_id);
                        self.set_role_state(source_agent.clone(), RoleLifecycleState::Failed);
                        if let Some(target_id) = self
                            .registry
                            .escalation_target(source_agent, &Severity::High)
                        {
                            let _ = self.bus.publish(SemanticEvent::new_escalation_requested(
                                RoleId::new("coordinator"),
                                source_agent.clone(),
                                target_id,
                                "retry limit exhausted",
                                EscalationSeverity::High,
                            ));
                        }
                    }
                } else {
                    self.set_role_state(source_agent.clone(), RoleLifecycleState::Failed);
                }
            }
            SemanticEvent::EscalationRequested {
                from_role,
                to_role,
                severity,
                reason,
                ..
            } => {
                self.last_event_time
                    .insert(from_role.clone(), Instant::now());
                let severity = Severity::from(severity.clone());
                let target_id = if to_role != from_role {
                    Some(to_role.clone())
                } else {
                    self.registry.escalation_target(from_role, &severity)
                };
                if let Some(target_id) = target_id {
                    let _ = self.bus.publish(SemanticEvent::new_task_assigned(
                        RoleId::new("coordinator"),
                        format!("escalation-{}", uuid::Uuid::new_v4()),
                        target_id.clone(),
                        TaskContract {
                            contract_id: uuid::Uuid::new_v4().to_string(),
                            description: reason.clone(),
                        },
                        vec![],
                    ));
                    let _ = self.bus.publish(SemanticEvent::new_escalation_accepted(
                        RoleId::new("coordinator"),
                        EventId::new(),
                        target_id,
                        1,
                    ));
                    self.set_role_state(from_role.clone(), RoleLifecycleState::Escalated);
                }
            }
            SemanticEvent::ToolExecuted {
                source_agent,
                token_usage,
                ..
            } => {
                self.last_event_time
                    .insert(source_agent.clone(), Instant::now());
                let contract_id = self.find_budget_for_role(source_agent).map(|(id, _)| id);
                if let Some(contract_id) = contract_id
                    && let Some(b) = self.budget_tracker.get_mut(&contract_id)
                {
                    b.tokens_used += token_usage;
                    let percent = b.token_usage_percent();
                    if (80..100).contains(&percent) {
                        let _ = self.bus.publish(SemanticEvent::new_budget_warning(
                            RoleId::new("coordinator"),
                            contract_id.to_string(),
                            format!("token usage at {}%", percent),
                            percent,
                        ));
                    }
                }
            }
            _ => {}
        }

        // Check for dead roles (heartbeat monitoring)
        self.check_dead_roles();
    }

    /// Drain any pending coordinator messages and handle them.
    pub fn process_coordinator_messages(&mut self) {
        while let Ok(msg) = self.coordinator_rx.try_recv() {
            self.handle_coordinator_message(msg);
        }
    }

    /// Handles a single coordinator message (status report or escalation request).
    pub fn handle_coordinator_message(&mut self, msg: CoordinatorMessage) {
        match msg {
            CoordinatorMessage::ReportStatus { role_id, state } => {
                self.set_role_state(role_id, state);
            }
            CoordinatorMessage::RequestEscalation {
                from,
                severity,
                reason,
            } => {
                let _ = self.bus.publish(SemanticEvent::new_escalation_requested(
                    RoleId::new("coordinator"),
                    from.clone(),
                    from.clone(),
                    reason,
                    EscalationSeverity::from(severity),
                ));
            }
        }
    }

    /// Detects running roles that have not sent a heartbeat within the timeout
    /// and marks them as `Failed`.
    pub fn check_dead_roles(&mut self) {
        let now = Instant::now();
        let dead_roles: Vec<RoleId> = self
            .last_event_time
            .iter()
            .filter(|(_, last_time)| now.duration_since(**last_time) > self.heartbeat_timeout)
            .filter(|(role_id, _)| {
                matches!(
                    self.role_states.get(role_id),
                    Some(RoleLifecycleState::Running)
                )
            })
            .map(|(role_id, _)| role_id.clone())
            .collect();

        for role_id in dead_roles {
            warn!("Role {} marked as failed due to heartbeat timeout", role_id);
            self.set_role_state(role_id, RoleLifecycleState::Failed);
        }
    }

    fn find_budget_for_role(&self, role_id: &RoleId) -> Option<(ContractId, &BudgetState)> {
        let contracts = self.role_contracts.get(role_id)?;
        let contract_id = contracts.first()?;
        self.budget_tracker
            .get(contract_id)
            .map(|state| (*contract_id, state))
    }

    fn remove_contract(&mut self, contract_id: ContractId) {
        if let Some(role_id) = self.contract_roles.remove(&contract_id)
            && let Some(contracts) = self.role_contracts.get_mut(&role_id)
        {
            contracts.retain(|cid| *cid != contract_id);
            if contracts.is_empty() {
                self.role_contracts.remove(&role_id);
            }
        }
        self.budget_tracker.remove(&contract_id);
        self.timeout_flagged.remove(&contract_id);
    }

    fn validate_role_output(&mut self, event: &SemanticEvent) -> bool {
        let Some((source_agent, event_type)) = role_output_event(event) else {
            return true;
        };

        if source_agent == RoleId::new("coordinator") {
            return true;
        }

        let Some(spec) = self.registry.get(source_agent.clone()) else {
            return true;
        };

        let output_allowed = spec.output_contract.contains(&event_type);
        let authority_allowed = spec.authority_scope.can_publish(&event_type);
        if output_allowed && authority_allowed {
            return true;
        }

        let reason = if !output_allowed {
            format!("role {source_agent} output contract does not allow {event_type:?}")
        } else {
            format!("role {source_agent} authority scope does not allow {event_type:?}")
        };

        let _ = self
            .bus
            .publish(SemanticEvent::new_policy_violation_detected(
                RoleId::new("coordinator"),
                "contract violation",
                reason,
                Some(event.event_id()),
            ));
        if let Some(contracts) = self.role_contracts.get(&source_agent)
            && let Some(contract_id) = contracts.first().copied()
        {
            self.remove_contract(contract_id);
        }
        self.set_role_state(source_agent, RoleLifecycleState::Failed);
        false
    }

    /// Replay a task-related event during startup to rebuild budget/task state silently.
    pub fn replay_task_event(&mut self, event: &SemanticEvent) {
        match event {
            SemanticEvent::TaskAssigned {
                worker_id,
                contract_ref,
                ..
            } => {
                if let Ok(cid) = uuid::Uuid::parse_str(&contract_ref.contract_id) {
                    let contract_id = ContractId(cid);
                    if let Some(spec) = self.registry.get(worker_id.clone()) {
                        self.budget_tracker
                            .insert(contract_id, BudgetState::new(&spec.default_budget));
                        self.role_contracts
                            .entry(worker_id.clone())
                            .or_default()
                            .push(contract_id);
                        self.contract_roles.insert(contract_id, worker_id.clone());
                        self.timeout_flagged.remove(&contract_id);
                    }
                }
            }
            SemanticEvent::TaskCompleted {
                source_agent,
                contract_id,
                ..
            } => {
                if let Ok(cid) = uuid::Uuid::parse_str(contract_id) {
                    self.remove_contract(ContractId(cid));
                } else if let Some(contracts) = self.role_contracts.get(source_agent)
                    && let Some(cid) = contracts.first().copied()
                {
                    self.remove_contract(cid);
                }
            }
            SemanticEvent::TaskFailed {
                source_agent,
                task_id,
                ..
            } => {
                if let Ok(cid) = uuid::Uuid::parse_str(task_id) {
                    self.remove_contract(ContractId(cid));
                } else if let Some(contracts) = self.role_contracts.get(source_agent)
                    && let Some(cid) = contracts.first().copied()
                {
                    self.remove_contract(cid);
                }
            }
            _ => {}
        }
    }
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler")
            .field("role_states", &self.role_states)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for crate::role::RoleType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

fn role_output_event(event: &SemanticEvent) -> Option<(RoleId, EventType)> {
    match event {
        SemanticEvent::TaskStarted { source_agent, .. } => {
            Some((source_agent.clone(), EventType::TaskStarted))
        }
        SemanticEvent::TaskCompleted { source_agent, .. } => {
            Some((source_agent.clone(), EventType::TaskCompleted))
        }
        SemanticEvent::TaskFailed { source_agent, .. } => {
            Some((source_agent.clone(), EventType::TaskFailed))
        }
        SemanticEvent::EscalationRequested { source_agent, .. } => {
            Some((source_agent.clone(), EventType::EscalationRequested))
        }
        SemanticEvent::ToolExecuted { source_agent, .. } => {
            Some((source_agent.clone(), EventType::ToolExecuted))
        }
        SemanticEvent::ClaimMade { source_agent, .. } => {
            Some((source_agent.clone(), EventType::ClaimMade))
        }
        SemanticEvent::DecisionRecorded { source_agent, .. } => {
            Some((source_agent.clone(), EventType::DecisionRecorded))
        }
        SemanticEvent::ReviewCompleted { source_agent, .. } => {
            Some((source_agent.clone(), EventType::ReviewCompleted))
        }
        SemanticEvent::ArtefactProduced { source_agent, .. } => {
            Some((source_agent.clone(), EventType::ArtefactProduced))
        }
        _ => None,
    }
}
