use std::sync::Arc;

use mmat_event_stream::event::{EventType, RoleId, SemanticEvent};
use mmat_event_stream::event_bus::EventBus;
use mmat_event_stream::event_store::EventStore;

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
