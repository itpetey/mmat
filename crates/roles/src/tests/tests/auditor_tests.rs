use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use coordinator::{Role, RoleContext};
use event_stream::event::{EventId, EventType, RoleId as EventRoleId, SemanticEvent};
use event_stream::event_bus::EventBus;
use llm::client::LlmClient;
use llm::message::{Choice, CompletionRequest, CompletionResponse, Message, Usage};
use memory::librarian::Librarian;
use memory::qdrant::VectorMemoryBackend;
use memory::store::MemoryStore;
use memory::types::MemoryId;
use qdrant_client::qdrant::Value;
use tempfile::tempdir;
use tokio::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::artefacts::{AuditReport, EvidenceFinding, EvidencePack};
use crate::{Auditor, AuditorLlmConfig};

#[derive(Default)]
struct MockLlmClient {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl LlmClient for MockLlmClient {
    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> llm::error::Result<CompletionResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(CompletionResponse {
            id: "mock".to_string(),
            object: "chat.completion".to_string(),
            created: 0,
            model: "mock".to_string(),
            choices: vec![Choice {
                index: 0,
                message: Message::Assistant {
                    content: Some("INCONSISTENT".to_string()),
                    tool_calls: None,
                    name: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        })
    }
}

#[derive(Default)]
struct FakeVectorBackend {
    deleted: parking_lot::Mutex<Vec<MemoryId>>,
}

#[async_trait::async_trait]
impl VectorMemoryBackend for FakeVectorBackend {
    async fn upsert(
        &self,
        _id: MemoryId,
        _embedding: Vec<f32>,
        _payload: HashMap<String, Value>,
    ) -> memory::error::Result<()> {
        Ok(())
    }

    async fn search(
        &self,
        _query_embedding: Vec<f32>,
        _limit: u64,
    ) -> memory::error::Result<Vec<(MemoryId, f32)>> {
        Ok(Vec::new())
    }

    async fn delete(&self, id: MemoryId) -> memory::error::Result<()> {
        self.deleted.lock().push(id);
        Ok(())
    }
}

fn setup_auditor_test_env() -> (
    EventBus,
    Arc<event_stream::event_store::EventStore>,
    Arc<MemoryStore>,
) {
    let dir = tempdir().unwrap();
    let event_store = Arc::new(
        event_stream::event_store::EventStore::open(dir.path().join("events.db")).unwrap(),
    );
    let bus = EventBus::new(100).with_store(event_store.clone());
    let memory_store = Arc::new(MemoryStore::open(dir.path().join("memory.db")).unwrap());
    (bus, event_store, memory_store)
}

#[tokio::test]
async fn test_auditor_detects_process_skipped_when_tests_not_run() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);

    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut output_rx = bus.subscribe(&[EventType::ProcessSkipped]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let claim = SemanticEvent::new_claim_made(
        EventRoleId("worker-001".to_string()),
        "tests passed",
        vec![],
        0.9,
    );
    bus.publish(claim).unwrap();

    let mut detected = false;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(event)) =
            tokio::time::timeout(tokio::time::Duration::from_millis(200), output_rx.recv()).await
        {
            if let SemanticEvent::ProcessSkipped { step, .. } = event.as_ref() {
                if step == "cargo test" {
                    detected = true;
                    break;
                }
            }
        }
    }

    run_handle.abort();
    assert!(
        detected,
        "Auditor should detect ProcessSkipped for 'tests passed' without cargo test"
    );
}

#[tokio::test]
async fn test_auditor_detects_contradiction_when_tests_fail() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);

    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut output_rx = bus.subscribe(&[EventType::PolicyViolationDetected]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let tool_event = SemanticEvent::new_tool_executed(
        EventRoleId("worker-001".to_string()),
        "cargo test",
        "",
        1,
        "",
        "test failure",
        0,
    );
    let tool_id = tool_event.event_id();
    bus.publish(tool_event).unwrap();

    let claim = SemanticEvent::new_claim_made(
        EventRoleId("worker-001".to_string()),
        "tests passed",
        vec![event_stream::event::EvidenceRef {
            event_id: tool_id,
            description: "test results".to_string(),
        }],
        0.9,
    );
    bus.publish(claim).unwrap();

    let mut detected = false;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(event)) =
            tokio::time::timeout(tokio::time::Duration::from_millis(200), output_rx.recv()).await
        {
            if let SemanticEvent::PolicyViolationDetected { violation_type, .. } = event.as_ref() {
                if violation_type == "contradiction" {
                    detected = true;
                    break;
                }
            }
        }
    }

    run_handle.abort();
    assert!(
        detected,
        "Auditor should detect contradiction when tests fail but claim says passed"
    );
}

#[tokio::test]
async fn test_auditor_rejects_non_tool_evidence_ref() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);
    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut output_rx = bus.subscribe(&[EventType::EvidenceChainBroken]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };
    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let decision = SemanticEvent::new_decision_recorded(
        EventRoleId("architect-001".to_string()),
        "Use SQLite",
        vec![],
    );
    let decision_id = decision.event_id();
    bus.publish(decision).unwrap();

    let claim = SemanticEvent::new_claim_made(
        EventRoleId("worker-001".to_string()),
        "SQLite is used",
        vec![event_stream::event::EvidenceRef {
            event_id: decision_id,
            description: "decision reference".to_string(),
        }],
        0.7,
    );
    bus.publish(claim).unwrap();

    let detected = tokio::time::timeout(tokio::time::Duration::from_secs(3), output_rx.recv())
        .await
        .is_ok();
    run_handle.abort();
    assert!(
        detected,
        "Claim evidence must reference ToolExecuted events"
    );
}

#[tokio::test]
async fn test_auditor_does_not_accept_uncited_stale_test_run() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);
    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut output_rx = bus.subscribe(&[EventType::ProcessSkipped]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };
    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    bus.publish(SemanticEvent::new_tool_executed(
        EventRoleId("worker-001".to_string()),
        "cargo test",
        "",
        0,
        "old passing run",
        "",
        0,
    ))
    .unwrap();

    bus.publish(SemanticEvent::new_claim_made(
        EventRoleId("worker-001".to_string()),
        "tests passed",
        vec![],
        0.7,
    ))
    .unwrap();

    let detected = tokio::time::timeout(tokio::time::Duration::from_secs(3), output_rx.recv())
        .await
        .is_ok();
    run_handle.abort();
    assert!(
        detected,
        "Uncited stale test runs must not satisfy process adherence"
    );
}

#[tokio::test]
async fn test_auditor_detects_evidence_chain_broken_for_missing_file() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);

    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut output_rx = bus.subscribe(&[EventType::EvidenceChainBroken]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let fake_id = event_stream::event::EventId::new();
    let claim = SemanticEvent::new_claim_made(
        EventRoleId("scholar-001".to_string()),
        "Reference to /nonexistent/file.rs",
        vec![event_stream::event::EvidenceRef {
            event_id: fake_id,
            description: "file path".to_string(),
        }],
        0.9,
    );
    bus.publish(claim).unwrap();

    let mut detected = false;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(event)) =
            tokio::time::timeout(tokio::time::Duration::from_millis(200), output_rx.recv()).await
        {
            if let SemanticEvent::EvidenceChainBroken { .. } = event.as_ref() {
                detected = true;
                break;
            }
        }
    }

    run_handle.abort();
    assert!(
        detected,
        "Auditor should detect EvidenceChainBroken for missing file reference"
    );
}

#[tokio::test]
async fn test_auditor_flags_unjustified_confidence() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);

    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut output_rx = bus.subscribe(&[EventType::PolicyViolationDetected]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let claim = SemanticEvent::new_claim_made(
        EventRoleId("worker-001".to_string()),
        "everything is fine",
        vec![],
        0.95,
    );
    bus.publish(claim).unwrap();

    let mut detected = false;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(event)) =
            tokio::time::timeout(tokio::time::Duration::from_millis(200), output_rx.recv()).await
        {
            if let SemanticEvent::PolicyViolationDetected { violation_type, .. } = event.as_ref() {
                if violation_type == "unjustified_confidence" {
                    detected = true;
                    break;
                }
            }
        }
    }

    run_handle.abort();
    assert!(
        detected,
        "Auditor should flag unjustified high confidence with no evidence"
    );
}

#[tokio::test]
async fn test_auditor_flags_authority_violation() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);

    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut output_rx = bus.subscribe(&[EventType::PolicyViolationDetected]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let decision = SemanticEvent::new_decision_recorded(
        EventRoleId("worker-001".to_string()),
        "I decided to rewrite everything",
        vec![],
    );
    bus.publish(decision).unwrap();

    let mut detected = false;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(event)) =
            tokio::time::timeout(tokio::time::Duration::from_millis(200), output_rx.recv()).await
        {
            if let SemanticEvent::PolicyViolationDetected { violation_type, .. } = event.as_ref() {
                if violation_type == "authority_boundary_exceeded" {
                    detected = true;
                    break;
                }
            }
        }
    }

    run_handle.abort();
    assert!(
        detected,
        "Auditor should flag authority violation when Worker publishes DecisionRecorded"
    );
}

#[tokio::test]
async fn test_auditor_detects_memory_contamination_without_mutation() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);

    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut output_rx = bus.subscribe(&[EventType::PolicyViolationDetected]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store: memory_store.clone(),
        coordinator,
        tools: Box::new(()),
    };

    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let fake_evidence_id = event_stream::event::EventId::new();
    let claim = SemanticEvent::new_claim_made(
        EventRoleId("worker-001".to_string()),
        "broken claim",
        vec![event_stream::event::EvidenceRef {
            event_id: fake_evidence_id,
            description: "nonexistent".to_string(),
        }],
        0.5,
    );
    let claim_id = claim.event_id();
    bus.publish(claim).unwrap();

    let memory = memory::types::Memory::builder()
        .memory_type(memory::types::MemoryType::Fact)
        .content("derived from broken claim")
        .scope(memory::types::MemoryScope::Project)
        .authority(memory::types::Authority::LLMInference)
        .confidence(memory::types::Confidence::new(0.7).unwrap())
        .evidence_refs(vec![claim_id])
        .source_agent(EventRoleId("worker-001".to_string()))
        .build()
        .unwrap();
    memory_store.insert(&memory).unwrap();

    let accepted = SemanticEvent::new_memory_accepted(
        EventRoleId("librarian-001".to_string()),
        EventId::from(memory.id.0),
        EventRoleId("librarian-001".to_string()),
    );
    bus.publish(accepted).unwrap();

    let mut detected = false;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(event)) =
            tokio::time::timeout(tokio::time::Duration::from_millis(200), output_rx.recv()).await
        {
            if let SemanticEvent::PolicyViolationDetected { violation_type, .. } = event.as_ref() {
                if violation_type == "memory_contamination" {
                    detected = true;
                    break;
                }
            }
        }
    }

    run_handle.abort();
    assert!(detected, "Auditor should detect memory contamination");

    let retrieved = memory_store.get_by_id(memory.id).unwrap().unwrap();
    assert_eq!(
        retrieved.content, "derived from broken claim",
        "Auditor must NOT mutate memory"
    );
}

#[tokio::test]
async fn test_auditor_does_not_flag_valid_claim() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);

    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut output_rx = bus.subscribe(&[
        EventType::PolicyViolationDetected,
        EventType::EvidenceChainBroken,
        EventType::ProcessSkipped,
    ]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };

    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let tool_event = SemanticEvent::new_tool_executed(
        EventRoleId("worker-001".to_string()),
        "cargo test",
        "",
        0,
        "all tests passed",
        "",
        0,
    );
    let tool_id = tool_event.event_id();
    bus.publish(tool_event).unwrap();

    let claim = SemanticEvent::new_claim_made(
        EventRoleId("worker-001".to_string()),
        "tests passed",
        vec![event_stream::event::EvidenceRef {
            event_id: tool_id,
            description: "test results".to_string(),
        }],
        0.7,
    );
    bus.publish(claim).unwrap();

    let mut any_flag = false;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(_event)) =
            tokio::time::timeout(tokio::time::Duration::from_millis(200), output_rx.recv()).await
        {
            any_flag = true;
            break;
        }
    }

    run_handle.abort();
    assert!(
        !any_flag,
        "Auditor should NOT flag a valid claim with proper evidence"
    );
}

#[tokio::test]
async fn test_low_confidence_with_strong_evidence_is_report_only() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);
    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut reports = bus.subscribe(&[EventType::ArtefactProduced]);
    let mut violations = bus.subscribe(&[EventType::PolicyViolationDetected]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };
    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let tool = SemanticEvent::new_tool_executed(
        EventRoleId("worker-001".to_string()),
        "cargo test",
        "",
        0,
        "all tests passed",
        "",
        0,
    );
    let tool_id = tool.event_id();
    bus.publish(tool).unwrap();
    bus.publish(SemanticEvent::new_claim_made(
        EventRoleId("worker-001".to_string()),
        "tests passed",
        vec![event_stream::event::EvidenceRef {
            event_id: tool_id,
            description: "test results".to_string(),
        }],
        0.3,
    ))
    .unwrap();
    bus.publish(SemanticEvent::new_task_completed(
        EventRoleId("worker-001".to_string()),
        "task-low-confidence",
        "contract-low-confidence",
        event_stream::event::ArtefactRef {
            artefact_type: "implementation_patch".to_string(),
            reference: "patch".to_string(),
        },
    ))
    .unwrap();

    let mut saw_confidence_report = false;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(event)) =
            tokio::time::timeout(tokio::time::Duration::from_millis(200), reports.recv()).await
        {
            if let SemanticEvent::ArtefactProduced {
                artefact_type,
                reference,
                ..
            } = event.as_ref()
                && artefact_type == "audit_report"
            {
                let report: AuditReport = serde_json::from_str(reference).unwrap();
                saw_confidence_report = !report.confidence_assessments.is_empty();
                break;
            }
        }
    }

    let no_policy_violation =
        tokio::time::timeout(tokio::time::Duration::from_millis(250), violations.recv())
            .await
            .is_err();

    run_handle.abort();
    assert!(
        saw_confidence_report,
        "Mismatch should be noted in AuditReport"
    );
    assert!(
        no_policy_violation,
        "Low confidence with strong evidence is report-only"
    );
}

#[tokio::test]
async fn test_llm_semantic_check_is_budgeted_and_flags_inconsistency() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let llm = Arc::new(MockLlmClient::default());
    let auditor = Arc::new(Auditor::new().with_llm_client(llm.clone()).with_llm_config(
        AuditorLlmConfig {
            enabled: true,
            model: "mock".to_string(),
            max_checks_per_cycle: 1,
        },
    ));

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);
    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut violations = bus.subscribe(&[EventType::PolicyViolationDetected]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };
    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let tool = SemanticEvent::new_tool_executed(
        EventRoleId("worker-001".to_string()),
        "probe",
        "",
        0,
        "feature flag is disabled",
        "",
        0,
    );
    let tool_id = tool.event_id();
    bus.publish(tool).unwrap();

    for claim_text in [
        "feature flag is enabled",
        "feature flag is definitely enabled",
    ] {
        bus.publish(SemanticEvent::new_claim_made(
            EventRoleId("worker-001".to_string()),
            claim_text,
            vec![event_stream::event::EvidenceRef {
                event_id: tool_id,
                description: "probe output".to_string(),
            }],
            0.6,
        ))
        .unwrap();
    }

    let violation = tokio::time::timeout(Duration::from_secs(3), violations.recv())
        .await
        .expect("timeout waiting for semantic violation")
        .expect("channel closed");
    run_handle.abort();

    assert!(matches!(
        violation.as_ref(),
        SemanticEvent::PolicyViolationDetected { violation_type, .. }
            if violation_type == "semantic_inconsistency"
    ));
    assert_eq!(
        llm.calls.load(Ordering::SeqCst),
        1,
        "LLM semantic checks must honour the per-cycle budget"
    );
}

#[tokio::test]
async fn test_auditor_detects_hallucinated_api_endpoint() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);
    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut violations = bus.subscribe(&[EventType::PolicyViolationDetected]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };
    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let tool = SemanticEvent::new_tool_executed(
        EventRoleId("worker-001".to_string()),
        "repo scan",
        "",
        0,
        "no endpoint found",
        "",
        0,
    );
    let tool_id = tool.event_id();
    bus.publish(tool).unwrap();
    let endpoint = format!("/missing-{}", uuid::Uuid::new_v4());
    bus.publish(SemanticEvent::new_claim_made(
        EventRoleId("worker-001".to_string()),
        format!("The API supports endpoint {endpoint}"),
        vec![event_stream::event::EvidenceRef {
            event_id: tool_id,
            description: "repository scan".to_string(),
        }],
        0.7,
    ))
    .unwrap();

    let violation = tokio::time::timeout(Duration::from_secs(3), violations.recv())
        .await
        .expect("timeout waiting for capability violation")
        .expect("channel closed");
    run_handle.abort();
    assert!(matches!(
        violation.as_ref(),
        SemanticEvent::PolicyViolationDetected { violation_type, .. }
            if violation_type == "hallucinated_capability"
    ));
}

#[tokio::test]
async fn test_auditor_verifies_scholar_web_sources() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/missing"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let auditor = Arc::new(Auditor::new().with_source_verification(true));

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);
    let receiver = bus.subscribe(&[EventType::OrganisationStarted]);
    let mut broken = bus.subscribe(&[EventType::EvidenceChainBroken]);

    let ctx = RoleContext {
        bus: bus.clone(),
        receiver,
        memory_store,
        coordinator,
        tools: Box::new(()),
    };
    let run_handle = tokio::spawn(auditor.run(ctx));

    tokio::time::sleep(Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let pack = EvidencePack {
        findings: vec![EvidenceFinding {
            claim: "source exists".to_string(),
            source_reference: format!("{}/missing", server.uri()),
            extracted_content: "missing".to_string(),
            confidence: 0.8,
            relevance: "citation".to_string(),
        }],
    };
    let reference = format!("evidence_pack|{}", serde_json::to_string(&pack).unwrap());
    bus.publish(SemanticEvent::new_artefact_produced(
        EventRoleId("scholar-001".to_string()),
        "evidence_pack",
        reference,
        EventRoleId("scholar-001".to_string()),
    ))
    .unwrap();

    let detected = tokio::time::timeout(Duration::from_secs(3), broken.recv())
        .await
        .is_ok();
    run_handle.abort();
    assert!(detected, "Unreachable Scholar source should be flagged");
}

#[tokio::test]
async fn test_auditor_memory_contamination_is_consumed_by_librarian() {
    let (bus, _event_store, memory_store) = setup_auditor_test_env();
    let bus = Arc::new(bus);
    let qdrant = Arc::new(FakeVectorBackend::default());
    let librarian = Librarian::new(
        memory_store.clone(),
        qdrant.clone(),
        Duration::from_secs(3600),
    );
    let auditor = Arc::new(Auditor::new());

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let coordinator = coordinator::CoordinatorHandle::new(tx);
    let auditor_ctx = RoleContext {
        bus: (*bus).clone(),
        receiver: bus.subscribe(&[EventType::OrganisationStarted]),
        memory_store: memory_store.clone(),
        coordinator,
        tools: Box::new(()),
    };

    let auditor_handle = tokio::spawn(auditor.run(auditor_ctx));
    let librarian_handle = tokio::spawn({
        let bus = bus.clone();
        async move { librarian.run(bus).await }
    });
    let mut superseded = bus.subscribe(&[EventType::MemorySuperseded]);

    tokio::time::sleep(Duration::from_millis(50)).await;
    bus.publish(SemanticEvent::new_organisation_started(EventRoleId(
        "system".to_string(),
    )))
    .unwrap();

    let fake_evidence_id = EventId::new();
    let claim = SemanticEvent::new_claim_made(
        EventRoleId("worker-001".to_string()),
        "broken claim",
        vec![event_stream::event::EvidenceRef {
            event_id: fake_evidence_id,
            description: "nonexistent".to_string(),
        }],
        0.5,
    );
    let claim_id = claim.event_id();
    bus.publish(claim).unwrap();

    let memory = memory::types::Memory::builder()
        .memory_type(memory::types::MemoryType::Fact)
        .content("derived from broken claim")
        .scope(memory::types::MemoryScope::Project)
        .authority(memory::types::Authority::LLMInference)
        .confidence(memory::types::Confidence::new(0.7).unwrap())
        .evidence_refs(vec![claim_id])
        .source_agent(EventRoleId("worker-001".to_string()))
        .build()
        .unwrap();
    memory_store.insert(&memory).unwrap();
    bus.publish(SemanticEvent::new_memory_accepted(
        EventRoleId("librarian-001".to_string()),
        EventId::from(memory.id.0),
        EventRoleId("librarian-001".to_string()),
    ))
    .unwrap();

    let event = tokio::time::timeout(Duration::from_secs(3), superseded.recv())
        .await
        .expect("timeout waiting for Librarian supersession")
        .expect("channel closed");
    auditor_handle.abort();
    librarian_handle.abort();

    assert!(matches!(
        event.as_ref(),
        SemanticEvent::MemorySuperseded { old_memory_id, .. } if *old_memory_id == memory.id.0.into()
    ));
    assert!(
        memory_store
            .get_by_id(memory.id)
            .unwrap()
            .unwrap()
            .superseded_by
            .is_some(),
        "Librarian should consume the audit violation and act on contaminated memory"
    );
    assert_eq!(qdrant.deleted.lock().as_slice(), &[memory.id]);
}

#[test]
fn test_auditor_spec_registers() {
    let auditor = Auditor::new();
    let mut registry = coordinator::RoleRegistry::new();
    registry.register(auditor.spec()).unwrap();
}
