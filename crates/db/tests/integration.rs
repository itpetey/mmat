use diesel_async::SimpleAsyncConnection;
use mmat_event_stream::event::{EventContext, RoleId, SemanticEvent, TaskContract};

async fn test_database(prefix: &str) -> Option<(mmat_db::AsyncPgConnection, String)> {
    let base_url = std::env::var("MMAT_DB_URL").ok()?;
    let schema = format!("{}_{}", prefix, now_nanos());
    let mut admin = mmat_db::connect(&base_url).await.ok()?;
    admin
        .batch_execute(&format!("CREATE SCHEMA \"{schema}\""))
        .await
        .ok()?;
    let separator = if base_url.contains('?') { '&' } else { '?' };
    let database_url = format!("{base_url}{separator}options=-c%20search_path%3D{schema}");
    let connection = mmat_db::connect(&database_url).await.ok()?;
    Some((connection, schema))
}

async fn drop_schema(schema: &str) {
    let Ok(base_url) = std::env::var("MMAT_DB_URL") else {
        return;
    };
    let Ok(mut connection) = mmat_db::connect(&base_url).await else {
        return;
    };
    let _ = connection
        .batch_execute(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .await;
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[tokio::test]
async fn event_crud_replay_and_variant_queries() {
    let Some((mut connection, schema)) = test_database("db_events").await else {
        return;
    };
    mmat_db::ensure_schema(&mut connection).await.unwrap();

    let task = SemanticEvent::new_task_assigned(
        RoleId::new("pm"),
        "task-1",
        RoleId::new("worker"),
        TaskContract {
            contract_id: "contract-1".to_string(),
            description: "Build the thing".to_string(),
        },
        Vec::new(),
    );
    let tool =
        SemanticEvent::new_tool_executed(RoleId::new("worker"), "cargo", "{}", 0, "ok", "", 1);
    let claim =
        SemanticEvent::new_claim_made(RoleId::new("worker"), "cargo test passed", Vec::new(), 0.8);

    let task_row = mmat_db::append_event(&mut connection, &task).await.unwrap();
    let tool_row = mmat_db::append_event(&mut connection, &tool).await.unwrap();
    let claim_row = mmat_db::append_event(&mut connection, &claim)
        .await
        .unwrap();

    assert_eq!(task_row.rowid, 1);
    assert_eq!(tool_row.rowid, 2);
    assert_eq!(claim_row.rowid, 3);
    assert_eq!(
        mmat_db::latest_event_row(&mut connection).await.unwrap(),
        Some(3)
    );
    assert_eq!(
        mmat_db::row_for_event_id(&mut connection, tool.event_id())
            .await
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        mmat_db::get_event_by_id(&mut connection, claim.event_id())
            .await
            .unwrap()
            .unwrap()
            .event_id(),
        claim.event_id()
    );

    let replayed = mmat_db::replay_events(&mut connection, 1, Some(3))
        .await
        .unwrap();
    assert_eq!(
        replayed
            .iter()
            .map(SemanticEvent::event_id)
            .collect::<Vec<_>>(),
        vec![tool.event_id(), claim.event_id()]
    );

    let tool_events = mmat_db::query_events_by_variant(&mut connection, "ToolExecuted", None, None)
        .await
        .unwrap();
    assert_eq!(tool_events.len(), 1);
    assert_eq!(tool_events[0].event_id(), tool.event_id());

    assert!(mmat_db::append_event(&mut connection, &task).await.is_err());

    drop_schema(&schema).await;
}

#[tokio::test]
async fn lane_crud_archive_and_event_persistence() {
    let Some((mut connection, schema)) = test_database("db_lanes").await else {
        return;
    };
    mmat_db::ensure_schema(&mut connection).await.unwrap();

    let source_event =
        SemanticEvent::new_human_feedback_received(RoleId::new("human"), "split this out")
            .with_context(EventContext::new("org", "workspace", "project-1", "run-1"));
    let source_event_id = source_event.event_id();
    let lane_event = SemanticEvent::new_lane_created(
        RoleId::new("tool:create_lane"),
        "lane-1",
        "Branch",
        "conversation",
        "",
        "Discuss a branch",
        Some("parent-lane".to_string()),
        Vec::new(),
        Some(source_event_id),
        Some("message-1".to_string()),
    )
    .with_context(EventContext::new("org", "workspace", "project-1", "run-1"));
    let now = mmat_db::now_timestamp_string();
    let lane = mmat_db::models::NewLane {
        id: "lane-1".to_string(),
        project_id: "project-1".to_string(),
        title: "Branch".to_string(),
        summary: "Discuss a branch".to_string(),
        status: "active".to_string(),
        creator: "tool:create_lane".to_string(),
        parent_lane_id: Some("parent-lane".to_string()),
        origin_event_id: Some(source_event_id.0),
        origin_message_id: Some("message-1".to_string()),
        created_at: now.clone(),
        updated_at: now,
        archived_at: None,
    };

    let created = mmat_db::create_lane_with_event(&mut connection, lane, lane_event)
        .await
        .unwrap();
    assert_eq!(created.id, "lane-1");
    assert_eq!(created.parent_lane_id.as_deref(), Some("parent-lane"));
    assert_eq!(created.origin_event_id, Some(source_event_id.0));

    let active = mmat_db::load_lanes_by_status(&mut connection, "project-1", "active")
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert!(
        mmat_db::get_lane(&mut connection, "lane-1")
            .await
            .unwrap()
            .is_some()
    );

    let archive_event = SemanticEvent::new_lane_archived(RoleId::new("human"), "lane-1")
        .with_context(EventContext::new("org", "workspace", "project-1", "run-1"));
    let archived = mmat_db::archive_lane_with_event(
        &mut connection,
        "lane-1",
        mmat_db::now_timestamp_string(),
        archive_event,
    )
    .await
    .unwrap();
    assert_eq!(archived.status, "archived");

    assert!(
        mmat_db::load_lanes_by_status(&mut connection, "project-1", "active")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        mmat_db::load_lanes_by_status(&mut connection, "project-1", "archived")
            .await
            .unwrap()
            .len(),
        1
    );
    let events = mmat_db::replay_events(&mut connection, 0, None)
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .map(SemanticEvent::variant_name)
            .collect::<Vec<_>>(),
        vec!["LaneCreated", "LaneArchived"]
    );

    drop_schema(&schema).await;
}
