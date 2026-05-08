use std::{collections::HashMap, sync::Arc};

use mmat_coordinator::{CoordinatorHandle, Role, RoleContext};
use mmat_event_stream::{
    event::{ArtefactRef, EventType, RoleId as EventRoleId, SemanticEvent, TaskContract},
    event_bus::EventBus,
};
use mmat_memory::{
    error::Result as MemoryResult,
    librarian::Librarian,
    qdrant::VectorMemoryBackend,
    store::MemoryStore,
    types::{MemoryId, MemoryType},
};
use mmat_roles::{
    Architect, IntentLead, OpsManager, ProjectManager, Reviewer, Scholar, Worker,
    artefacts::TaskCard, project_manager::TaskStatus,
};
use parking_lot::Mutex;
use qdrant_client::qdrant::Value;
use tempfile::{TempDir, tempdir};

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

#[tokio::test]
async fn architect_receives_task_and_produces_adr() {
    let (_dir, bus, memory_store) = setup_role_test_env();
    let architect = Arc::new(Architect::new());
    let (coordinator, _coordinator_rx) = coordinator_pair();
    let receiver = bus.subscribe(&[EventType::TaskAssigned]);

    let bus_clone = bus.clone();
    let handle = tokio::spawn(async move {
        let mut received_decision = false;
        let mut received_artefact = false;
        let mut sub =
            bus_clone.subscribe(&[EventType::DecisionRecorded, EventType::ArtefactProduced]);

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

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            tokio::select! {
                result = sub.recv() => {
                    if let Ok(evt) = result {
                        match evt.as_ref() {
                            SemanticEvent::DecisionRecorded { .. } => {
                                received_decision = true;
                            }
                            SemanticEvent::ArtefactProduced { artefact_type, .. }
                                if artefact_type == "adr"
                                    || artefact_type == "interface_spec"
                                    || artefact_type == "dependency_rules" =>
                            {
                                received_artefact = true;
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
        memory_store,
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

fn coordinator_pair() -> (
    CoordinatorHandle,
    tokio::sync::mpsc::Receiver<mmat_coordinator::role::CoordinatorMessage>,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    (CoordinatorHandle::new(tx), rx)
}

#[tokio::test]
async fn intent_lead_turns_initial_prompt_into_brief_and_dispatches_roles() {
    let (_dir, bus, memory_store) = setup_role_test_env();
    let intent_lead = Arc::new(IntentLead::new());
    let (coordinator, _coordinator_rx) = coordinator_pair();
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
            tokio::time::timeout(tokio::time::Duration::from_millis(250), output_rx.recv()).await;

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
                produced_intent_brief |= artefact_type == "intent_brief";
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
async fn ops_manager_creates_sop_on_task() {
    let (_dir, bus, memory_store) = setup_role_test_env();
    let ops_manager = Arc::new(OpsManager::new());
    let (coordinator, _coordinator_rx) = coordinator_pair();
    let receiver = bus.subscribe(&[EventType::TaskAssigned]);

    let bus_for_spawn = bus.clone();
    let handle = tokio::spawn(async move {
        let mut sub = bus_for_spawn.subscribe(&[EventType::MemoryProposed]);

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

        for _ in 0..30 {
            if let Ok(evt) = sub.recv().await
                && let SemanticEvent::MemoryProposed { memory_type, .. } = evt.as_ref()
                && memory_type == "SOP"
            {
                return true;
            }
        }

        false
    });

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let _ = tokio::time::timeout(tokio::time::Duration::from_secs(10), ops_manager.run(ctx)).await;

    let sop_published = handle.await.unwrap();
    assert!(
        sop_published,
        "SOP MemoryProposed events should have been published"
    );
}

#[tokio::test]
async fn ops_manager_sop_memory_is_accepted_by_librarian() {
    let dir = tempdir().unwrap();
    let bus = Arc::new(EventBus::new(100));
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
    let (coordinator, _coordinator_rx) = coordinator_pair();
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

#[tokio::test]
async fn project_manager_deduplicates_adrs() {
    let (_dir, bus, memory_store) = setup_role_test_env();
    let project_manager = Arc::new(ProjectManager::new());
    let (coordinator, _coordinator_rx) = coordinator_pair();
    let receiver = bus.subscribe(&[EventType::DecisionRecorded]);

    let bus_clone = bus.clone();
    let handle = tokio::spawn(async move {
        let mut assigned_count = 0;
        let mut sub = bus_clone.subscribe(&[EventType::TaskAssigned]);

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

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
                    if let Ok(evt) = result
                        && matches!(evt.as_ref(), SemanticEvent::TaskAssigned { .. })
                    {
                        assigned_count += 1;
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
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let _ = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        project_manager.clone().run(ctx),
    )
    .await;

    let assigned_count = handle.await.unwrap();
    assert_eq!(
        assigned_count, 1,
        "Duplicate ADRs should produce exactly one TaskAssigned event"
    );

    let graph = project_manager.delivery_graph();
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
async fn project_manager_marks_failed_task_in_delivery_graph() {
    let (_dir, bus, memory_store) = setup_role_test_env();
    let project_manager = Arc::new(ProjectManager::new());

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
        let graph = project_manager.delivery_graph();
        let mut graph = graph.write();
        graph.add_node(task, vec![]);
        graph.update_status("task-failed-001", TaskStatus::Assigned);
    }

    let (coordinator, _coordinator_rx) = coordinator_pair();
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
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let _ = tokio::time::timeout(
        tokio::time::Duration::from_secs(2),
        project_manager.clone().run(ctx),
    )
    .await;

    let graph = project_manager.delivery_graph();
    let graph = graph.read();
    assert_eq!(
        graph.nodes["task-failed-001"].status,
        TaskStatus::Failed,
        "TaskFailed events should update delivery graph status"
    );
}

#[tokio::test]
async fn reviewer_extracts_implementation_from_task_completed() {
    let (_dir, bus, memory_store) = setup_role_test_env();
    let reviewer = Arc::new(Reviewer::new());
    let (coordinator, _coordinator_rx) = coordinator_pair();
    let receiver = bus.subscribe(&[EventType::TaskCompleted]);

    let bus_clone = bus.clone();
    let handle = tokio::spawn(async move {
        let mut sub =
            bus_clone.subscribe(&[EventType::ReviewRequested, EventType::ReviewCompleted]);
        let mut got_request = false;
        let mut got_completed = false;
        let mut missing_implementation_finding = true;

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
                                missing_implementation_finding = findings.iter().any(|f| f.finding.contains("No implementation content"));
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
        (got_request, got_completed, missing_implementation_finding)
    });

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let _ = tokio::time::timeout(tokio::time::Duration::from_secs(5), reviewer.run(ctx)).await;

    let (got_request, got_completed, missing_implementation_finding) = handle.await.unwrap();
    assert!(got_request, "Reviewer should publish ReviewRequested");
    assert!(got_completed, "Reviewer should publish ReviewCompleted");
    assert!(
        !missing_implementation_finding,
        "Reviewer should see implementation content, not complain about missing it"
    );
}

#[tokio::test]
async fn scholar_budget_exhaustion_requests_escalation() {
    let (_dir, bus, memory_store) = setup_role_test_env();
    let scholar = Arc::new(Scholar::new().with_budget(1, 0, 1));
    let (coordinator, _coordinator_rx) = coordinator_pair();
    let receiver = bus.subscribe(&[EventType::TaskAssigned]);

    let bus_clone = bus.clone();
    let handle = tokio::spawn(async move {
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

        let Ok(event) = sub.recv().await else {
            return false;
        };
        matches!(event.as_ref(), SemanticEvent::EscalationRequested { reason, .. } if reason.contains("budget"))
    });

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
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
async fn scholar_receives_task_and_completes_research_outputs() {
    let (_dir, bus, memory_store) = setup_role_test_env();
    let scholar = Arc::new(Scholar::new());
    let (coordinator, _coordinator_rx) = coordinator_pair();
    let receiver = bus.subscribe(&[EventType::TaskAssigned]);

    let bus_clone = bus.clone();
    let handle = tokio::spawn(async move {
        let mut received_artefacts = Vec::new();
        let mut sub = bus_clone.subscribe(&[EventType::ArtefactProduced]);

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

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            tokio::select! {
                result = sub.recv() => {
                    if let Ok(evt) = result
                        && let SemanticEvent::ArtefactProduced { artefact_type, .. } = evt.as_ref()
                    {
                        received_artefacts.push(artefact_type.clone());
                        if received_artefacts.len() >= 3 {
                            break;
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
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let result = tokio::time::timeout(tokio::time::Duration::from_secs(10), scholar.run(ctx)).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_ok());

    let artefacts = handle.await.unwrap();
    assert!(artefacts.contains(&"research_brief".to_string()));
    assert!(artefacts.contains(&"evidence_pack".to_string()));
    assert!(artefacts.contains(&"open_questions".to_string()));
}

fn setup_role_test_env() -> (TempDir, EventBus, Arc<MemoryStore>) {
    let dir = tempdir().unwrap();
    let bus = EventBus::new(100);
    let memory_store = Arc::new(MemoryStore::open(dir.path().join("test.db")).unwrap());
    (dir, bus, memory_store)
}

#[tokio::test]
async fn worker_does_not_fallback_by_default() {
    let (_dir, bus, memory_store) = setup_role_test_env();
    let worker = Arc::new(Worker::new().with_validation_commands(vec![]));
    let (coordinator, _coordinator_rx) = coordinator_pair();
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
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let result = tokio::time::timeout(tokio::time::Duration::from_secs(10), worker.run(ctx)).await;
    assert!(
        matches!(result, Ok(Err(_))),
        "Worker should return the git worktree error when fallback is disabled: {:?}",
        result
    );
}

#[tokio::test]
async fn worker_receives_task_and_completes() {
    let (_dir, bus, memory_store) = setup_role_test_env();
    let worker = Arc::new(
        Worker::new()
            .with_validation_commands(vec![])
            .with_fallback_worktree(true),
    );
    let (coordinator, _coordinator_rx) = coordinator_pair();
    let receiver = bus.subscribe(&[EventType::TaskAssigned]);

    let bus_clone = bus.clone();
    let handle = tokio::spawn(async move {
        let mut received_completed = false;
        let mut received_tool_executed = false;
        let mut received_claim = false;
        let mut received_artefact = false;
        let mut sub = bus_clone.subscribe(&[
            EventType::TaskCompleted,
            EventType::ToolExecuted,
            EventType::ClaimMade,
            EventType::ArtefactProduced,
        ]);

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
                            SemanticEvent::ArtefactProduced { artefact_type, .. }
                                if artefact_type == "implementation_patch" =>
                            {
                                received_artefact = true;
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
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let result = tokio::time::timeout(tokio::time::Duration::from_secs(10), worker.run(ctx)).await;
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
