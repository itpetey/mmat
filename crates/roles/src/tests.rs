#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use mmat_coordinator::{
        AuthorityScope, CoordinatorHandle, Role, RoleContext, RoleRegistry, RoleType,
    };
    use mmat_event_stream::event::{
        ArtefactRef, EventType, ReviewFinding, RoleId as EventRoleId, SemanticEvent, TaskContract,
    };
    use mmat_event_stream::event_bus::EventBus;
    use mmat_memory::error::Result as MemoryResult;
    use mmat_memory::librarian::Librarian;
    use mmat_memory::qdrant::VectorMemoryBackend;
    use mmat_memory::store::MemoryStore;
    use mmat_memory::types::{MemoryId, MemoryType};
    use parking_lot::Mutex;
    use qdrant_client::qdrant::Value;
    use tempfile::tempdir;

    use crate::artefacts::{Adr, FailureClass, TaskCard};
    use crate::project_manager::{DeliveryGraph, TaskStatus};
    use crate::{Architect, ProjectManager, Reviewer, Worker};
    use crate::{IntentLead, OpsManager, Scholar};

    #[derive(Default)]
    struct FakeVectorBackend {
        upserts: Mutex<Vec<MemoryId>>,
    }

    #[async_trait::async_trait]
    impl VectorMemoryBackend for FakeVectorBackend {
        async fn upsert(
            &self,
            id: MemoryId,
            _embedding: Vec<f32>,
            _payload: HashMap<String, Value>,
        ) -> MemoryResult<()> {
            self.upserts.lock().push(id);
            Ok(())
        }

        async fn search(
            &self,
            _query_embedding: Vec<f32>,
            _limit: u64,
        ) -> MemoryResult<Vec<(MemoryId, f32)>> {
            Ok(self.upserts.lock().iter().map(|id| (*id, 1.0)).collect())
        }

        async fn delete(&self, id: MemoryId) -> MemoryResult<()> {
            self.upserts.lock().retain(|existing| *existing != id);
            Ok(())
        }
    }

    fn setup_test_env() -> (EventBus, Arc<MemoryStore>) {
        let bus = EventBus::new(100);
        let dir = tempdir().unwrap();
        let memory_store = Arc::new(MemoryStore::open(dir.path().join("test.db")).unwrap());
        (bus, memory_store)
    }

    #[test]
    fn test_intent_lead_creation() {
        let intent_lead = IntentLead::new();
        assert_eq!(intent_lead.id().0, "intent-lead-001");
    }

    #[test]
    fn test_intent_lead_subscriptions() {
        let intent_lead = IntentLead::new();
        let subs = intent_lead.subscriptions();
        assert!(subs.contains(&EventType::HumanFeedbackReceived));
        assert!(subs.contains(&EventType::TaskCompleted));
    }

    #[test]
    fn test_intent_lead_spec() {
        let intent_lead = IntentLead::new();
        let spec = intent_lead.spec();
        assert_eq!(spec.role_type, RoleType::IntentLead);
        assert!(matches!(spec.authority_scope, AuthorityScope::IntentOnly));
        assert_eq!(spec.input_contract, EventType::HumanFeedbackReceived);
        assert!(spec.output_contract.contains(&EventType::ArtefactProduced));
        assert!(spec.output_contract.contains(&EventType::TaskAssigned));
        assert!(
            spec.output_contract
                .contains(&EventType::HumanFeedbackRequested)
        );
        assert!(spec.output_contract.contains(&EventType::MemoryProposed));

        assert!(
            spec.authority_scope
                .can_publish(&EventType::HumanFeedbackRequested)
        );
        assert!(spec.authority_scope.can_publish(&EventType::TaskAssigned));
        assert!(
            spec.authority_scope
                .can_publish(&EventType::ArtefactProduced)
        );
        assert!(spec.authority_scope.can_publish(&EventType::MemoryProposed));

        let mut registry = RoleRegistry::new();
        registry.register(spec).unwrap();
    }

    #[test]
    fn test_scholar_creation() {
        let scholar = Scholar::new();
        assert_eq!(scholar.id().0, "scholar-001");
    }

    #[test]
    fn test_scholar_subscriptions() {
        let scholar = Scholar::new();
        let subs = scholar.subscriptions();
        assert!(subs.contains(&EventType::TaskAssigned));
        assert!(subs.contains(&EventType::HumanFeedbackReceived));
    }

    #[test]
    fn test_scholar_spec() {
        let scholar = Scholar::new();
        let spec = scholar.spec();
        assert_eq!(spec.role_type, RoleType::Scholar);
        assert!(matches!(spec.authority_scope, AuthorityScope::Architecture));
        assert!(spec.output_contract.contains(&EventType::ArtefactProduced));
        assert!(spec.output_contract.contains(&EventType::ClaimMade));
        assert!(spec.output_contract.contains(&EventType::MemoryProposed));
        assert!(!spec.output_contract.contains(&EventType::DecisionRecorded));
    }

    #[test]
    fn test_scholar_budget_tracking() {
        let scholar = Scholar::new().with_budget(5, 3, 10);
        let spec = scholar.spec();
        assert_eq!(spec.default_budget.max_retries, 2);
        assert!(spec.default_budget.time_limit_seconds > 0);
    }

    #[test]
    fn test_ops_manager_creation() {
        let ops_manager = OpsManager::new();
        assert_eq!(ops_manager.id().0, "ops-manager-001");
    }

    #[test]
    fn test_ops_manager_subscriptions() {
        let ops_manager = OpsManager::new();
        let subs = ops_manager.subscriptions();
        assert!(subs.contains(&EventType::TaskAssigned));
        assert!(subs.contains(&EventType::ReviewCompleted));
    }

    #[test]
    fn test_ops_manager_spec() {
        let ops_manager = OpsManager::new();
        let spec = ops_manager.spec();
        assert_eq!(spec.role_type, RoleType::OpsManager);
        assert!(matches!(spec.authority_scope, AuthorityScope::Architecture));
        assert!(spec.output_contract.contains(&EventType::DecisionRecorded));
        assert!(spec.output_contract.contains(&EventType::MemoryProposed));
        assert!(spec.output_contract.contains(&EventType::ArtefactProduced));
    }

    #[tokio::test]
    async fn test_intent_lead_initial_prompt_to_brief_and_dispatch() {
        let (bus, memory_store) = setup_test_env();
        let intent_lead = Arc::new(IntentLead::new());
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);
        let receiver = bus.subscribe(&[EventType::HumanFeedbackReceived]);
        let mut output_rx = bus.subscribe(&[
            EventType::HumanFeedbackRequested,
            EventType::ArtefactProduced,
            EventType::TaskAssigned,
            EventType::MemoryProposed,
        ]);

        let ctx = RoleContext {
            bus: bus.clone(),
            receiver,
            memory_store,
            coordinator,
            tools: Box::new(()),
        };

        let run_handle = tokio::spawn(intent_lead.run(ctx));
        bus.publish(SemanticEvent::new_human_feedback_received(
            EventRoleId("user".to_string()),
            "Build a data pipeline for analytics",
        ))
        .unwrap();

        let answers = [
            "Success means reliable daily reports",
            "Do not build a dashboard",
            "Must run within the existing Rust workspace",
            "Daily report generated before 09:00",
            "Prefer simplicity over throughput",
            "Prefer explicit errors",
        ];
        let mut answer_index = 0;
        let mut questions = 0;
        let mut produced_intent_brief = false;
        let mut dispatched_scholar = false;
        let mut dispatched_ops = false;
        let mut proposed_preference = false;

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            let event =
                tokio::time::timeout(tokio::time::Duration::from_millis(250), output_rx.recv())
                    .await;

            let Ok(Ok(event)) = event else {
                continue;
            };

            match event.as_ref() {
                SemanticEvent::HumanFeedbackRequested { .. } => {
                    questions += 1;
                    let answer = answers.get(answer_index).unwrap_or(&"No more details");
                    answer_index += 1;
                    bus.publish(SemanticEvent::new_human_feedback_received(
                        EventRoleId("user".to_string()),
                        *answer,
                    ))
                    .unwrap();
                }
                SemanticEvent::ArtefactProduced { artefact_type, .. } => {
                    if artefact_type == "intent_brief" {
                        produced_intent_brief = true;
                    }
                }
                SemanticEvent::TaskAssigned { worker_id, .. } => {
                    dispatched_scholar |= worker_id.0 == "scholar-001";
                    dispatched_ops |= worker_id.0 == "ops-manager-001";
                }
                SemanticEvent::MemoryProposed {
                    memory_type,
                    content,
                    ..
                } => {
                    proposed_preference |=
                        memory_type == "Preference" && content.contains("simplicity");
                }
                _ => {}
            }

            if questions > 0
                && produced_intent_brief
                && dispatched_scholar
                && dispatched_ops
                && proposed_preference
            {
                break;
            }
        }

        let result = tokio::time::timeout(tokio::time::Duration::from_secs(1), run_handle)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok());
        assert!(questions > 0);
        assert!(produced_intent_brief);
        assert!(dispatched_scholar);
        assert!(dispatched_ops);
        assert!(proposed_preference);
    }

    #[tokio::test]
    async fn test_scholar_receives_task_and_completes() {
        let (bus, _memory_store) = setup_test_env();

        let scholar = Arc::new(Scholar::new());

        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);

        let receiver = bus.subscribe(&[EventType::TaskAssigned]);

        let bus_clone = bus.clone();
        let handle = tokio::spawn(async move {
            let mut received_artefacts = Vec::new();

            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let contract = TaskContract {
                contract_id: "test-contract".to_string(),
                description: "Research the codebase architecture".to_string(),
            };
            let event = SemanticEvent::new_task_assigned(
                EventRoleId("intent-lead-001".to_string()),
                "test-task",
                EventRoleId("scholar-001".to_string()),
                contract,
                vec![],
            );
            bus_clone.publish(event).unwrap();

            let mut sub = bus_clone.subscribe(&[EventType::ArtefactProduced]);
            let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
            while tokio::time::Instant::now() < deadline {
                tokio::select! {
                    result = sub.recv() => {
                        if let Ok(evt) = result {
                            if let SemanticEvent::ArtefactProduced { artefact_type, .. } = evt.as_ref() {
                                received_artefacts.push(artefact_type.clone());
                                if received_artefacts.len() >= 3 {
                                    break;
                                }
                            }
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
                }
            }

            received_artefacts
        });

        let ctx = RoleContext {
            bus: bus.clone(),
            receiver,
            memory_store: Arc::new(
                MemoryStore::open(tempdir().unwrap().path().join("test3.db")).unwrap(),
            ),
            coordinator,
            tools: Box::new(()),
        };

        let result =
            tokio::time::timeout(tokio::time::Duration::from_secs(10), scholar.run(ctx)).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());

        let artefacts = handle.await.unwrap();
        assert!(artefacts.contains(&"research_brief".to_string()));
        assert!(artefacts.contains(&"evidence_pack".to_string()));
        assert!(artefacts.contains(&"open_questions".to_string()));
    }

    #[tokio::test]
    async fn test_scholar_exceeds_budget_escapes_budget_extension_via_task_assigned() {
        let (bus, _memory_store) = setup_test_env();

        let scholar = Arc::new(Scholar::new().with_budget(1, 0, 1));

        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);

        let receiver = bus.subscribe(&[EventType::TaskAssigned]);

        let bus_clone = bus.clone();
        let handle = tokio::spawn(async move {
            let mut received_escalation = false;

            let mut sub = bus_clone.subscribe(&[EventType::EscalationRequested]);

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            let contract = TaskContract {
                contract_id: "test-contract".to_string(),
                description: "Research with limited budget".to_string(),
            };
            let event = SemanticEvent::new_task_assigned(
                EventRoleId("intent-lead-001".to_string()),
                "test-task",
                EventRoleId("scholar-001".to_string()),
                contract,
                vec![],
            );
            bus_clone.publish(event).unwrap();

            if let Ok(event) = sub.recv().await {
                if let SemanticEvent::EscalationRequested { reason, .. } = event.as_ref() {
                    if reason.contains("budget") {
                        received_escalation = true;
                    }
                }
            }

            received_escalation
        });

        let ctx = RoleContext {
            bus: bus.clone(),
            receiver,
            memory_store: Arc::new(
                MemoryStore::open(tempdir().unwrap().path().join("test4.db")).unwrap(),
            ),
            coordinator,
            tools: Box::new(()),
        };

        let _ = tokio::time::timeout(tokio::time::Duration::from_secs(10), scholar.run(ctx)).await;

        let received_escalation = handle.await.unwrap();
        assert!(
            received_escalation,
            "Scholar should escalate on budget exhaustion"
        );
    }

    #[tokio::test]
    async fn test_ops_manager_creates_sop_on_task() {
        let (bus, _memory_store) = setup_test_env();

        let ops_manager = Arc::new(OpsManager::new());

        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);

        let receiver = bus.subscribe(&[EventType::TaskAssigned]);

        let bus_for_spawn = bus.clone();
        let handle = tokio::spawn(async move {
            let mut received_memory_proposed = false;

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            let contract = TaskContract {
                contract_id: "test-contract".to_string(),
                description: "Define database migration SOP".to_string(),
            };
            let event = SemanticEvent::new_task_assigned(
                EventRoleId("intent-lead-001".to_string()),
                "test-task",
                EventRoleId("ops-manager-001".to_string()),
                contract,
                vec![],
            );
            bus_for_spawn.publish(event).unwrap();

            let mut sub = bus_for_spawn.subscribe(&[EventType::MemoryProposed]);
            for _ in 0..30 {
                if let Ok(evt) = sub.recv().await {
                    if let SemanticEvent::MemoryProposed { memory_type, .. } = evt.as_ref() {
                        if memory_type == "SOP" {
                            received_memory_proposed = true;
                            break;
                        }
                    }
                }
            }

            received_memory_proposed
        });

        let ctx = RoleContext {
            bus: bus.clone(),
            receiver,
            memory_store: Arc::new(
                MemoryStore::open(tempdir().unwrap().path().join("test5.db")).unwrap(),
            ),
            coordinator,
            tools: Box::new(()),
        };

        let _ =
            tokio::time::timeout(tokio::time::Duration::from_secs(10), ops_manager.run(ctx)).await;

        let sop_published = handle.await.unwrap();
        assert!(
            sop_published,
            "SOP MemoryProposed events should have been published"
        );
    }

    #[tokio::test]
    async fn test_ops_manager_sop_memory_is_accepted_by_librarian() {
        let bus = Arc::new(EventBus::new(100));
        let dir = tempdir().unwrap();
        let memory_store = Arc::new(MemoryStore::open(dir.path().join("test.db")).unwrap());
        let qdrant = Arc::new(FakeVectorBackend::default());
        let librarian = Librarian::new(
            memory_store.clone(),
            qdrant,
            tokio::time::Duration::from_secs(3600),
        );
        let librarian_bus = bus.clone();
        let librarian_handle = tokio::spawn(async move { librarian.run(librarian_bus).await });
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let ops_manager = Arc::new(OpsManager::new());
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);
        let receiver = bus.subscribe(&[EventType::TaskAssigned]);

        let ctx = RoleContext {
            bus: (*bus).clone(),
            receiver,
            memory_store: memory_store.clone(),
            coordinator,
            tools: Box::new(()),
        };
        let role_handle = tokio::spawn(ops_manager.run(ctx));
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        bus.publish(SemanticEvent::new_task_assigned(
            EventRoleId("intent-lead-001".to_string()),
            "test-task",
            EventRoleId("ops-manager-001".to_string()),
            TaskContract {
                contract_id: "test-contract".to_string(),
                description: "Define database migration SOP".to_string(),
            },
            vec![],
        ))
        .unwrap();

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        let mut accepted = false;
        while tokio::time::Instant::now() < deadline {
            let sops = memory_store.query_by_type(MemoryType::SOP).unwrap();
            if !sops.is_empty() {
                accepted = true;
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        role_handle.abort();
        librarian_handle.abort();
        assert!(accepted, "Librarian should accept at least one SOP memory");
    }

    #[test]
    fn test_scholar_output_does_not_contain_architectural_decisions() {
        let input = "You should use microservices architecture for this system";
        let filtered = Scholar::filter_architectural_recommendendations(input);
        assert!(
            !filtered.contains("should use"),
            "Architectural recommendations should be filtered"
        );

        let safe_input = "The codebase uses a modular structure with clear boundaries";
        let filtered_safe = Scholar::filter_architectural_recommendendations(safe_input);
        assert_eq!(
            filtered_safe, safe_input,
            "Safe content should pass through unchanged"
        );
    }

    #[test]
    fn test_intent_lead_output_does_not_contain_implementation_suggestions() {
        let input = "Use React for the frontend and Node.js for the backend";
        let filtered = IntentLead::filter_implementation_suggestions(input);
        assert!(
            !filtered.contains("Use React"),
            "Implementation suggestions should be filtered"
        );

        let safe_input = "I want a fast and responsive user interface";
        let filtered_safe = IntentLead::filter_implementation_suggestions(safe_input);
        assert_eq!(
            filtered_safe, safe_input,
            "Safe content should pass through unchanged"
        );
    }

    #[test]
    fn test_artefact_types_serialise() {
        use crate::artefacts::{
            DeliveryStandards, EscalationRules, EvidencePack, IntentBrief, OpenQuestions,
            ReviewRubric, ValidationPolicy,
        };

        let brief = IntentBrief {
            goals: vec!["Goal 1".to_string()],
            non_goals: vec![],
            constraints: vec![],
            success_metrics: vec![],
            stakeholder_preferences: vec![],
            open_questions: vec![],
            confidence: 0.8,
        };
        let json = serde_json::to_string(&brief).unwrap();
        assert!(json.contains("Goal 1"));

        let pack = EvidencePack { findings: vec![] };
        let json = serde_json::to_string(&pack).unwrap();
        assert!(json.contains("findings"));

        let questions = OpenQuestions { questions: vec![] };
        let json = serde_json::to_string(&questions).unwrap();
        assert!(json.contains("questions"));

        let rubric = ReviewRubric { dimensions: vec![] };
        let json = serde_json::to_string(&rubric).unwrap();
        assert!(json.contains("dimensions"));

        let policy = ValidationPolicy {
            project_type: "cli".to_string(),
            steps: vec![],
        };
        let json = serde_json::to_string(&policy).unwrap();
        assert!(json.contains("cli"));

        let rules = EscalationRules { rules: vec![] };
        let json = serde_json::to_string(&rules).unwrap();
        assert!(json.contains("rules"));

        let standards = DeliveryStandards {
            branch_naming_convention: "feature/<desc>".to_string(),
            commit_message_format: "type: msg".to_string(),
            pr_size_limit: "400 lines".to_string(),
            review_requirements: vec![],
        };
        let json = serde_json::to_string(&standards).unwrap();
        assert!(json.contains("feature/<desc>"));
    }

    #[test]
    fn test_architect_creation() {
        let architect = Architect::new();
        assert_eq!(architect.id().0, "architect-001");
    }

    #[test]
    fn test_architect_spec() {
        let architect = Architect::new();
        let spec = architect.spec();
        assert_eq!(spec.role_type, RoleType::Architect);
        assert!(matches!(spec.authority_scope, AuthorityScope::Architecture));
        assert!(spec.output_contract.contains(&EventType::DecisionRecorded));
        assert!(spec.output_contract.contains(&EventType::ArtefactProduced));
    }

    #[test]
    fn test_architect_subscriptions() {
        let architect = Architect::new();
        let subs = architect.subscriptions();
        assert!(subs.contains(&EventType::TaskAssigned));
    }

    #[test]
    fn test_project_manager_creation() {
        let pm = ProjectManager::new();
        assert_eq!(pm.id().0, "pm-001");
    }

    #[test]
    fn test_project_manager_spec() {
        let pm = ProjectManager::new();
        let spec = pm.spec();
        assert_eq!(spec.role_type, RoleType::ProjectManager);
        assert!(matches!(spec.authority_scope, AuthorityScope::Planning));
        assert!(spec.output_contract.contains(&EventType::TaskAssigned));
        assert!(spec.output_contract.contains(&EventType::ArtefactProduced));
    }

    #[test]
    fn test_project_manager_subscriptions() {
        let pm = ProjectManager::new();
        let subs = pm.subscriptions();
        assert!(subs.contains(&EventType::TaskAssigned));
        assert!(subs.contains(&EventType::TaskCompleted));
        assert!(subs.contains(&EventType::TaskFailed));
    }

    #[test]
    fn test_worker_creation() {
        let worker = Worker::new();
        assert_eq!(worker.id().0, "worker-001");
    }

    #[test]
    fn test_worker_spec() {
        let worker = Worker::new();
        let spec = worker.spec();
        assert_eq!(spec.role_type, RoleType::Worker);
        assert!(matches!(
            spec.authority_scope,
            AuthorityScope::Implementation
        ));
        assert!(spec.output_contract.contains(&EventType::ToolExecuted));
        assert!(spec.output_contract.contains(&EventType::TaskCompleted));
    }

    #[test]
    fn test_worker_subscriptions() {
        let worker = Worker::new();
        let subs = worker.subscriptions();
        assert!(subs.contains(&EventType::TaskAssigned));
    }

    #[test]
    fn test_reviewer_creation() {
        let reviewer = Reviewer::new();
        assert_eq!(reviewer.id().0, "reviewer-001");
    }

    #[test]
    fn test_reviewer_spec() {
        let reviewer = Reviewer::new();
        let spec = reviewer.spec();
        assert_eq!(spec.role_type, RoleType::Reviewer);
        assert!(matches!(spec.authority_scope, AuthorityScope::Review));
        assert!(spec.output_contract.contains(&EventType::ReviewCompleted));
        assert!(
            spec.output_contract
                .contains(&EventType::EscalationRequested)
        );
    }

    #[test]
    fn test_reviewer_subscriptions() {
        let reviewer = Reviewer::new();
        let subs = reviewer.subscriptions();
        assert!(subs.contains(&EventType::ReviewRequested));
        assert!(subs.contains(&EventType::TaskCompleted));
    }

    #[test]
    fn test_delivery_graph_topological_sort() {
        let mut graph = DeliveryGraph::new();

        let task_a = TaskCard {
            id: "task-a".to_string(),
            description: "Task A".to_string(),
            contract: "contract-a".to_string(),
            dependencies: vec![],
            adr_references: vec![],
            validation_policy: None,
            acceptance_criteria: vec![],
        };
        let task_b = TaskCard {
            id: "task-b".to_string(),
            description: "Task B".to_string(),
            contract: "contract-b".to_string(),
            dependencies: vec!["task-a".to_string()],
            adr_references: vec![],
            validation_policy: None,
            acceptance_criteria: vec![],
        };
        let task_c = TaskCard {
            id: "task-c".to_string(),
            description: "Task C".to_string(),
            contract: "contract-c".to_string(),
            dependencies: vec!["task-a".to_string(), "task-b".to_string()],
            adr_references: vec![],
            validation_policy: None,
            acceptance_criteria: vec![],
        };

        graph.add_node(task_a, vec![]);
        graph.add_node(task_b, vec!["task-a".to_string()]);
        graph.add_node(task_c, vec!["task-a".to_string(), "task-b".to_string()]);

        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0], "task-a");
        let b_idx = sorted.iter().position(|s| s == "task-b").unwrap();
        let c_idx = sorted.iter().position(|s| s == "task-c").unwrap();
        assert!(b_idx < c_idx);
    }

    #[test]
    fn test_delivery_graph_ready_tasks() {
        let mut graph = DeliveryGraph::new();

        let task_a = TaskCard {
            id: "task-a".to_string(),
            description: "Task A".to_string(),
            contract: "contract-a".to_string(),
            dependencies: vec![],
            adr_references: vec![],
            validation_policy: None,
            acceptance_criteria: vec![],
        };
        let task_b = TaskCard {
            id: "task-b".to_string(),
            description: "Task B".to_string(),
            contract: "contract-b".to_string(),
            dependencies: vec!["task-a".to_string()],
            adr_references: vec![],
            validation_policy: None,
            acceptance_criteria: vec![],
        };

        graph.add_node(task_a, vec![]);
        graph.add_node(task_b, vec!["task-a".to_string()]);

        let ready = graph.ready_tasks();
        assert_eq!(ready, vec!["task-a"]);

        graph.update_status("task-a", TaskStatus::Completed);
        let ready = graph.ready_tasks();
        assert_eq!(ready, vec!["task-b"]);
    }

    #[test]
    fn test_delivery_graph_cycle_detection() {
        let mut graph = DeliveryGraph::new();

        let task_a = TaskCard {
            id: "task-a".to_string(),
            description: "Task A".to_string(),
            contract: "contract-a".to_string(),
            dependencies: vec!["task-b".to_string()],
            adr_references: vec![],
            validation_policy: None,
            acceptance_criteria: vec![],
        };
        let task_b = TaskCard {
            id: "task-b".to_string(),
            description: "Task B".to_string(),
            contract: "contract-b".to_string(),
            dependencies: vec!["task-a".to_string()],
            adr_references: vec![],
            validation_policy: None,
            acceptance_criteria: vec![],
        };

        graph.add_node(task_a, vec!["task-b".to_string()]);
        graph.add_node(task_b, vec!["task-a".to_string()]);

        let result = graph.topological_sort();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_architect_receives_task_and_produces_adr() {
        let (bus, _memory_store) = setup_test_env();

        let architect = Arc::new(Architect::new());

        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);

        let receiver = bus.subscribe(&[EventType::TaskAssigned]);

        let bus_clone = bus.clone();
        let handle = tokio::spawn(async move {
            let mut received_decision = false;
            let mut received_artefact = false;

            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let contract = TaskContract {
                contract_id: "test-contract".to_string(),
                description: "Design the data storage layer".to_string(),
            };
            let event = SemanticEvent::new_task_assigned(
                EventRoleId("pm-001".to_string()),
                "test-task",
                EventRoleId("architect-001".to_string()),
                contract,
                vec![],
            );
            bus_clone.publish(event).unwrap();

            let mut sub =
                bus_clone.subscribe(&[EventType::DecisionRecorded, EventType::ArtefactProduced]);
            let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
            while tokio::time::Instant::now() < deadline {
                tokio::select! {
                    result = sub.recv() => {
                        if let Ok(evt) = result {
                            match evt.as_ref() {
                                SemanticEvent::DecisionRecorded { .. } => {
                                    received_decision = true;
                                }
                                SemanticEvent::ArtefactProduced { artefact_type, .. } => {
                                    if artefact_type == "adr" || artefact_type == "interface_spec" || artefact_type == "dependency_rules" {
                                        received_artefact = true;
                                    }
                                }
                                _ => {}
                            }
                            if received_decision && received_artefact {
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
                }
            }

            (received_decision, received_artefact)
        });

        let ctx = RoleContext {
            bus: bus.clone(),
            receiver,
            memory_store: Arc::new(
                MemoryStore::open(tempdir().unwrap().path().join("test-arch.db")).unwrap(),
            ),
            coordinator,
            tools: Box::new(()),
        };

        let result =
            tokio::time::timeout(tokio::time::Duration::from_secs(10), architect.run(ctx)).await;
        assert!(result.is_ok());

        let (received_decision, received_artefact) = handle.await.unwrap();
        assert!(
            received_decision,
            "Architect should publish DecisionRecorded"
        );
        assert!(
            received_artefact,
            "Architect should publish ArtefactProduced"
        );
    }

    #[tokio::test]
    async fn test_worker_receives_task_and_completes() {
        let (bus, _memory_store) = setup_test_env();

        let worker = Arc::new(
            Worker::new()
                .with_validation_commands(vec![])
                .with_fallback_worktree(true),
        );

        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);

        let receiver = bus.subscribe(&[EventType::TaskAssigned]);

        let bus_clone = bus.clone();
        let handle = tokio::spawn(async move {
            let mut received_completed = false;
            let mut received_tool_executed = false;
            let mut received_claim = false;
            let mut received_artefact = false;

            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let contract = TaskContract {
                contract_id: "test-contract".to_string(),
                description: "Add error handling to module X".to_string(),
            };
            let event = SemanticEvent::new_task_assigned(
                EventRoleId("pm-001".to_string()),
                "test-task",
                EventRoleId("worker-001".to_string()),
                contract,
                vec![],
            );
            bus_clone.publish(event).unwrap();

            let mut sub = bus_clone.subscribe(&[
                EventType::TaskCompleted,
                EventType::ToolExecuted,
                EventType::ClaimMade,
                EventType::ArtefactProduced,
            ]);
            let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
            while tokio::time::Instant::now() < deadline {
                tokio::select! {
                    result = sub.recv() => {
                        if let Ok(evt) = result {
                            match evt.as_ref() {
                                SemanticEvent::TaskCompleted { .. } => {
                                    received_completed = true;
                                }
                                SemanticEvent::ToolExecuted { .. } => {
                                    received_tool_executed = true;
                                }
                                SemanticEvent::ClaimMade { .. } => {
                                    received_claim = true;
                                }
                                SemanticEvent::ArtefactProduced { artefact_type, .. } => {
                                    if artefact_type == "implementation_patch" {
                                        received_artefact = true;
                                    }
                                }
                                _ => {}
                            }
                            if received_completed && received_tool_executed && received_claim && received_artefact {
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
                }
            }

            (
                received_completed,
                received_tool_executed,
                received_claim,
                received_artefact,
            )
        });

        let ctx = RoleContext {
            bus: bus.clone(),
            receiver,
            memory_store: Arc::new(
                MemoryStore::open(tempdir().unwrap().path().join("test-worker.db")).unwrap(),
            ),
            coordinator,
            tools: Box::new(()),
        };

        let result =
            tokio::time::timeout(tokio::time::Duration::from_secs(10), worker.run(ctx)).await;
        assert!(result.is_ok());

        let (completed, tool_executed, claim, artefact) = handle.await.unwrap();
        assert!(completed, "Worker should publish TaskCompleted");
        assert!(tool_executed, "Worker should publish ToolExecuted");
        assert!(claim, "Worker should publish ClaimMade");
        assert!(
            artefact,
            "Worker should publish ArtefactProduced with patch"
        );
    }

    #[tokio::test]
    async fn test_worker_does_not_fallback_by_default() {
        let (bus, _memory_store) = setup_test_env();

        let worker = Arc::new(Worker::new().with_validation_commands(vec![]));

        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);

        let receiver = bus.subscribe(&[EventType::TaskAssigned]);

        let bus_clone = bus.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let contract = TaskContract {
                contract_id: "test-contract".to_string(),
                description: "Task with invalid branch name".to_string(),
            };
            let event = SemanticEvent::new_task_assigned(
                EventRoleId("pm-001".to_string()),
                "bad..branch",
                EventRoleId("worker-001".to_string()),
                contract,
                vec![],
            );
            bus_clone.publish(event).unwrap();
        });

        let ctx = RoleContext {
            bus: bus.clone(),
            receiver,
            memory_store: Arc::new(
                MemoryStore::open(tempdir().unwrap().path().join("test-worker-no-fallback.db"))
                    .unwrap(),
            ),
            coordinator,
            tools: Box::new(()),
        };

        let result =
            tokio::time::timeout(tokio::time::Duration::from_secs(10), worker.run(ctx)).await;
        assert!(
            matches!(result, Ok(Err(_))),
            "Worker should return the git worktree error when fallback is disabled: {:?}",
            result
        );
    }

    #[test]
    fn test_reviewer_failure_classification() {
        let reviewer = Reviewer::new();

        let defect = ReviewFinding {
            finding: "Missing error handling".to_string(),
            severity: "high".to_string(),
        };
        assert!(matches!(
            reviewer.classify_failure(&defect, &[]),
            FailureClass::ImplementationDefect
        ));

        let arch_conflict = ReviewFinding {
            finding: "Architectural dependency violation".to_string(),
            severity: "high".to_string(),
        };
        assert!(matches!(
            reviewer.classify_failure(&arch_conflict, &[]),
            FailureClass::ArchitecturalConflict
        ));

        let missing_knowledge = ReviewFinding {
            finding: "Missing domain knowledge about X".to_string(),
            severity: "medium".to_string(),
        };
        assert!(matches!(
            reviewer.classify_failure(&missing_knowledge, &[]),
            FailureClass::MissingKnowledge
        ));

        let ambiguous = ReviewFinding {
            finding: "Ambiguous intent in task description".to_string(),
            severity: "high".to_string(),
        };
        assert!(matches!(
            reviewer.classify_failure(&ambiguous, &[]),
            FailureClass::AmbiguousIntent
        ));

        let broken_process = ReviewFinding {
            finding: "Broken process detected".to_string(),
            severity: "medium".to_string(),
        };
        assert!(matches!(
            reviewer.classify_failure(&broken_process, &[]),
            FailureClass::BrokenProcess
        ));
    }

    #[test]
    fn test_reviewer_escalation_targets() {
        let reviewer = Reviewer::new();

        assert_eq!(
            reviewer
                .escalation_target_for(&FailureClass::ArchitecturalConflict)
                .0,
            "architect-001"
        );
        assert_eq!(
            reviewer
                .escalation_target_for(&FailureClass::MissingKnowledge)
                .0,
            "scholar-001"
        );
        assert_eq!(
            reviewer
                .escalation_target_for(&FailureClass::AmbiguousIntent)
                .0,
            "intent-lead-001"
        );
        assert_eq!(
            reviewer
                .escalation_target_for(&FailureClass::BrokenProcess)
                .0,
            "ops-manager-001"
        );
        assert_eq!(
            reviewer
                .escalation_target_for(&FailureClass::ImplementationDefect)
                .0,
            "worker-001"
        );
    }

    #[test]
    fn test_adr_serialisation() {
        let adr = Adr {
            id: "adr-001".to_string(),
            title: "Use SQLite for storage".to_string(),
            status: "accepted".to_string(),
            context: "Need lightweight storage".to_string(),
            decision: "Use SQLite".to_string(),
            alternatives: vec!["PostgreSQL".to_string(), "MongoDB".to_string()],
            tradeoffs: "Simplicity vs scalability".to_string(),
            consequences: "Limited concurrent writes".to_string(),
            references: vec!["intent-brief".to_string()],
        };

        let json = serde_json::to_string(&adr).unwrap();
        assert!(json.contains("Use SQLite"));
        assert!(json.contains("PostgreSQL"));

        let back: Adr = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, adr.id);
        assert_eq!(back.alternatives.len(), 2);
    }

    #[test]
    fn test_task_card_serialisation() {
        let card = TaskCard {
            id: "task-001".to_string(),
            description: "Implement storage layer".to_string(),
            contract: "Create storage module".to_string(),
            dependencies: vec!["task-000".to_string()],
            adr_references: vec!["adr-001".to_string()],
            validation_policy: None,
            acceptance_criteria: vec!["Tests pass".to_string()],
        };

        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("Implement storage layer"));

        let back: TaskCard = serde_json::from_str(&json).unwrap();
        assert_eq!(back.dependencies.len(), 1);
    }

    #[test]
    fn test_failure_class_serialisation() {
        let classes = vec![
            FailureClass::ImplementationDefect,
            FailureClass::ArchitecturalConflict,
            FailureClass::MissingKnowledge,
            FailureClass::AmbiguousIntent,
            FailureClass::BrokenProcess,
        ];

        for class in classes {
            let json = serde_json::to_string(&class).unwrap();
            let back: FailureClass = serde_json::from_str(&json).unwrap();
            assert_eq!(class, back);
            assert!(!class.as_str().is_empty());
        }
    }

    #[test]
    fn test_worker_path_traversal_is_blocked() {
        let worktree_path = std::env::temp_dir().join("test-worktree-safe");
        let _ = std::fs::create_dir_all(&worktree_path);

        let content = "FILE: ../../etc/passwd\nroot:x:0:0\n";
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(Worker::parse_and_write_files(content, &worktree_path));
        assert!(
            result.is_err(),
            "Path traversal should be rejected: {:?}",
            result
        );

        let content2 = "FILE: /etc/passwd\nroot:x:0:0\n";
        let result2 = rt.block_on(Worker::parse_and_write_files(content2, &worktree_path));
        assert!(
            result2.is_err(),
            "Absolute paths should be rejected: {:?}",
            result2
        );

        let safe_content = "FILE: src/lib.rs\npub fn safe() {}\n";
        let written = rt
            .block_on(Worker::parse_and_write_files(safe_content, &worktree_path))
            .unwrap();
        assert_eq!(written, vec!["src/lib.rs".to_string()]);
        assert!(worktree_path.join("src/lib.rs").exists());
    }

    #[tokio::test]
    async fn test_reviewer_extracts_implementation_from_task_completed() {
        let (bus, _memory_store) = setup_test_env();
        let reviewer = Arc::new(Reviewer::new());

        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);

        let receiver = bus.subscribe(&[EventType::TaskCompleted]);

        let bus_clone = bus.clone();
        let handle = tokio::spawn(async move {
            let mut sub =
                bus_clone.subscribe(&[EventType::ReviewRequested, EventType::ReviewCompleted]);
            let mut got_request = false;
            let mut got_completed = false;
            let mut findings_empty = true;

            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let patch_content =
                "# Implementation Patch\n\n## File: src/lib.rs\n\n```rust\nfn main() {}\n```\n";
            let artefact_ref = ArtefactRef {
                artefact_type: "implementation_patch".to_string(),
                reference: format!("patch-test-uuid|{}", patch_content),
            };

            let completed_event = SemanticEvent::new_task_completed(
                EventRoleId("worker-001".to_string()),
                "task-001",
                "contract-001",
                artefact_ref,
            );
            bus_clone.publish(completed_event).unwrap();

            let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
            while tokio::time::Instant::now() < deadline {
                tokio::select! {
                    result = sub.recv() => {
                        if let Ok(evt) = result {
                            match evt.as_ref() {
                                SemanticEvent::ReviewRequested { .. } => {
                                    got_request = true;
                                }
                                SemanticEvent::ReviewCompleted { findings, .. } => {
                                    got_completed = true;
                                    findings_empty = findings.iter().any(|f| f.finding.contains("No implementation content"));
                                }
                                _ => {}
                            }
                            if got_request && got_completed {
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {}
                }
            }
            (got_request, got_completed, findings_empty)
        });

        let ctx = RoleContext {
            bus: bus.clone(),
            receiver,
            memory_store: Arc::new(
                MemoryStore::open(tempdir().unwrap().path().join("test-reviewer.db")).unwrap(),
            ),
            coordinator,
            tools: Box::new(()),
        };

        let _ = tokio::time::timeout(tokio::time::Duration::from_secs(5), reviewer.run(ctx)).await;

        let (got_request, got_completed, findings_empty) = handle.await.unwrap();
        assert!(got_request, "Reviewer should publish ReviewRequested");
        assert!(got_completed, "Reviewer should publish ReviewCompleted");
        assert!(
            !findings_empty,
            "Reviewer should see implementation content, not complain about missing it"
        );
    }

    #[tokio::test]
    async fn test_pm_deduplicates_adrs() {
        let (bus, _memory_store) = setup_test_env();
        let pm = Arc::new(ProjectManager::new());

        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);

        let receiver = bus.subscribe(&[EventType::DecisionRecorded]);

        let bus_clone = bus.clone();
        let handle = tokio::spawn(async move {
            let mut assigned_count = 0;
            let mut sub = bus_clone.subscribe(&[EventType::TaskAssigned]);

            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            // Publish the same ADR twice via DecisionRecorded and ArtefactProduced
            let adr_event = SemanticEvent::new_decision_recorded(
                EventRoleId("architect-001".to_string()),
                "Use X",
                vec![],
            );
            bus_clone.publish(adr_event).unwrap();

            let adr_artefact = SemanticEvent::new_artefact_produced(
                EventRoleId("architect-001".to_string()),
                "adr",
                "adr-dedup-001|{\"id\":\"adr-dedup-001\",\"title\":\"Use X\",\"status\":\"proposed\",\"context\":\"Test\",\"decision\":\"Use X\",\"alternatives\":[],\"tradeoffs\":\"None\",\"consequences\":\"None\",\"references\":[]}",
                EventRoleId("architect-001".to_string()),
            );
            bus_clone.publish(adr_artefact).unwrap();

            let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
            while tokio::time::Instant::now() < deadline {
                tokio::select! {
                    result = sub.recv() => {
                        if let Ok(evt) = result {
                            if matches!(evt.as_ref(), SemanticEvent::TaskAssigned { .. }) {
                                assigned_count += 1;
                            }
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {}
                }
            }
            assigned_count
        });

        let ctx = RoleContext {
            bus: bus.clone(),
            receiver,
            memory_store: Arc::new(
                MemoryStore::open(tempdir().unwrap().path().join("test-pm-dedup.db")).unwrap(),
            ),
            coordinator,
            tools: Box::new(()),
        };

        let _ =
            tokio::time::timeout(tokio::time::Duration::from_secs(5), pm.clone().run(ctx)).await;

        let assigned_count = handle.await.unwrap();
        assert_eq!(
            assigned_count, 1,
            "Duplicate ADRs should produce exactly one TaskAssigned event"
        );

        let graph = pm.delivery_graph();
        let graph = graph.read();
        let task = graph
            .nodes
            .values()
            .next()
            .expect("PM should create a task");
        assert_eq!(
            task.task_card.adr_references,
            vec!["adr-dedup-001".to_string()],
            "PM should preserve the real ADR artefact id rather than synthetic DecisionRecorded id"
        );
    }

    #[tokio::test]
    async fn test_pm_marks_failed_task_in_delivery_graph() {
        let (bus, _memory_store) = setup_test_env();
        let pm = Arc::new(ProjectManager::new());

        let task = TaskCard {
            id: "task-failed-001".to_string(),
            description: "Test".to_string(),
            contract: "Test".to_string(),
            dependencies: vec![],
            adr_references: vec![],
            validation_policy: None,
            acceptance_criteria: vec![],
        };
        {
            let graph = pm.delivery_graph();
            let mut graph = graph.write();
            graph.add_node(task, vec![]);
            graph.update_status("task-failed-001", TaskStatus::Assigned);
        }

        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = CoordinatorHandle::new(tx);

        let receiver = bus.subscribe(&[EventType::TaskFailed]);

        let bus_clone = bus.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let failed = SemanticEvent::new_task_failed(
                EventRoleId("worker-001".to_string()),
                "task-failed-001",
                "validation failed",
            );
            bus_clone.publish(failed).unwrap();
        });

        let ctx = RoleContext {
            bus: bus.clone(),
            receiver,
            memory_store: Arc::new(
                MemoryStore::open(tempdir().unwrap().path().join("test-pm-failed.db")).unwrap(),
            ),
            coordinator,
            tools: Box::new(()),
        };

        let _ =
            tokio::time::timeout(tokio::time::Duration::from_secs(2), pm.clone().run(ctx)).await;

        let graph = pm.delivery_graph();
        let graph = graph.read();
        assert_eq!(
            graph.nodes["task-failed-001"].status,
            TaskStatus::Failed,
            "TaskFailed events should update delivery graph status"
        );
    }

    #[test]
    fn test_delivery_graph_status_updated_on_assignment() {
        let mut graph = DeliveryGraph::new();
        let task = TaskCard {
            id: "task-status-001".to_string(),
            description: "Test".to_string(),
            contract: "Test".to_string(),
            dependencies: vec![],
            adr_references: vec![],
            validation_policy: None,
            acceptance_criteria: vec![],
        };
        graph.add_node(task.clone(), vec![]);
        assert_eq!(graph.nodes["task-status-001"].status, TaskStatus::Pending);

        graph.update_status("task-status-001", TaskStatus::Assigned);
        assert_eq!(graph.nodes["task-status-001"].status, TaskStatus::Assigned);

        let ready = graph.ready_tasks();
        assert!(ready.is_empty(), "Assigned task should not be ready");
    }
}
