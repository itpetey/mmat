use crate::event::{EventType, SemanticEvent};
use crate::event_store::{EventStore, EventStoreError};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::broadcast::{self, Receiver, Sender};

#[derive(Clone, Debug)]
pub struct EventBus {
    sender: Sender<Arc<SemanticEvent>>,
    store: Option<Arc<EventStore>>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            store: None,
        }
    }

    pub fn with_store(mut self, store: Arc<EventStore>) -> Self {
        self.store = Some(store);
        self
    }

    pub fn publish(&self, event: SemanticEvent) -> std::result::Result<(), EventStoreError> {
        if let Some(store) = &self.store {
            store.insert(&event)?;
        }
        let arc = Arc::new(event);
        let _ = self.sender.send(arc);
        Ok(())
    }

    pub fn subscribe(&self, filter: &[EventType]) -> EventReceiver {
        let receiver = self.sender.subscribe();
        let filter_set: HashSet<EventType> = filter.iter().cloned().collect();
        EventReceiver {
            receiver,
            filter: filter_set,
        }
    }
}

#[derive(Debug)]
pub enum RecvError {
    Closed,
    Lagged(u64),
}

#[derive(Debug)]
pub struct EventReceiver {
    receiver: Receiver<Arc<SemanticEvent>>,
    filter: HashSet<EventType>,
}

impl EventReceiver {
    pub async fn recv(&mut self) -> std::result::Result<Arc<SemanticEvent>, RecvError> {
        loop {
            match self.receiver.recv().await {
                Ok(event) => {
                    if self.filter.is_empty() || self.filter.contains(&event.event_type()) {
                        return Ok(event);
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return Err(RecvError::Closed),
                Err(broadcast::error::RecvError::Lagged(n)) => return Err(RecvError::Lagged(n)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{RoleId, SemanticEvent};

    #[tokio::test]
    async fn subscriber_receives_event() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe(&[]);
        let event = SemanticEvent::new_tool_executed(RoleId::new("a"), "t", "{}", 0, "", "");
        bus.publish(event.clone()).unwrap();
        let received = rx.recv().await.unwrap();
        assert_eq!(received.variant_name(), "ToolExecuted");
    }

    #[tokio::test]
    async fn filtered_subscriber_ignores_unmatched() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe(&[EventType::TaskAssigned]);
        bus.publish(SemanticEvent::new_tool_executed(
            RoleId::new("a"),
            "t",
            "{}",
            0,
            "",
            "",
        ))
        .unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
        assert!(result.is_err());
    }

    #[test]
    fn publish_without_store_succeeds() {
        let bus = EventBus::new(16);
        let event = SemanticEvent::new_tool_executed(RoleId::new("a"), "t", "{}", 0, "", "");
        assert!(bus.publish(event).is_ok());
    }

    #[tokio::test]
    async fn concurrent_publish_and_subscribe() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = Arc::new(EventStore::open(tmp.path()).unwrap());
        let bus = Arc::new(EventBus::new(64).with_store(store.clone()));

        let mut rx = bus.subscribe(&[]);
        let bus_clone = bus.clone();

        let handle = tokio::spawn(async move {
            for i in 0..10 {
                let event = SemanticEvent::new_tool_executed(
                    RoleId::new("worker"),
                    &format!("tool_{}", i),
                    "{}",
                    0,
                    "",
                    "",
                );
                bus_clone.publish(event).unwrap();
            }
        });

        let mut received = 0;
        for _ in 0..10 {
            match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
                Ok(Ok(_)) => received += 1,
                _ => break,
            }
        }

        handle.await.unwrap();
        assert_eq!(received, 10);
        assert_eq!(store.latest_row().unwrap(), Some(10));
    }
}
