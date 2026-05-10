use std::sync::Arc;
use std::time::{Duration, Instant};

use mmat_coordinator::{OrganisationConfig, OrganisationRuntime, Role, RoleRegistry};
use mmat_event_stream::event::{RoleId, SemanticEvent, TaskContract};
use mmat_event_stream::event_bus::EventBus;
use mmat_memory::artefact_store::ArtefactStore;
use mmat_roles::IntentLead;
use mmat_workbench::AppState;
use uuid::Uuid;

mod common;

// ---------------------------------------------------------------------------
// 2.1  /api/state and /api/messages
// ---------------------------------------------------------------------------

#[tokio::test]
async fn state_endpoint_returns_json_projection() {
    let state = common::test_app_state();
    let base_url = common::spawn_test_server(state).await;

    let resp = reqwest::get(&format!("{base_url}/api/state"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("messages").is_some(),
        "projection should contain messages",
    );
    assert!(
        body.get("has_conversation").is_some(),
        "projection should contain has_conversation",
    );
    assert!(
        body.get("project").is_some(),
        "projection should contain project metadata",
    );
}

#[tokio::test]
async fn post_message_returns_accepted() {
    let state = common::test_app_state();
    let base_url = common::spawn_test_server(state).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(&format!("{base_url}/api/messages"))
        .json(&serde_json::json!({ "message": "Hello from integration test" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 202);
}

#[tokio::test]
async fn post_empty_message_returns_bad_request() {
    let state = common::test_app_state();
    let base_url = common::spawn_test_server(state).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(&format!("{base_url}/api/messages"))
        .json(&serde_json::json!({ "message": "" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn post_message_updates_state() {
    let state = common::test_app_state();
    let base_url = common::spawn_test_server(state).await;

    let client = reqwest::Client::new();
    client
        .post(&format!("{base_url}/api/messages"))
        .json(&serde_json::json!({ "message": "A test message" }))
        .send()
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client
        .get(&format!("{base_url}/api/state"))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();

    let messages = body["messages"].as_array().unwrap();
    assert!(!messages.is_empty(), "should have at least one message");
    assert_eq!(messages[0]["speaker"], "You");
    assert!(
        body["has_conversation"].as_bool().unwrap(),
        "should have a conversation flag",
    );
}

// ---------------------------------------------------------------------------
// 2.2  Notification / action acknowledgement
// ---------------------------------------------------------------------------

#[tokio::test]
async fn acknowledge_existing_notification_returns_no_content() {
    let event = SemanticEvent::new_human_feedback_requested(
        RoleId::new("intent-lead-001"),
        "What are we making?",
        "test",
    );
    let events = vec![event];
    let state = common::test_app_state_with_events(&events);
    let base_url = common::spawn_test_server(state).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(&format!("{base_url}/api/state"))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let notification_id = body["notifications"][0]["id"].as_str().unwrap().to_string();

    let ack_resp = client
        .post(&format!(
            "{base_url}/api/notifications/{notification_id}/ack"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(ack_resp.status(), 204);
}

#[tokio::test]
async fn acknowledge_missing_notification_returns_not_found() {
    let state = common::test_app_state();
    let base_url = common::spawn_test_server(state).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(&format!("{base_url}/api/notifications/nonexistent-id/ack"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

// ---------------------------------------------------------------------------
// 2.3  Bounded /events SSE
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sse_returns_initial_state_event() {
    let state = common::test_app_state();
    let base_url = common::spawn_test_server(state).await;

    let client = reqwest::Client::new();
    let mut resp = client
        .get(&format!("{base_url}/events"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let first_chunk = tokio::time::timeout(Duration::from_secs(3), resp.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let text = String::from_utf8_lossy(&first_chunk);
    assert!(
        text.contains("data:"),
        "SSE stream should contain data lines: {text:?}",
    );
    assert!(
        text.contains("\"type\":\"State\"") || text.contains("\"type\":\"state\""),
        "first SSE event should be a State snapshot: {text:?}",
    );
}

#[tokio::test]
async fn sse_delivers_live_event_after_publish() {
    let bus = EventBus::new(16);
    let store = Arc::new(ArtefactStore::new());
    let state = AppState::with_events(bus.clone(), &[], store);
    let base_url = common::spawn_test_server(state).await;

    let client = reqwest::Client::new();
    let mut resp = client
        .get(&format!("{base_url}/events"))
        .send()
        .await
        .unwrap();

    // Read first chunk (initial State) — confirms subscription is established
    let _first = tokio::time::timeout(Duration::from_secs(3), resp.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    // NOW publish (race-free: SSE subscriber is guaranteed to exist)
    bus.publish(SemanticEvent::new_organisation_started(RoleId::new(
        "coordinator",
    )))
    .unwrap();

    // Accumulate chunks until the Event SSE frame appears (handles
    // arbitrary chunk boundaries and avoids flaky single-chunk reads).
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut accumulated = Vec::new();
    while Instant::now() < deadline {
        let remaining = deadline - Instant::now();
        match tokio::time::timeout(remaining, resp.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                accumulated.extend_from_slice(&chunk);
                if String::from_utf8_lossy(&accumulated).contains("\"type\":\"Event\"") {
                    break;
                }
            }
            _ => break,
        }
    }

    let text = String::from_utf8_lossy(&accumulated);
    assert!(
        text.contains("\"type\":\"Event\""),
        "live SSE event should be an Event, got: {text:?}",
    );
}

// ---------------------------------------------------------------------------
// 2.4  Static asset routes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn index_returns_html() {
    let state = common::test_app_state();
    let base_url = common::spawn_test_server(state).await;

    let resp = reqwest::get(&format!("{base_url}/")).await.unwrap();
    assert_eq!(resp.status(), 200);

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        content_type.starts_with("text/html"),
        "expected text/html, got {content_type}",
    );

    let body = resp.text().await.unwrap();
    assert!(
        body.contains("makemeathing"),
        "HTML should reference the app"
    );
}

#[tokio::test]
async fn style_css_returns_css() {
    let state = common::test_app_state();
    let base_url = common::spawn_test_server(state).await;

    let resp = reqwest::get(&format!("{base_url}/style.css"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        content_type.starts_with("text/css"),
        "expected text/css, got {content_type}",
    );

    let body = resp.text().await.unwrap();
    assert!(body.contains(":root"), "CSS should contain :root selector",);
}

#[tokio::test]
async fn app_js_returns_javascript() {
    let state = common::test_app_state();
    let base_url = common::spawn_test_server(state).await;

    let resp = reqwest::get(&format!("{base_url}/app.js")).await.unwrap();
    assert_eq!(resp.status(), 200);

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        content_type.starts_with("application/javascript"),
        "expected application/javascript, got {content_type}",
    );

    let body = resp.text().await.unwrap();
    assert!(body.contains("loadState"), "JS should contain loadState");
}

// ---------------------------------------------------------------------------
// 3.1  Replay / resume — end-to-end through /api/state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn replay_restores_full_projection_through_api() {
    let Some((database_url, admin_pool, schema)) =
        common::postgres_test_database("workbench_replay_api").await
    else {
        println!("[SKIP] replay_restores_full_projection_through_api requires DATABASE_URL");
        return;
    };

    let task_id = Uuid::new_v4().to_string();
    let artefact_id = Uuid::new_v4().to_string();

    // First runtime: persist events covering all projection areas
    {
        let config = OrganisationConfig {
            database_url: Some(database_url.clone()),
            event_store_path: None,
            memory_store_path: None,
            ..Default::default()
        };
        let intent_lead = IntentLead::new();
        let mut registry = RoleRegistry::new();
        registry.register(intent_lead.spec()).unwrap();
        let runtime = OrganisationRuntime::new(config, registry).unwrap();
        let bus = runtime.bus();

        bus.publish(SemanticEvent::new_human_feedback_requested(
            RoleId::new("intent-lead-001"),
            "What are we making?",
            "test",
        ))
        .unwrap();

        bus.publish(SemanticEvent::new_human_feedback_received(
            RoleId::new("human"),
            "A test project",
        ))
        .unwrap();

        bus.publish(SemanticEvent::new_task_assigned(
            RoleId::new("human"),
            &task_id,
            RoleId::new("worker-001"),
            TaskContract {
                contract_id: Uuid::new_v4().to_string(),
                description: "Build the feature".to_string(),
            },
            Vec::new(),
        ))
        .unwrap();

        bus.publish(SemanticEvent::new_memory_proposed(
            RoleId::new("scholar-001"),
            "pattern",
            "discovered memory",
            "project",
            RoleId::new("librarian"),
            Vec::new(),
            0.85,
        ))
        .unwrap();

        bus.publish(SemanticEvent::new_artefact_produced_ref(
            RoleId::new("worker-001"),
            &artefact_id,
            "prd",
            "abc123",
            "file:///tmp/replay-test.json",
            RoleId::new("worker-001"),
            Vec::new(),
        ))
        .unwrap();
    }

    // Second runtime: replay, start server, verify /api/state
    {
        let config = OrganisationConfig {
            database_url: Some(database_url.clone()),
            event_store_path: None,
            memory_store_path: None,
            ..Default::default()
        };
        let intent_lead = IntentLead::new();
        let mut registry = RoleRegistry::new();
        registry.register(intent_lead.spec()).unwrap();

        let artefact_store = Arc::new(ArtefactStore::new());
        let runtime = OrganisationRuntime::new(config, registry).unwrap();
        let events = runtime.event_store().replay(0, None).unwrap();

        assert_eq!(events.len(), 5, "should replay 5 persisted events");
        assert!(
            events.iter().any(|e| e.variant_name() == "MemoryProposed"),
            "replayed events should include MemoryProposed",
        );
        assert!(
            events
                .iter()
                .any(|e| e.variant_name() == "ArtefactProduced"),
            "replayed events should include ArtefactProduced",
        );

        let state = AppState::with_events(runtime.bus().clone(), &events, artefact_store);
        let base_url = common::spawn_test_server(state).await;

        let resp = reqwest::get(&format!("{base_url}/api/state"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();

        // Messages — derived from HumanFeedbackRequested + HumanFeedbackReceived
        let messages = body["messages"].as_array().unwrap();
        assert!(!messages.is_empty(), "should have messages after replay");
        assert_eq!(messages[0]["speaker"], "Intent Lead");
        assert_eq!(messages[1]["speaker"], "You");

        // DAG steps — derived from TaskAssigned
        let dag_steps = body["dag_steps"].as_array().unwrap();
        assert!(!dag_steps.is_empty(), "should have DAG steps after replay");
        assert!(
            dag_steps.iter().any(|s| s["role"] == "worker-001"),
            "should have a worker step from replayed TaskAssigned",
        );

        // Memories — derived from MemoryProposed
        let memories = body["memories"].as_array().unwrap();
        assert!(!memories.is_empty(), "should have memories after replay");
        assert_eq!(memories[0]["status"], "Proposed");

        // Artefacts — derived from ArtefactProduced
        let artefacts = body["artefacts"].as_array().unwrap();
        assert!(!artefacts.is_empty(), "should have artefacts after replay");
        assert_eq!(artefacts[0]["artefact_type"], "prd");
    }

    common::drop_postgres_schema(&admin_pool, &schema).await;
}
