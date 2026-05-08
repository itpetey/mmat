//! A publish-subscribe event bus with optional persistence via [`EventStore`].
//!
//! The [`EventBus`] is the central mechanism for distributing [`SemanticEvent`]s
//! to subscribers. Subscribers can filter by [`EventType`] and receive events
//! through an asynchronous [`EventReceiver`].

use std::{collections::HashSet, sync::Arc};

use tokio::sync::broadcast::{self, Receiver, Sender};

use crate::{
    event::{EventType, SemanticEvent},
    event_store::{EventStore, EventStoreError},
};

/// A broadcast bus for publishing semantic events to multiple subscribers.
///
/// Optionally backed by an [`EventStore`] for persistence. When a store is
/// configured, each published event is persisted before being sent to subscribers.
#[derive(Clone, Debug)]
pub struct EventBus {
    sender: Sender<Arc<SemanticEvent>>,
    store: Option<Arc<EventStore>>,
}

/// Errors that can occur when receiving events from an [`EventReceiver`].
#[derive(Debug)]
pub enum RecvError {
    /// The sender (and all its clones) have been dropped; no more events will be sent.
    Closed,
    /// The receiver has missed `n` events due to a full buffer.
    Lagged(u64),
}

/// A filtered subscription to the [`EventBus`].
///
/// Created via [`EventBus::subscribe`]. Events that match the optional filter
/// are yielded via [`recv`](EventReceiver::recv).
#[derive(Debug)]
pub struct EventReceiver {
    receiver: Receiver<Arc<SemanticEvent>>,
    filter: HashSet<EventType>,
}

impl EventBus {
    /// Creates a new [`EventBus`] with the given channel capacity and no persistent store.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            store: None,
        }
    }

    /// Attaches a persistent [`EventStore`] to this bus, consuming `self`.
    pub fn with_store(mut self, store: Arc<EventStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Publishes an event to all subscribers.
    ///
    /// If a store is configured the event is persisted first. The event is
    /// wrapped in an [`Arc`] before being broadcast.
    pub fn publish(&self, event: SemanticEvent) -> std::result::Result<(), EventStoreError> {
        if let Some(store) = &self.store {
            store.insert(&event)?;
        }
        let arc = Arc::new(event);
        let _ = self.sender.send(arc);
        Ok(())
    }

    /// Subscribes to the bus with an optional event-type filter.
    ///
    /// Returns an [`EventReceiver`] that yields events matching the given
    /// types. Pass an empty slice to receive all events.
    pub fn subscribe(&self, filter: &[EventType]) -> EventReceiver {
        let receiver = self.sender.subscribe();
        let filter_set: HashSet<EventType> = filter.iter().cloned().collect();
        EventReceiver {
            receiver,
            filter: filter_set,
        }
    }

    /// Returns a reference to the attached [`EventStore`], if any.
    pub fn store(&self) -> Option<Arc<EventStore>> {
        self.store.clone()
    }

    /// Replays persisted events after `after_row`, filtered by event type.
    ///
    /// Consumers should call this after receiving [`RecvError::Lagged`], process
    /// the returned events in order, persist the latest replay cursor, and then
    /// create a fresh subscription.
    pub fn replay_from(
        &self,
        after_row: i64,
        filter: &[EventType],
    ) -> std::result::Result<Vec<SemanticEvent>, EventStoreError> {
        let Some(store) = &self.store else {
            return Ok(Vec::new());
        };

        let filter_set: HashSet<EventType> = filter.iter().cloned().collect();
        let events = store.replay(after_row, None)?;
        Ok(events
            .into_iter()
            .filter(|event| filter_set.is_empty() || filter_set.contains(&event.event_type()))
            .collect())
    }

    /// Returns the persisted row for an event so consumers can maintain cursors.
    pub fn row_for_event_id(
        &self,
        event_id: crate::event::EventId,
    ) -> std::result::Result<Option<i64>, EventStoreError> {
        let Some(store) = &self.store else {
            return Ok(None);
        };
        store.row_for_event_id(event_id)
    }
}

impl EventReceiver {
    /// Awaits the next event that matches the configured filter.
    ///
    /// If the sender is dropped, returns [`RecvError::Closed`]. If the
    /// receiver has fallen behind, returns [`RecvError::Lagged`] with the
    /// number of skipped messages.
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
        let event = SemanticEvent::new_tool_executed(RoleId::new("a"), "t", "{}", 0, "", "", 0);
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
            0,
        ))
        .unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
        assert!(result.is_err());
    }

    #[test]
    fn publish_without_store_succeeds() {
        let bus = EventBus::new(16);
        let event = SemanticEvent::new_tool_executed(RoleId::new("a"), "t", "{}", 0, "", "", 0);
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
                    0,
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

    #[test]
    fn replay_from_filters_persisted_events() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = Arc::new(EventStore::open(tmp.path()).unwrap());
        let bus = EventBus::new(16).with_store(store);

        let tool = SemanticEvent::new_tool_executed(RoleId::new("a"), "t", "{}", 0, "", "", 0);
        let task = SemanticEvent::new_task_started(RoleId::new("pm"), "task-1", RoleId::new("w"));
        bus.publish(tool).unwrap();
        bus.publish(task).unwrap();

        let replayed = bus.replay_from(0, &[EventType::TaskStarted]).unwrap();
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].variant_name(), "TaskStarted");
    }
}
