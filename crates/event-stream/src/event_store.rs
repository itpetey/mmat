//! SQLite-backed persistent storage for semantic events.
//!
//! The [`EventStore`] provides insert and query capabilities over a local
//! SQLite database. It is used by the [`EventBus`](crate::event_bus::EventBus)
//! for optional persistence and replay.

use std::path::Path;

use parking_lot::Mutex;
use rusqlite::Connection;
use thiserror::Error;

use crate::event::{EventId, SemanticEvent};

/// The result type for event store operations.
pub type Result<T> = std::result::Result<T, EventStoreError>;

/// Errors that can occur during event store operations.
#[derive(Error, Debug)]
pub enum EventStoreError {
    /// An error from the underlying SQLite database.
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// An error serialising or deserialising event JSON.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A persistent, thread-safe event store backed by SQLite.
pub struct EventStore {
    conn: Mutex<Connection>,
}

impl EventStore {
    /// Opens (or creates) a SQLite database at `path` and runs migrations.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                event_id TEXT PRIMARY KEY,
                rowid INTEGER NOT NULL,
                variant TEXT NOT NULL,
                payload TEXT NOT NULL,
                timestamp_ns INTEGER NOT NULL,
                source_agent TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_events_rowid ON events(rowid);
            CREATE INDEX IF NOT EXISTS idx_events_variant ON events(variant);
            ",
        )?;
        Ok(())
    }

    /// Inserts a semantic event into the store and returns its [`EventId`].
    pub fn insert(&self, event: &SemanticEvent) -> Result<EventId> {
        let payload = serde_json::to_string(event)?;
        let variant = event.variant_name();
        let event_id = event.event_id();
        let timestamp_ns = event_timestamp_ns(event);
        let source_agent = event_source_agent(event);

        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO events (event_id, rowid, variant, payload, timestamp_ns, source_agent)
             VALUES (?1, (SELECT COALESCE(MAX(rowid), 0) + 1 FROM events), ?2, ?3, ?4, ?5)",
            (
                &event_id.to_string(),
                variant,
                &payload,
                timestamp_ns as i64,
                &source_agent,
            ),
        )?;

        Ok(event_id)
    }

    /// Replays events in rowid order from `after_row` (exclusive) up to
    /// `before_row` (inclusive, if provided).
    pub fn replay(&self, after_row: i64, before_row: Option<i64>) -> Result<Vec<SemanticEvent>> {
        let sql = if before_row.is_some() {
            "SELECT payload FROM events WHERE rowid > ?1 AND rowid <= ?2 ORDER BY rowid ASC"
        } else {
            "SELECT payload FROM events WHERE rowid > ?1 ORDER BY rowid ASC"
        };

        let conn = self.conn.lock();
        let mut stmt = conn.prepare(sql)?;

        let rows: Vec<rusqlite::Result<SemanticEvent>> = if let Some(before) = before_row {
            stmt.query_map((after_row, before), Self::map_row)?
                .collect()
        } else {
            stmt.query_map([after_row], Self::map_row)?.collect()
        };

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SemanticEvent> {
        let payload: String = row.get(0)?;
        serde_json::from_str(&payload).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
    }

    /// Queries events by variant name, optionally bounded by rowid range.
    pub fn query_by_variant(
        &self,
        variant: &str,
        after_row: Option<i64>,
        before_row: Option<i64>,
    ) -> Result<Vec<SemanticEvent>> {
        let mut sql = String::from("SELECT payload FROM events WHERE variant = ?1");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(variant.to_string())];

        if let Some(after) = after_row {
            sql.push_str(" AND rowid > ?");
            params.push(Box::new(after));
        }
        if let Some(before) = before_row {
            sql.push_str(" AND rowid <= ?");
            params.push(Box::new(before));
        }
        sql.push_str(" ORDER BY rowid ASC");

        let conn = self.conn.lock();
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(&param_refs[..], |row| {
            let payload: String = row.get(0)?;
            let event: SemanticEvent = serde_json::from_str(&payload).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(event)
        })?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Returns the maximum `rowid` in the table, or `None` if the table is empty.
    pub fn latest_row(&self) -> Result<Option<i64>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT MAX(rowid) FROM events")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let value: Option<i64> = row.get(0)?;
            return Ok(value);
        }
        Ok(None)
    }

    /// Returns the `rowid` for a given [`EventId`], or `None` if not found.
    pub fn row_for_event_id(&self, event_id: EventId) -> Result<Option<i64>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT rowid FROM events WHERE event_id = ?1")?;
        let mut rows = stmt.query([&event_id.to_string()])?;
        if let Some(row) = rows.next()? {
            let value: i64 = row.get(0)?;
            return Ok(Some(value));
        }
        Ok(None)
    }

    /// Retrieves a full [`SemanticEvent`] by its [`EventId`], if it exists.
    pub fn get_by_event_id(&self, event_id: EventId) -> Result<Option<SemanticEvent>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT payload FROM events WHERE event_id = ?1")?;
        let mut rows = stmt.query([&event_id.to_string()])?;
        if let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            let event: SemanticEvent = serde_json::from_str(&payload).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            return Ok(Some(event));
        }
        Ok(None)
    }
}

impl std::fmt::Debug for EventStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStore").finish_non_exhaustive()
    }
}

fn event_source_agent(event: &SemanticEvent) -> String {
    match event {
        SemanticEvent::ToolExecuted { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::ClaimMade { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::DecisionRecorded { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::MemoryProposed { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::MemoryAccepted { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::MemoryRejected { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::MemorySuperseded { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::EvidenceChainBroken { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::ProcessSkipped { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::PolicyViolationDetected { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::TaskAssigned { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::TaskStarted { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::TaskCompleted { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::TaskFailed { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::ReviewRequested { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::ReviewCompleted { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::EscalationRequested { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::HumanFeedbackRequested { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::HumanFeedbackReceived { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::ArtefactProduced { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::BudgetWarning { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::EscalationAccepted { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::RoleStateChanged { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::OrganisationStarted { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::OrganisationStopped { source_agent, .. } => source_agent.to_string(),
        SemanticEvent::Heartbeat { source_agent, .. } => source_agent.to_string(),
    }
}

fn event_timestamp_ns(event: &SemanticEvent) -> u64 {
    match event {
        SemanticEvent::ToolExecuted { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::ClaimMade { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::DecisionRecorded { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::MemoryProposed { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::MemoryAccepted { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::MemoryRejected { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::MemorySuperseded { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::EvidenceChainBroken { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::ProcessSkipped { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::PolicyViolationDetected { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::TaskAssigned { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::TaskStarted { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::TaskCompleted { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::TaskFailed { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::ReviewRequested { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::ReviewCompleted { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::EscalationRequested { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::HumanFeedbackRequested { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::HumanFeedbackReceived { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::ArtefactProduced { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::BudgetWarning { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::EscalationAccepted { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::RoleStateChanged { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::OrganisationStarted { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::OrganisationStopped { timestamp_ns, .. } => *timestamp_ns,
        SemanticEvent::Heartbeat { timestamp_ns, .. } => *timestamp_ns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{RoleId, SemanticEvent};

    #[test]
    fn store_create_and_insert() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = EventStore::open(tmp.path()).unwrap();
        let event = SemanticEvent::new_tool_executed(RoleId::new("a"), "t", "{}", 0, "", "", 0);
        let id = store.insert(&event).unwrap();
        assert_eq!(id, event.event_id());
    }

    #[test]
    fn replay_events() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = EventStore::open(tmp.path()).unwrap();
        let e1 = SemanticEvent::new_tool_executed(RoleId::new("a"), "t1", "{}", 0, "", "", 0);
        let e2 = SemanticEvent::new_claim_made(RoleId::new("a"), "claim", vec![], 0.9);
        store.insert(&e1).unwrap();
        store.insert(&e2).unwrap();

        let replayed = store.replay(0, None).unwrap();
        assert_eq!(replayed.len(), 2);
    }

    #[test]
    fn query_by_variant() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = EventStore::open(tmp.path()).unwrap();
        let e1 = SemanticEvent::new_tool_executed(RoleId::new("a"), "t1", "{}", 0, "", "", 0);
        let e2 = SemanticEvent::new_claim_made(RoleId::new("a"), "claim", vec![], 0.9);
        store.insert(&e1).unwrap();
        store.insert(&e2).unwrap();

        let found = store.query_by_variant("ToolExecuted", None, None).unwrap();
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn latest_row() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = EventStore::open(tmp.path()).unwrap();
        assert_eq!(store.latest_row().unwrap(), None);
        let e1 = SemanticEvent::new_tool_executed(RoleId::new("a"), "t1", "{}", 0, "", "", 0);
        store.insert(&e1).unwrap();
        assert_eq!(store.latest_row().unwrap(), Some(1));
    }

    #[test]
    fn row_for_event_id() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = EventStore::open(tmp.path()).unwrap();
        let e1 = SemanticEvent::new_tool_executed(RoleId::new("a"), "t1", "{}", 0, "", "", 0);
        let id = store.insert(&e1).unwrap();
        assert_eq!(store.row_for_event_id(id).unwrap(), Some(1));
        assert_eq!(store.row_for_event_id(EventId::new()).unwrap(), None);
    }
}
