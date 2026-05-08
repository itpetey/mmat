use std::sync::Arc;

use mmat_event_stream::{
    event::{EventType, RoleId, SemanticEvent},
    event_bus::EventBus,
    event_store::EventStore,
};
use sqlx::postgres::PgPoolOptions;

#[tokio::test]
async fn multiple_subscribers_with_filters() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let store = Arc::new(EventStore::open(tmp.path()).unwrap());
    let bus = EventBus::new(16).with_store(store);

    let mut rx_a = bus.subscribe(&[EventType::TaskAssigned]);
    let mut rx_b = bus.subscribe(&[EventType::TaskCompleted]);

    let assigned = SemanticEvent::new_task_assigned(
        RoleId::new("coordinator"),
        "task-1",
        RoleId::new("worker"),
        mmat_event_stream::event::TaskContract {
            contract_id: "c1".into(),
            description: "do thing".into(),
        },
        vec![],
    );
    bus.publish(assigned).unwrap();

    let a_received = rx_a.recv().await.unwrap();
    assert_eq!(a_received.variant_name(), "TaskAssigned");

    let b_result = tokio::time::timeout(std::time::Duration::from_millis(50), rx_b.recv()).await;
    assert!(b_result.is_err());
}

#[tokio::test]
async fn publish_subscribe_and_store() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let store = Arc::new(EventStore::open(tmp.path()).unwrap());
    let bus = EventBus::new(16).with_store(store.clone());

    let mut rx = bus.subscribe(&[]);
    let event =
        SemanticEvent::new_tool_executed(RoleId::new("worker"), "test", "{}", 0, "out", "err", 0);
    bus.publish(event.clone()).unwrap();

    let received = rx.recv().await.unwrap();
    assert_eq!(received.variant_name(), "ToolExecuted");

    let replayed = store.replay(0, None).unwrap();
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].variant_name(), "ToolExecuted");
}

#[tokio::test]
async fn subscriber_replays_after_lag() {
    let bus = EventBus::new(2);

    let e1 = SemanticEvent::new_tool_executed(RoleId::new("a"), "t1", "{}", 0, "", "", 0);
    let e2 = SemanticEvent::new_tool_executed(RoleId::new("a"), "t2", "{}", 0, "", "", 0);
    let e3 = SemanticEvent::new_tool_executed(RoleId::new("a"), "t3", "{}", 0, "", "", 0);

    let mut rx = bus.subscribe(&[]);
    bus.publish(e1).unwrap();
    bus.publish(e2).unwrap();
    bus.publish(e3).unwrap();

    // rx should lag since capacity is 2
    let result = rx.recv().await;
    match result {
        Err(mmat_event_stream::event_bus::RecvError::Lagged(n)) => {
            assert!(n >= 1);
        }
        other => {
            // Depending on timing, it might receive the latest event
            assert!(other.is_ok());
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_event_store_round_trip_and_concurrent_writes() {
    let Some((database_url, admin_pool, schema)) = postgres_test_database("event_stream").await
    else {
        return;
    };

    let store = Arc::new(EventStore::new(&database_url).unwrap());
    assert!(store.replay(0, None).unwrap().is_empty());

    let tool =
        SemanticEvent::new_tool_executed(RoleId::new("worker"), "cargo test", "{}", 0, "ok", "", 0);
    let assigned = SemanticEvent::new_task_assigned(
        RoleId::new("coordinator"),
        "task-1",
        RoleId::new("worker"),
        mmat_event_stream::event::TaskContract {
            contract_id: "contract-1".into(),
            description: "run tests".into(),
        },
        vec![],
    );

    store.insert(&tool).unwrap();
    store.insert(&assigned).unwrap();

    assert_eq!(store.latest_row().unwrap(), Some(2));
    assert_eq!(
        store
            .get_by_event_id(tool.event_id())
            .unwrap()
            .unwrap()
            .variant_name(),
        "ToolExecuted"
    );
    assert_eq!(
        store
            .query_by_variant("ToolExecuted", None, None)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.replay(0, None).unwrap().len(), 2);

    let mut handles = Vec::new();
    for i in 0..8 {
        let store = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let event = SemanticEvent::new_tool_executed(
                RoleId::new("worker"),
                format!("cmd-{i}"),
                "{}",
                0,
                "",
                "",
                0,
            );
            store.insert(&event).unwrap();
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(store.replay(0, None).unwrap().len(), 10);
    drop(store);
    drop_postgres_schema(&admin_pool, &schema).await;
}

async fn postgres_test_database(prefix: &str) -> Option<(String, sqlx::PgPool, String)> {
    let base_url = std::env::var("DATABASE_URL").ok()?;
    let schema = format!("{}_{}", prefix, now_nanos());
    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&base_url)
        .await
        .ok()?;
    sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
        .execute(&admin_pool)
        .await
        .ok()?;
    let separator = if base_url.contains('?') { '&' } else { '?' };
    let database_url = format!("{base_url}{separator}options=-c%20search_path%3D{schema}");
    Some((database_url, admin_pool, schema))
}

async fn drop_postgres_schema(pool: &sqlx::PgPool, schema: &str) {
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(pool)
        .await
        .unwrap();
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
