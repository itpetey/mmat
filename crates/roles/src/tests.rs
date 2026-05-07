#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use coordinator::{Role, RoleContext};
    use event_stream::event::{EventType, RoleId as EventRoleId, SemanticEvent, TaskContract};
    use event_stream::event_bus::EventBus;
    use memory::librarian::Librarian;
    use memory::qdrant::VectorMemoryBackend;
    use memory::store::MemoryStore;
    use memory::types::MemoryId;
    use parking_lot::Mutex;
    use qdrant_client::qdrant::Value;
    use tempfile::tempdir;

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
        ) -> memory::error::Result<()> {
            self.upserts.lock().push(id);
            Ok(())
        }

        async fn search(
            &self,
            _query_embedding: Vec<f32>,
            _limit: u64,
        ) -> memory::error::Result<Vec<(MemoryId, f32)>> {
            Ok(self.upserts.lock().iter().map(|id| (*id, 1.0)).collect())
        }

        async fn delete(&self, id: MemoryId) -> memory::error::Result<()> {
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
        assert_eq!(spec.role_type, coordinator::RoleType::IntentLead);
        assert!(matches!(
            spec.authority_scope,
            coordinator::AuthorityScope::IntentOnly
        ));
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

        let mut registry = coordinator::RoleRegistry::new();
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
        assert_eq!(spec.role_type, coordinator::RoleType::Scholar);
        assert!(matches!(
            spec.authority_scope,
            coordinator::AuthorityScope::Architecture
        ));
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
        assert_eq!(spec.role_type, coordinator::RoleType::OpsManager);
        assert!(matches!(
            spec.authority_scope,
            coordinator::AuthorityScope::Architecture
        ));
        assert!(spec.output_contract.contains(&EventType::DecisionRecorded));
        assert!(spec.output_contract.contains(&EventType::MemoryProposed));
        assert!(spec.output_contract.contains(&EventType::ArtefactProduced));
    }

    #[tokio::test]
    async fn test_intent_lead_initial_prompt_to_brief_and_dispatch() {
        let (bus, memory_store) = setup_test_env();
        let intent_lead = Arc::new(IntentLead::new());
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let coordinator = coordinator::CoordinatorHandle::new(tx);
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
        let coordinator = coordinator::CoordinatorHandle::new(tx);

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
        let coordinator = coordinator::CoordinatorHandle::new(tx);

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
        let coordinator = coordinator::CoordinatorHandle::new(tx);

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
        let coordinator = coordinator::CoordinatorHandle::new(tx);
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
            let sops = memory_store
                .query_by_type(memory::types::MemoryType::SOP)
                .unwrap();
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
}
