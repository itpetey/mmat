use std::{fmt, sync::Mutex};

use crate::event::{EventId, SemanticEvent};

pub type Result<T> = std::result::Result<T, EventStoreError>;

#[derive(Debug)]
pub enum EventStoreError {
    Runtime(String),
}

impl fmt::Display for EventStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(message) => write!(f, "event store error: {message}"),
        }
    }
}

impl std::error::Error for EventStoreError {}

pub struct EventStore {
    events: Mutex<Vec<SemanticEvent>>,
}

impl EventStore {
    pub fn empty() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    pub fn insert(&self, event: &SemanticEvent) -> Result<EventId> {
        let mut events = self
            .events
            .lock()
            .map_err(|err| EventStoreError::Runtime(err.to_string()))?;
        events.push(event.clone());
        Ok(event.event_id())
    }

    pub fn replay(&self, after_row: i64, before_row: Option<i64>) -> Result<Vec<SemanticEvent>> {
        let events = self
            .events
            .lock()
            .map_err(|err| EventStoreError::Runtime(err.to_string()))?;
        Ok(events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                let row = i64::try_from(index + 1).ok()?;
                if row > after_row && before_row.is_none_or(|before| row <= before) {
                    Some(event.clone())
                } else {
                    None
                }
            })
            .collect())
    }

    pub fn query_by_variant(
        &self,
        variant: &str,
        after_row: Option<i64>,
        before_row: Option<i64>,
    ) -> Result<Vec<SemanticEvent>> {
        let after = after_row.unwrap_or(0);
        Ok(self
            .replay(after, before_row)?
            .into_iter()
            .filter(|event| event.variant_name() == variant)
            .collect())
    }

    pub fn latest_row(&self) -> Result<Option<i64>> {
        let events = self
            .events
            .lock()
            .map_err(|err| EventStoreError::Runtime(err.to_string()))?;
        if events.is_empty() {
            Ok(None)
        } else {
            Ok(i64::try_from(events.len()).ok())
        }
    }

    pub fn row_for_event_id(&self, event_id: EventId) -> Result<Option<i64>> {
        let events = self
            .events
            .lock()
            .map_err(|err| EventStoreError::Runtime(err.to_string()))?;
        Ok(events.iter().enumerate().find_map(|(index, event)| {
            if event.event_id() == event_id {
                i64::try_from(index + 1).ok()
            } else {
                None
            }
        }))
    }

    pub fn get_by_event_id(&self, event_id: EventId) -> Result<Option<SemanticEvent>> {
        let events = self
            .events
            .lock()
            .map_err(|err| EventStoreError::Runtime(err.to_string()))?;
        Ok(events
            .iter()
            .find(|event| event.event_id() == event_id)
            .cloned())
    }
}

impl fmt::Debug for EventStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventStore").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{RoleId, SemanticEvent};

    #[test]
    fn store_create_and_insert() {
        let store = EventStore::empty();
        let event = SemanticEvent::new_tool_executed(RoleId::new("a"), "t", "{}", 0, "", "", 0);
        let id = store.insert(&event).unwrap();
        assert_eq!(id, event.event_id());
    }

    #[test]
    fn replay_events() {
        let store = EventStore::empty();
        let e1 = SemanticEvent::new_tool_executed(RoleId::new("a"), "t1", "{}", 0, "", "", 0);
        let e2 = SemanticEvent::new_claim_made(RoleId::new("a"), "claim", vec![], 0.9);
        store.insert(&e1).unwrap();
        store.insert(&e2).unwrap();

        let replayed = store.replay(0, None).unwrap();
        assert_eq!(replayed.len(), 2);
    }

    #[test]
    fn query_by_variant() {
        let store = EventStore::empty();
        let e1 = SemanticEvent::new_tool_executed(RoleId::new("a"), "t1", "{}", 0, "", "", 0);
        let e2 = SemanticEvent::new_claim_made(RoleId::new("a"), "claim", vec![], 0.9);
        store.insert(&e1).unwrap();
        store.insert(&e2).unwrap();

        let found = store.query_by_variant("ToolExecuted", None, None).unwrap();
        assert_eq!(found.len(), 1);
    }
}
