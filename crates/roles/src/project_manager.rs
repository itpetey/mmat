//! The ProjectManager role decomposes architectural decisions into task cards, manages a delivery graph,
//! assigns tasks to workers, and tracks progress through to completion.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use mmat_coordinator::{
    AuthorityScope, Budget, CapabilityStatus, Role, RoleContext, RoleError, RoleLifecycleState,
    RoleReadiness, RoleSpec, RoleType,
};
use mmat_event_stream::event::{EventType, RoleId as EventRoleId, SemanticEvent, TaskContract};
use mmat_llm::{
    client::LlmClient,
    executor::{Executor, ExecutorConfig},
    message::{CompletionRequest, Message},
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    artefacts::{Adr, TaskCard as ArtefactTaskCard, ValidationPolicy},
    tooling::{RoleToolRegistry, RoleToolRuntime},
};

/// The lifecycle status of a task within the delivery graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// The task is waiting for dependencies to be satisfied.
    Pending,
    /// The task has been assigned to a worker.
    Assigned,
    /// The task is currently being executed.
    Running,
    /// The task has been completed successfully.
    Completed,
    /// The task has failed.
    Failed,
}

/// A node in the delivery graph representing a task and its dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryGraphNode {
    /// The task card describing the work.
    pub task_card: ArtefactTaskCard,
    /// Current status of the task.
    pub status: TaskStatus,
    /// IDs of tasks this task depends on.
    pub dependencies: Vec<String>,
    /// The role assigned to execute this task, if any.
    pub assignee: Option<String>,
}

/// A directed acyclic graph representing the delivery plan for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryGraph {
    /// Unique identifier for this delivery graph.
    pub id: String,
    /// Nodes keyed by task ID.
    pub nodes: HashMap<String, DeliveryGraphNode>,
    /// Directed edges from dependency to dependent task.
    pub edges: Vec<(String, String)>,
}

/// The ProjectManager role decomposes ADRs into tasks, manages the delivery graph, and assigns work to workers.
pub struct ProjectManager {
    id: EventRoleId,
    llm_client: Option<Arc<dyn LlmClient>>,
    #[allow(dead_code)]
    executor: Executor,
    tool_registry: RoleToolRegistry,
    tool_runtime: RoleToolRuntime,
    delivery_graph: Arc<RwLock<DeliveryGraph>>,
    task_status: Arc<RwLock<HashMap<String, TaskStatus>>>,
    pending_adrs: Arc<RwLock<Vec<Adr>>>,
    processed_decisions: Arc<RwLock<std::collections::HashSet<String>>>,
}

impl DeliveryGraph {
    /// Creates a new empty delivery graph.
    pub fn new() -> Self {
        Self {
            id: format!("dg-{}", Uuid::new_v4()),
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    /// Adds a task card as a node in the delivery graph with the given dependencies.
    pub fn add_node(&mut self, task_card: ArtefactTaskCard, dependencies: Vec<String>) {
        let id = task_card.id.clone();
        self.nodes.insert(
            id.clone(),
            DeliveryGraphNode {
                task_card,
                status: TaskStatus::Pending,
                dependencies: dependencies.clone(),
                assignee: None,
            },
        );
        for dep in &dependencies {
            self.edges.push((dep.clone(), id.clone()));
        }
    }

    /// Performs a topological sort of the delivery graph. Returns an error if a cycle is detected.
    pub fn topological_sort(&self) -> Result<Vec<String>, String> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();

        for (node_id, node) in &self.nodes {
            in_degree.entry(node_id.clone()).or_insert(0);
            for dep in &node.dependencies {
                adj.entry(dep.clone()).or_default().push(node_id.clone());
                *in_degree.entry(node_id.clone()).or_insert(0) += 1;
            }
        }

        let mut queue: Vec<String> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(id, _)| id.clone())
            .collect();

        let mut result = Vec::new();

        while let Some(node) = queue.pop() {
            result.push(node.clone());
            if let Some(neighbors) = adj.get(&node) {
                for neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(neighbor.clone());
                        }
                    }
                }
            }
        }

        if result.len() == self.nodes.len() {
            Ok(result)
        } else {
            Err("Cycle detected in delivery graph".to_string())
        }
    }

    /// Returns the IDs of tasks whose dependencies are all completed and are pending execution.
    pub fn ready_tasks(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|(_, node)| {
                node.status == TaskStatus::Pending
                    && node.dependencies.iter().all(|dep| {
                        self.nodes
                            .get(dep)
                            .map(|n| n.status == TaskStatus::Completed)
                            .unwrap_or(true)
                    })
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Updates the status of a task in the delivery graph.
    pub fn update_status(&mut self, task_id: &str, status: TaskStatus) {
        if let Some(node) = self.nodes.get_mut(task_id) {
            node.status = status;
        }
    }
}

/// Creates an empty delivery graph with a new unique identifier.
impl Default for DeliveryGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectManager {
    /// Creates a new ProjectManager with default settings and no LLM client.
    pub fn new() -> Self {
        Self {
            id: EventRoleId("pm-001".to_string()),
            llm_client: None,
            executor: Executor,
            tool_registry: RoleToolRegistry::new(),
            tool_runtime: RoleToolRuntime,
            delivery_graph: Arc::new(RwLock::new(DeliveryGraph::new())),
            task_status: Arc::new(RwLock::new(HashMap::new())),
            pending_adrs: Arc::new(RwLock::new(Vec::new())),
            processed_decisions: Arc::new(RwLock::new(std::collections::HashSet::new())),
        }
    }

    /// Configures the ProjectManager with an LLM client for task decomposition.
    pub fn with_llm_client(mut self, llm_client: Arc<dyn LlmClient>) -> Self {
        self.llm_client = Some(llm_client);
        self
    }

    /// Configures the ProjectManager with a custom tool registry.
    pub fn with_tool_registry(mut self, tool_registry: RoleToolRegistry) -> Self {
        self.tool_registry = tool_registry;
        self
    }

    /// Returns whether an LLM client has been configured.
    pub fn has_llm_client(&self) -> bool {
        self.llm_client.is_some()
    }

    /// Returns a reference to the shared delivery graph for external inspection.
    pub fn delivery_graph(&self) -> Arc<RwLock<DeliveryGraph>> {
        Arc::clone(&self.delivery_graph)
    }

    fn create_task_card(
        &self,
        description: &str,
        contract: &str,
        dependencies: Vec<String>,
        adr_refs: Vec<String>,
        validation_policy: Option<ValidationPolicy>,
    ) -> ArtefactTaskCard {
        ArtefactTaskCard {
            id: format!("task-{}", Uuid::new_v4()),
            description: description.to_string(),
            contract: contract.to_string(),
            dependencies,
            adr_references: adr_refs,
            validation_policy,
            acceptance_criteria: vec!["Meets contract specification".to_string()],
        }
    }

    async fn decompose_work(&self, _ctx: &RoleContext) -> Result<Vec<ArtefactTaskCard>, RoleError> {
        let adrs = {
            let mut pending = self.pending_adrs.write();
            std::mem::take(&mut *pending)
        };

        if adrs.is_empty() {
            info!("No ADRs received yet, cannot decompose work");
            return Ok(vec![]);
        }

        let mut cards = Vec::new();

        for adr in &adrs {
            if let Some(client) = &self.llm_client {
                let prompt = format!(
                    "Given this Architecture Decision Record:\n{}\n\n\
Generate a task card with: description, contract specification, \
dependencies (if any), acceptance criteria, and validation policy.",
                    serde_json::to_string(adr).unwrap_or_default()
                );

                let request = CompletionRequest::new(
                    "pm-decompose",
                    vec![
                        Message::system(
                            "You are a project manager decomposing architectural decisions into implementation tasks.",
                        ),
                        Message::user(&prompt),
                    ],
                );

                let response = Executor::run(
                    client.as_ref(),
                    &self.tool_registry,
                    &ExecutorConfig {
                        max_turns: 3,
                        max_tokens: None,
                    },
                    &self.tool_runtime,
                    request,
                )
                .await;

                let description = match response {
                    Ok(Message::Assistant { content, .. }) => {
                        content.unwrap_or_else(|| format!("Implement: {}", adr.title))
                    }
                    _ => format!("Implement: {}", adr.title),
                };

                let card = self.create_task_card(
                    &description,
                    &adr.decision,
                    vec![],
                    vec![adr.id.clone()],
                    None,
                );
                cards.push(card);
            } else {
                let card = self.create_task_card(
                    &format!("Implement: {}", adr.title),
                    &adr.decision,
                    vec![],
                    vec![adr.id.clone()],
                    None,
                );
                cards.push(card);
            }
        }

        info!(
            "Decomposed {} ADRs into {} task cards",
            adrs.len(),
            cards.len()
        );
        Ok(cards)
    }

    async fn decompose_and_assign(&self, ctx: &RoleContext) -> Result<(), RoleError> {
        let cards = self.decompose_work(ctx).await?;

        if cards.is_empty() {
            return Ok(());
        }

        let sorted_order = {
            let mut graph = self.delivery_graph.write();
            for card in &cards {
                graph.add_node(card.clone(), card.dependencies.clone());
            }

            match graph.topological_sort() {
                Ok(order) => order,
                Err(e) => {
                    warn!("Delivery graph has cycles, using insertion order: {}", e);
                    cards.iter().map(|c| c.id.clone()).collect()
                }
            }
        };

        self.publish_delivery_graph(ctx).await?;

        for task_id in &sorted_order {
            let node = {
                let graph = self.delivery_graph.read();
                graph.nodes.get(task_id).cloned()
            };
            if let Some(node) = node
                && node.status == TaskStatus::Pending
                && node.dependencies.iter().all(|dep| {
                    self.delivery_graph
                        .read()
                        .nodes
                        .get(dep)
                        .map(|n| n.status == TaskStatus::Completed)
                        .unwrap_or(true)
                })
            {
                self.assign_task(ctx, &node.task_card, "worker-001").await?;
            }
        }

        Ok(())
    }

    async fn assign_task(
        &self,
        ctx: &RoleContext,
        task_card: &ArtefactTaskCard,
        worker_id: &str,
    ) -> Result<(), RoleError> {
        let contract = TaskContract {
            contract_id: task_card.id.clone(),
            description: task_card.contract.clone(),
        };

        let event = SemanticEvent::new_task_assigned(
            EventRoleId(self.id.0.clone()),
            &task_card.id,
            EventRoleId(worker_id.to_string()),
            contract,
            task_card.dependencies.clone(),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish task assigned event: {e:?}"))
        })?;

        self.task_status
            .write()
            .insert(task_card.id.clone(), TaskStatus::Assigned);
        self.delivery_graph
            .write()
            .update_status(&task_card.id, TaskStatus::Assigned);

        info!("Assigned task {} to worker {}", task_card.id, worker_id);
        Ok(())
    }

    #[allow(dead_code)]
    async fn publish_milestone(
        &self,
        ctx: &RoleContext,
        milestone_name: &str,
        completed_tasks: &[String],
    ) -> Result<(), RoleError> {
        let serialised = serde_json::to_string(&completed_tasks)
            .map_err(|e| RoleError::Internal(format!("Failed to serialise milestone: {e}")))?;

        let payload = format!("{milestone_name}|{serialised}");
        let stored = ctx.store_artefact("milestone", &payload).await?;
        let event = SemanticEvent::new_artefact_produced_ref(
            EventRoleId(self.id.0.clone()),
            stored.artefact_id,
            "milestone",
            stored.content_hash,
            stored.storage_uri,
            EventRoleId(self.id.0.clone()),
            Vec::new(),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish milestone event: {e:?}"))
        })?;

        info!("Published milestone: {}", milestone_name);
        Ok(())
    }

    async fn publish_delivery_graph(&self, ctx: &RoleContext) -> Result<(), RoleError> {
        let serialised = {
            let graph = self.delivery_graph.read();
            serde_json::to_string(&*graph).map_err(|e| {
                RoleError::Internal(format!("Failed to serialise delivery graph: {e}"))
            })?
        };

        let stored = ctx.store_artefact("delivery_graph", &serialised).await?;
        let event = SemanticEvent::new_artefact_produced_ref(
            EventRoleId(self.id.0.clone()),
            stored.artefact_id.clone(),
            "delivery_graph",
            stored.content_hash,
            stored.storage_uri,
            EventRoleId(self.id.0.clone()),
            Vec::new(),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish delivery graph event: {e:?}"))
        })?;

        info!("Published delivery graph: {}", stored.artefact_id);
        Ok(())
    }

    async fn detect_blockers(&self, _ctx: &RoleContext) -> Result<(), RoleError> {
        let status = self.task_status.read();
        for (task_id, task_status) in status.iter() {
            if *task_status == TaskStatus::Assigned {
                warn!(
                    "Potential blocker detected: task {} still assigned",
                    task_id
                );
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Role for ProjectManager {
    fn id(&self) -> EventRoleId {
        EventRoleId(self.id.0.clone())
    }

    fn spec(&self) -> RoleSpec {
        RoleSpec {
            id: EventRoleId(self.id.0.clone()),
            role_type: RoleType::ProjectManager,
            authority_scope: AuthorityScope::Planning,
            default_budget: Budget {
                time_limit_seconds: 600,
                token_limit: 200_000,
                max_retries: 3,
            },
            escalation_paths: std::collections::HashMap::new(),
            input_contract: EventType::TaskAssigned,
            output_contract: vec![
                EventType::TaskAssigned,
                EventType::ArtefactProduced,
                EventType::MemoryProposed,
            ],
        }
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[
            EventType::TaskAssigned,
            EventType::TaskCompleted,
            EventType::TaskFailed,
            EventType::DecisionRecorded,
            EventType::ArtefactProduced,
        ]
    }

    fn role_readiness(&self) -> RoleReadiness {
        let has_llm = self.has_llm_client();
        let capability = if has_llm {
            CapabilityStatus::Configured
        } else {
            CapabilityStatus::Fallback
        };
        RoleReadiness {
            capability,
            has_llm_client: has_llm,
            has_tools: false,
            tool_count: 0,
            fallback_worktree: false,
            requires_llm: true,
            has_artefact_store: false,
            summary: format!(
                "LLM: {} — {}",
                if has_llm { "configured" } else { "missing" },
                capability,
            ),
        }
    }

    async fn run(self: Arc<Self>, ctx: RoleContext) -> Result<(), RoleError> {
        info!("ProjectManager starting");

        ctx.coordinator
            .report_status(EventRoleId(self.id.0.clone()), RoleLifecycleState::Running)
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        let mut receiver = ctx.bus.subscribe(self.subscriptions());

        loop {
            let event = receiver
                .recv()
                .await
                .map_err(|e| RoleError::Internal(format!("Failed to receive event: {e:?}")))?;

            match event.as_ref() {
                SemanticEvent::DecisionRecorded {
                    decision_text,
                    rationale_refs,
                    source_agent,
                    ..
                } => {
                    info!("PM received ADR from {}", source_agent.0);
                    if source_agent.0 == "architect-001" {
                        info!("PM waiting for Architect ADR artefact before decomposition");
                        continue;
                    }
                    if self.processed_decisions.read().contains(decision_text) {
                        info!("PM already processed this decision, skipping");
                        continue;
                    }
                    self.processed_decisions
                        .write()
                        .insert(decision_text.clone());
                    let adr = Adr {
                        id: format!("adr-{}", Uuid::new_v4()),
                        title: decision_text
                            .lines()
                            .next()
                            .unwrap_or("Architecture Decision")
                            .to_string(),
                        status: "received".to_string(),
                        context: format!("From: {}", source_agent.0),
                        decision: decision_text.clone(),
                        alternatives: vec![],
                        tradeoffs: "See decision text".to_string(),
                        consequences: "See decision text".to_string(),
                        references: rationale_refs
                            .iter()
                            .map(|r| r.description.clone())
                            .collect(),
                    };
                    {
                        let mut pending = self.pending_adrs.write();
                        if !pending.iter().any(|a| a.id == adr.id) {
                            pending.push(adr);
                        }
                    }
                    info!(
                        "PM stored pending ADR, total: {}",
                        self.pending_adrs.read().len()
                    );
                    self.decompose_and_assign(&ctx).await?;
                }
                SemanticEvent::ArtefactProduced {
                    artefact_type,
                    storage_uri,
                    source_agent,
                    ..
                } if artefact_type == "adr" => {
                    info!("PM received ADR artefact from {}", source_agent.0);
                    if let Ok(Some(serialised)) = ctx.get_artefact_payload(storage_uri).await
                        && let Ok(adr) = serde_json::from_str::<Adr>(&serialised)
                    {
                        if self.processed_decisions.read().contains(&adr.decision) {
                            info!("PM already processed this decision artefact, skipping");
                            continue;
                        }
                        self.processed_decisions
                            .write()
                            .insert(adr.decision.clone());
                        {
                            let mut pending = self.pending_adrs.write();
                            if !pending.iter().any(|a| a.id == adr.id) {
                                pending.push(adr);
                            }
                        }
                        info!(
                            "PM parsed ADR artefact, total: {}",
                            self.pending_adrs.read().len()
                        );
                        self.decompose_and_assign(&ctx).await?;
                    }
                }
                SemanticEvent::TaskCompleted {
                    task_id,
                    contract_id: _,
                    ..
                } => {
                    let ready_tasks = {
                        self.task_status
                            .write()
                            .insert(task_id.clone(), TaskStatus::Completed);

                        let mut graph = self.delivery_graph.write();
                        graph.update_status(task_id, TaskStatus::Completed);
                        graph.ready_tasks()
                    };

                    for ready_task_id in ready_tasks {
                        let node = {
                            let graph = self.delivery_graph.read();
                            graph.nodes.get(&ready_task_id).cloned()
                        };
                        if let Some(node) = node {
                            self.assign_task(&ctx, &node.task_card, "worker-001")
                                .await?;
                        }
                    }
                }
                SemanticEvent::TaskFailed {
                    task_id,
                    error_description,
                    ..
                } => {
                    self.task_status
                        .write()
                        .insert(task_id.clone(), TaskStatus::Failed);
                    self.delivery_graph
                        .write()
                        .update_status(task_id, TaskStatus::Failed);
                    warn!("Task {} failed: {}", task_id, error_description);
                }
                SemanticEvent::TaskAssigned { worker_id, .. } if worker_id.0 == self.id.0 => {
                    self.decompose_and_assign(&ctx).await?;
                }
                _ => {}
            }

            self.detect_blockers(&ctx).await?;
        }
    }
}

impl Default for ProjectManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use mmat_coordinator::{AuthorityScope, Role, RoleType};
    use mmat_event_stream::event::EventType;

    use super::*;

    fn task_card(id: &str, dependencies: Vec<String>) -> ArtefactTaskCard {
        ArtefactTaskCard {
            id: id.to_string(),
            description: format!("Task {id}"),
            contract: format!("contract-{id}"),
            dependencies,
            adr_references: vec![],
            validation_policy: None,
            acceptance_criteria: vec![],
        }
    }

    #[test]
    fn creates_with_default_id() {
        let project_manager = ProjectManager::new();
        assert_eq!(project_manager.id().0, "pm-001");
    }

    #[test]
    fn spec_matches_planning_authority_and_contracts() {
        let project_manager = ProjectManager::new();
        let spec = project_manager.spec();
        assert_eq!(spec.role_type, RoleType::ProjectManager);
        assert!(matches!(spec.authority_scope, AuthorityScope::Planning));
        assert!(spec.output_contract.contains(&EventType::TaskAssigned));
        assert!(spec.output_contract.contains(&EventType::ArtefactProduced));
    }

    #[test]
    fn subscribes_to_delivery_events() {
        let project_manager = ProjectManager::new();
        let subscriptions = project_manager.subscriptions();
        assert!(subscriptions.contains(&EventType::TaskAssigned));
        assert!(subscriptions.contains(&EventType::TaskCompleted));
        assert!(subscriptions.contains(&EventType::TaskFailed));
    }

    #[test]
    fn delivery_graph_topological_sort_orders_dependencies_first() {
        let mut graph = DeliveryGraph::new();

        graph.add_node(task_card("task-a", vec![]), vec![]);
        graph.add_node(
            task_card("task-b", vec!["task-a".to_string()]),
            vec!["task-a".to_string()],
        );
        graph.add_node(
            task_card("task-c", vec!["task-a".to_string(), "task-b".to_string()]),
            vec!["task-a".to_string(), "task-b".to_string()],
        );

        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0], "task-a");
        let task_b_position = sorted.iter().position(|id| id == "task-b").unwrap();
        let task_c_position = sorted.iter().position(|id| id == "task-c").unwrap();
        assert!(task_b_position < task_c_position);
    }

    #[test]
    fn delivery_graph_ready_tasks_excludes_incomplete_dependencies() {
        let mut graph = DeliveryGraph::new();

        graph.add_node(task_card("task-a", vec![]), vec![]);
        graph.add_node(
            task_card("task-b", vec!["task-a".to_string()]),
            vec!["task-a".to_string()],
        );

        let ready = graph.ready_tasks();
        assert_eq!(ready, vec!["task-a"]);

        graph.update_status("task-a", TaskStatus::Completed);
        let ready = graph.ready_tasks();
        assert_eq!(ready, vec!["task-b"]);
    }

    #[test]
    fn delivery_graph_detects_cycles() {
        let mut graph = DeliveryGraph::new();

        graph.add_node(
            task_card("task-a", vec!["task-b".to_string()]),
            vec!["task-b".to_string()],
        );
        graph.add_node(
            task_card("task-b", vec!["task-a".to_string()]),
            vec!["task-a".to_string()],
        );

        let result = graph.topological_sort();
        assert!(result.is_err());
    }

    #[test]
    fn delivery_graph_status_updates_affect_readiness() {
        let mut graph = DeliveryGraph::new();
        graph.add_node(task_card("task-status-001", vec![]), vec![]);
        assert_eq!(graph.nodes["task-status-001"].status, TaskStatus::Pending);

        graph.update_status("task-status-001", TaskStatus::Assigned);
        assert_eq!(graph.nodes["task-status-001"].status, TaskStatus::Assigned);

        let ready = graph.ready_tasks();
        assert!(ready.is_empty(), "Assigned task should not be ready");
    }
}
