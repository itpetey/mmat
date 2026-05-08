use std::path::Path;

use parking_lot::Mutex;
use rusqlite::Connection;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use thiserror::Error;

use crate::event::{EventId, SemanticEvent};

pub type Result<T> = std::result::Result<T, EventStoreError>;

#[derive(Error, Debug)]
pub enum EventStoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Postgres error: {0}")]
    Postgres(#[from] sqlx::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Not in a Tokio runtime: {0}")]
    Runtime(String),
}

enum EventStoreInner {
    Sqlite(SqliteEventStore),
    Postgres(PgEventStore),
}

pub struct EventStore {
    inner: EventStoreInner,
}

struct SqliteEventStore {
    conn: Mutex<Connection>,
}

pub struct PgEventStore {
    pool: PgPool,
}

impl SqliteEventStore {
    fn open(path: &Path) -> Result<Self> {
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
            CREATE INDEX IF NOT EXISTS idx_events_variant ON events(variant);",
        )?;
        Ok(())
    }
}

impl PgEventStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    pub fn connect_lazy(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_lazy(database_url)?;
        let store = Self { pool };
        Ok(store)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS events (
                event_id UUID PRIMARY KEY,
                rowid BIGSERIAL NOT NULL,
                variant TEXT NOT NULL,
                payload JSONB NOT NULL,
                timestamp_ns BIGINT NOT NULL,
                source_agent TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_events_rowid ON events(rowid);
            CREATE INDEX IF NOT EXISTS idx_events_variant ON events(variant);",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

impl EventStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let sqlite = SqliteEventStore::open(path.as_ref())?;
        Ok(Self {
            inner: EventStoreInner::Sqlite(sqlite),
        })
    }

    pub fn new(database_url: &str) -> Result<Self> {
        let pg = PgEventStore::connect_lazy(database_url)?;
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| EventStoreError::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| -> Result<()> { rt.block_on(pg.migrate()) })?;
        Ok(Self {
            inner: EventStoreInner::Postgres(pg),
        })
    }

    pub async fn new_async(database_url: &str) -> Result<Self> {
        let pg = PgEventStore::connect(database_url).await?;
        Ok(Self {
            inner: EventStoreInner::Postgres(pg),
        })
    }

    pub fn insert(&self, event: &SemanticEvent) -> Result<EventId> {
        match &self.inner {
            EventStoreInner::Sqlite(store) => store.insert(event),
            EventStoreInner::Postgres(store) => store.insert(event),
        }
    }

    pub fn replay(&self, after_row: i64, before_row: Option<i64>) -> Result<Vec<SemanticEvent>> {
        match &self.inner {
            EventStoreInner::Sqlite(store) => store.replay(after_row, before_row),
            EventStoreInner::Postgres(store) => store.replay(after_row, before_row),
        }
    }

    pub fn query_by_variant(
        &self,
        variant: &str,
        after_row: Option<i64>,
        before_row: Option<i64>,
    ) -> Result<Vec<SemanticEvent>> {
        match &self.inner {
            EventStoreInner::Sqlite(store) => {
                store.query_by_variant(variant, after_row, before_row)
            }
            EventStoreInner::Postgres(store) => {
                store.query_by_variant(variant, after_row, before_row)
            }
        }
    }

    pub fn latest_row(&self) -> Result<Option<i64>> {
        match &self.inner {
            EventStoreInner::Sqlite(store) => store.latest_row(),
            EventStoreInner::Postgres(store) => store.latest_row(),
        }
    }

    pub fn row_for_event_id(&self, event_id: EventId) -> Result<Option<i64>> {
        match &self.inner {
            EventStoreInner::Sqlite(store) => store.row_for_event_id(event_id),
            EventStoreInner::Postgres(store) => store.row_for_event_id(event_id),
        }
    }

    pub fn get_by_event_id(&self, event_id: EventId) -> Result<Option<SemanticEvent>> {
        match &self.inner {
            EventStoreInner::Sqlite(store) => store.get_by_event_id(event_id),
            EventStoreInner::Postgres(store) => store.get_by_event_id(event_id),
        }
    }

    pub fn pool(&self) -> Option<&PgPool> {
        match &self.inner {
            EventStoreInner::Postgres(store) => Some(&store.pool),
            _ => None,
        }
    }
}

impl std::fmt::Debug for EventStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStore").finish_non_exhaustive()
    }
}

impl SqliteEventStore {
    fn insert(&self, event: &SemanticEvent) -> Result<EventId> {
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

    fn replay(&self, after_row: i64, before_row: Option<i64>) -> Result<Vec<SemanticEvent>> {
        let sql = if before_row.is_some() {
            "SELECT payload FROM events WHERE rowid > ?1 AND rowid <= ?2 ORDER BY rowid ASC"
        } else {
            "SELECT payload FROM events WHERE rowid > ?1 ORDER BY rowid ASC"
        };

        let conn = self.conn.lock();
        let mut stmt = conn.prepare(sql)?;

        let rows: Vec<rusqlite::Result<SemanticEvent>> = if let Some(before) = before_row {
            stmt.query_map((after_row, before), sqlite_map_row)?
                .collect()
        } else {
            stmt.query_map([after_row], sqlite_map_row)?.collect()
        };

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    fn query_by_variant(
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

    fn latest_row(&self) -> Result<Option<i64>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT MAX(rowid) FROM events")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let value: Option<i64> = row.get(0)?;
            return Ok(value);
        }
        Ok(None)
    }

    fn row_for_event_id(&self, event_id: EventId) -> Result<Option<i64>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT rowid FROM events WHERE event_id = ?1")?;
        let mut rows = stmt.query([&event_id.to_string()])?;
        if let Some(row) = rows.next()? {
            let value: i64 = row.get(0)?;
            return Ok(Some(value));
        }
        Ok(None)
    }

    fn get_by_event_id(&self, event_id: EventId) -> Result<Option<SemanticEvent>> {
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

fn sqlite_map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SemanticEvent> {
    let payload: String = row.get(0)?;
    serde_json::from_str(&payload).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

impl PgEventStore {
    fn insert(&self, event: &SemanticEvent) -> Result<EventId> {
        let payload = serde_json::to_string(event)?;
        let variant = event.variant_name();
        let event_id = event.event_id();
        let timestamp_ns = event_timestamp_ns(event);
        let source_agent = event_source_agent(event);

        let pool = self.pool.clone();
        let event_id_str = event_id.to_string();
        let variant_owned = variant.to_string();
        let source_agent_owned = source_agent.to_string();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| EventStoreError::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                sqlx::query(
                    "INSERT INTO events (event_id, variant, payload, timestamp_ns, source_agent)
                     VALUES ($1::uuid, $2, $3::jsonb, $4, $5)",
                )
                .bind(&event_id_str)
                .bind(&variant_owned)
                .bind(&payload)
                .bind(timestamp_ns as i64)
                .bind(&source_agent_owned)
                .execute(&pool)
                .await
            })
        })?;

        Ok(event_id)
    }

    fn replay(&self, after_row: i64, before_row: Option<i64>) -> Result<Vec<SemanticEvent>> {
        let pool = self.pool.clone();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| EventStoreError::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let rows = if let Some(before) = before_row {
                    sqlx::query_as::<_, (String,)>(
                        "SELECT payload::text FROM events WHERE rowid > $1 AND rowid <= $2 ORDER BY rowid ASC",
                    )
                    .bind(after_row)
                    .bind(before)
                    .fetch_all(&pool)
                    .await?
                } else {
                    sqlx::query_as::<_, (String,)>(
                        "SELECT payload::text FROM events WHERE rowid > $1 ORDER BY rowid ASC",
                    )
                    .bind(after_row)
                    .fetch_all(&pool)
                    .await?
                };

                let events: Vec<SemanticEvent> = rows
                    .into_iter()
                    .map(|(json_str,)| serde_json::from_str(&json_str).map_err(EventStoreError::Json))
                    .collect::<Result<Vec<_>>>()?;
                Ok(events)
            })
        })
    }

    fn query_by_variant(
        &self,
        variant: &str,
        after_row: Option<i64>,
        before_row: Option<i64>,
    ) -> Result<Vec<SemanticEvent>> {
        let pool = self.pool.clone();
        let variant_owned = variant.to_string();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| EventStoreError::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let mut sql = String::from("SELECT payload::text FROM events WHERE variant = $1");

                if after_row.is_some() {
                    sql.push_str(" AND rowid > $2");
                }
                if before_row.is_some() {
                    sql.push_str(if after_row.is_some() {
                        " AND rowid <= $3"
                    } else {
                        " AND rowid <= $2"
                    });
                }
                sql.push_str(" ORDER BY rowid ASC");

                let mut query = sqlx::query_as::<_, (String,)>(&sql).bind(&variant_owned);
                if let Some(after) = after_row {
                    query = query.bind(after);
                }
                if let Some(before) = before_row {
                    query = query.bind(before);
                }

                let rows = query.fetch_all(&pool).await?;
                let events: Vec<SemanticEvent> = rows
                    .into_iter()
                    .map(|(json_str,)| {
                        serde_json::from_str(&json_str).map_err(EventStoreError::Json)
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(events)
            })
        })
    }

    fn latest_row(&self) -> Result<Option<i64>> {
        let pool = self.pool.clone();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| EventStoreError::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let row: Option<(Option<i64>,)> = sqlx::query_as("SELECT MAX(rowid) FROM events")
                    .fetch_optional(&pool)
                    .await?;
                Ok(row.and_then(|r| r.0))
            })
        })
    }

    fn row_for_event_id(&self, event_id: EventId) -> Result<Option<i64>> {
        let pool = self.pool.clone();
        let event_id_str = event_id.to_string();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| EventStoreError::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let row: Option<(i64,)> =
                    sqlx::query_as("SELECT rowid FROM events WHERE event_id = $1::uuid")
                        .bind(&event_id_str)
                        .fetch_optional(&pool)
                        .await?;
                Ok(row.map(|r| r.0))
            })
        })
    }

    fn get_by_event_id(&self, event_id: EventId) -> Result<Option<SemanticEvent>> {
        let pool = self.pool.clone();
        let event_id_str = event_id.to_string();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| EventStoreError::Runtime(e.to_string()))?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let row: Option<(String,)> =
                    sqlx::query_as("SELECT payload::text FROM events WHERE event_id = $1::uuid")
                        .bind(&event_id_str)
                        .fetch_optional(&pool)
                        .await?;
                match row {
                    Some((json_str,)) => {
                        let event: SemanticEvent =
                            serde_json::from_str(&json_str).map_err(EventStoreError::Json)?;
                        Ok(Some(event))
                    }
                    None => Ok(None),
                }
            })
        })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
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

    #[tokio::test]
    async fn postgres_insert_and_replay() {
        let Some((database_url, admin_pool, schema)) = postgres_test_database("event_store").await
        else {
            return;
        };
        let store = PgEventStore::connect(&database_url).await.unwrap();

        let e1 = SemanticEvent::new_tool_executed(RoleId::new("a"), "t1", "{}", 0, "", "", 0);
        let e2 = SemanticEvent::new_claim_made(RoleId::new("a"), "claim", vec![], 0.9);
        let id1 = store.insert(&e1).unwrap();
        let id2 = store.insert(&e2).unwrap();
        assert_eq!(id1, e1.event_id());
        assert_eq!(id2, e2.event_id());

        let replayed = store.replay(0, None).unwrap();
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].variant_name(), "ToolExecuted");
        assert_eq!(replayed[1].variant_name(), "ClaimMade");

        // BIGSERIAL should have auto-assigned sequential rowids
        let row1 = store.row_for_event_id(id1).unwrap();
        let row2 = store.row_for_event_id(id2).unwrap();
        assert_eq!(row2, row1.map(|r| r + 1));

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
}
