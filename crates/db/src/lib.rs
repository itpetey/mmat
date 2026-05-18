use diesel::{QueryResult, prelude::*, sql_types};
use diesel_async::{
    AsyncConnection, RunQueryDsl, SimpleAsyncConnection,
    pooled_connection::{AsyncDieselConnectionManager, PoolError},
};
use mmat_event_stream::event::{EventId, SemanticEvent};
use thiserror::Error;
use uuid::Uuid;

use crate::models::{Artefact, Event, Lane, Memory, NewArtefact, NewEvent, NewLane, NewMemory};

pub use diesel_async::{
    AsyncPgConnection,
    pooled_connection::bb8::{Pool, PooledConnection, RunError},
};

pub mod models;
pub mod schema;

type Result<T, E = DbError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database connection error: {0}")]
    DbConnection(#[from] ConnectionError),

    #[error("database error: {0}")]
    Diesel(#[from] diesel::result::Error),

    #[error("pool error: {0}")]
    Pool(String),

    #[error("event JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),

    #[error("transaction error: {0}")]
    Transaction(String),

    #[error("duplicate event id: {0}")]
    DuplicateEventId(EventId),
}

impl From<PoolError> for DbError {
    fn from(e: PoolError) -> Self {
        DbError::Pool(e.to_string())
    }
}

pub async fn connect(url: &str) -> Result<AsyncPgConnection> {
    Ok(AsyncPgConnection::establish(url).await?)
}

pub async fn new_pool(url: &str) -> Result<Pool<AsyncPgConnection>, PoolError> {
    let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(url);
    Pool::builder().build(config).await
}

/// Execute a raw SQL statement (e.g. schema setup).
pub async fn execute_sql(connection: &mut AsyncPgConnection, sql: &str) -> QueryResult<()> {
    SimpleAsyncConnection::batch_execute(connection, sql).await
}

pub async fn begin_transaction(connection: &mut AsyncPgConnection) -> QueryResult<()> {
    execute_sql(connection, "BEGIN").await
}

pub async fn commit_transaction(connection: &mut AsyncPgConnection) -> QueryResult<()> {
    execute_sql(connection, "COMMIT").await
}

pub async fn rollback_transaction(connection: &mut AsyncPgConnection) -> QueryResult<()> {
    execute_sql(connection, "ROLLBACK").await
}

// ── Memory CRUD ──

pub async fn insert_memory(
    connection: &mut AsyncPgConnection,
    memory: &NewMemory,
) -> QueryResult<Memory> {
    diesel::insert_into(schema::memories::table)
        .values(memory)
        .get_result::<Memory>(connection)
        .await
}

pub async fn get_memory_by_id(
    connection: &mut AsyncPgConnection,
    memory_id: Uuid,
) -> QueryResult<Option<Memory>> {
    use crate::schema::memories::dsl::{id, memories};

    memories
        .filter(id.eq(memory_id))
        .first::<Memory>(connection)
        .await
        .optional()
}

pub async fn query_memories_not_superseded_by_type(
    connection: &mut AsyncPgConnection,
    memory_type_filter: &str,
) -> QueryResult<Vec<Memory>> {
    use crate::schema::memories::dsl::{memories, superseded_by};

    memories
        .filter(crate::schema::memories::memory_type.eq(memory_type_filter))
        .filter(superseded_by.is_null())
        .load::<Memory>(connection)
        .await
}

pub async fn query_memories_not_superseded_by_scope(
    connection: &mut AsyncPgConnection,
    scope_filter: &str,
) -> QueryResult<Vec<Memory>> {
    use crate::schema::memories::dsl::{memories, superseded_by};

    memories
        .filter(crate::schema::memories::scope.eq(scope_filter))
        .filter(superseded_by.is_null())
        .load::<Memory>(connection)
        .await
}

pub async fn query_memories_not_superseded(
    connection: &mut AsyncPgConnection,
) -> QueryResult<Vec<Memory>> {
    use crate::schema::memories::dsl::{memories, superseded_by};

    memories
        .filter(superseded_by.is_null())
        .load::<Memory>(connection)
        .await
}

pub async fn query_all_memories(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Memory>> {
    use crate::schema::memories::dsl::memories;

    memories.load::<Memory>(connection).await
}

pub async fn update_memory_superseded_by(
    connection: &mut AsyncPgConnection,
    memory_id: Uuid,
    new_superseded_by: Option<Uuid>,
) -> QueryResult<usize> {
    use crate::schema::memories::dsl::{id, memories, superseded_by};

    diesel::update(memories.filter(id.eq(memory_id)))
        .set(superseded_by.eq(new_superseded_by))
        .execute(connection)
        .await
}

pub async fn update_memory_supersedes(
    connection: &mut AsyncPgConnection,
    memory_id: Uuid,
    supersedes_value: Option<Uuid>,
) -> QueryResult<usize> {
    use crate::schema::memories::dsl::{id, memories};

    diesel::update(memories.filter(id.eq(memory_id)))
        .set(crate::schema::memories::supersedes.eq(supersedes_value))
        .execute(connection)
        .await
}

pub async fn update_memory_last_accessed(
    connection: &mut AsyncPgConnection,
    memory_id: Uuid,
    new_last_accessed_at: &str,
) -> QueryResult<usize> {
    use crate::schema::memories::dsl::{id, memories};

    diesel::update(memories.filter(id.eq(memory_id)))
        .set(crate::schema::memories::last_accessed_at.eq(new_last_accessed_at))
        .execute(connection)
        .await
}

pub async fn update_memory_content(
    connection: &mut AsyncPgConnection,
    memory_id: Uuid,
    new_content: &str,
) -> QueryResult<usize> {
    use crate::schema::memories::dsl::{id, memories};

    diesel::update(memories.filter(id.eq(memory_id)))
        .set(crate::schema::memories::content.eq(new_content))
        .execute(connection)
        .await
}

pub async fn memory_exists(
    connection: &mut AsyncPgConnection,
    memory_id: Uuid,
) -> QueryResult<bool> {
    use crate::schema::memories::dsl::{id, memories};

    let row = memories
        .filter(id.eq(memory_id))
        .select(id)
        .first::<Uuid>(connection)
        .await
        .optional()?;
    Ok(row.is_some())
}

// ── Artefact CRUD ──

pub async fn insert_artefact(
    connection: &mut AsyncPgConnection,
    artefact: &NewArtefact,
) -> QueryResult<Artefact> {
    diesel::insert_into(schema::artefacts::table)
        .values(artefact)
        .get_result::<Artefact>(connection)
        .await
}

pub async fn get_artefact_payload(
    connection: &mut AsyncPgConnection,
    artefact_id: Uuid,
) -> QueryResult<Option<String>> {
    use crate::schema::artefacts::dsl::{artefacts, id, payload};

    let row = artefacts
        .filter(id.eq(artefact_id))
        .select(payload)
        .first::<serde_json::Value>(connection)
        .await
        .optional()?;

    Ok(row.map(|v| v.to_string()))
}

pub async fn insert_event(
    connection: &mut AsyncPgConnection,
    event: &NewEvent,
) -> QueryResult<Event> {
    diesel::insert_into(schema::events::table)
        .values(event)
        .get_result::<Event>(connection)
        .await
}

pub async fn append_event(
    connection: &mut AsyncPgConnection,
    event: &SemanticEvent,
) -> Result<Event> {
    if row_for_event_id(connection, event.event_id())
        .await?
        .is_some()
    {
        return Err(DbError::DuplicateEventId(event.event_id()));
    }

    let row = NewEvent {
        variant: event.variant_name().to_string(),
        payload: serde_json::to_value(event)?,
        timestamp_ns: event.timestamp_ns() as i64,
        source_agent: event.source_agent().to_string(),
    };

    diesel::insert_into(schema::events::table)
        .values(&row)
        .get_result::<Event>(connection)
        .await
        .map_err(DbError::from)
}

pub async fn replay_events(
    connection: &mut AsyncPgConnection,
    after_row: i64,
    before_row: Option<i64>,
) -> Result<Vec<SemanticEvent>> {
    use crate::schema::events::dsl::{events, rowid};

    let mut query = events.filter(rowid.gt(after_row)).into_boxed();
    if let Some(before) = before_row {
        query = query.filter(rowid.le(before));
    }

    let rows = query.order(rowid.asc()).load::<Event>(connection).await?;
    rows.into_iter()
        .map(|row| serde_json::from_value(row.payload).map_err(DbError::from))
        .collect()
}

pub async fn query_events_by_variant(
    connection: &mut AsyncPgConnection,
    event_variant: &str,
    after_row: Option<i64>,
    before_row: Option<i64>,
) -> Result<Vec<SemanticEvent>> {
    use crate::schema::events::dsl::{events, rowid, variant};

    let mut query = events.filter(variant.eq(event_variant)).into_boxed();
    if let Some(after) = after_row {
        query = query.filter(rowid.gt(after));
    }
    if let Some(before) = before_row {
        query = query.filter(rowid.le(before));
    }

    let rows = query.order(rowid.asc()).load::<Event>(connection).await?;
    rows.into_iter()
        .map(|row| serde_json::from_value(row.payload).map_err(DbError::from))
        .collect()
}

pub async fn latest_event_row(connection: &mut AsyncPgConnection) -> QueryResult<Option<i64>> {
    use crate::schema::events::dsl::{events, rowid};

    events
        .select(diesel::dsl::max(rowid))
        .first(connection)
        .await
}

pub async fn row_for_event_id(
    connection: &mut AsyncPgConnection,
    event_id: EventId,
) -> QueryResult<Option<i64>> {
    #[derive(QueryableByName)]
    struct EventRowId {
        #[diesel(sql_type = sql_types::BigInt)]
        rowid: i64,
    }

    diesel::sql_query("SELECT rowid FROM events WHERE payload->>'event_id' = $1 LIMIT 1")
        .bind::<sql_types::Text, _>(event_id.to_string())
        .get_result::<EventRowId>(connection)
        .await
        .optional()
        .map(|row| row.map(|row| row.rowid))
}

pub async fn get_event_by_id(
    connection: &mut AsyncPgConnection,
    event_id: EventId,
) -> Result<Option<SemanticEvent>> {
    #[derive(QueryableByName)]
    struct EventPayload {
        #[diesel(sql_type = sql_types::Jsonb)]
        payload: serde_json::Value,
    }

    let row =
        diesel::sql_query("SELECT payload FROM events WHERE payload->>'event_id' = $1 LIMIT 1")
            .bind::<sql_types::Text, _>(event_id.to_string())
            .get_result::<EventPayload>(connection)
            .await
            .optional()?;

    row.map(|event| serde_json::from_value(event.payload).map_err(DbError::from))
        .transpose()
}

pub async fn create_lane(connection: &mut AsyncPgConnection, lane: &NewLane) -> QueryResult<Lane> {
    diesel::insert_into(schema::lanes::table)
        .values(lane)
        .get_result::<Lane>(connection)
        .await
}

pub async fn create_lane_with_event(
    connection: &mut AsyncPgConnection,
    lane: NewLane,
    event: SemanticEvent,
) -> Result<Lane> {
    connection.batch_execute("BEGIN").await?;
    let lane_result = diesel::insert_into(schema::lanes::table)
        .values(&lane)
        .get_result::<Lane>(&mut *connection)
        .await;
    let lane = match lane_result {
        Ok(lane) => lane,
        Err(error) => {
            let _rollback = connection.batch_execute("ROLLBACK").await;
            return Err(DbError::from(error));
        }
    };

    if let Err(error) = append_event(connection, &event).await {
        let _rollback = connection.batch_execute("ROLLBACK").await;
        return Err(error);
    }

    connection.batch_execute("COMMIT").await?;
    Ok(lane)
}

pub async fn archive_lane(
    connection: &mut AsyncPgConnection,
    lane_id: &str,
    archived_at_value: String,
) -> QueryResult<Lane> {
    use crate::schema::lanes::dsl::{archived_at, id, lanes, status, updated_at};

    diesel::update(lanes.filter(id.eq(lane_id)))
        .set((
            status.eq("archived"),
            updated_at.eq(archived_at_value.clone()),
            archived_at.eq(Some(archived_at_value)),
        ))
        .get_result::<Lane>(connection)
        .await
}

pub async fn archive_lane_with_event(
    connection: &mut AsyncPgConnection,
    lane_id: &str,
    archived_at_value: String,
    event: SemanticEvent,
) -> Result<Lane> {
    connection.batch_execute("BEGIN").await?;
    let lane = match archive_lane(&mut *connection, lane_id, archived_at_value).await {
        Ok(lane) => lane,
        Err(error) => {
            let _rollback = connection.batch_execute("ROLLBACK").await;
            return Err(DbError::from(error));
        }
    };

    if let Err(error) = append_event(connection, &event).await {
        let _rollback = connection.batch_execute("ROLLBACK").await;
        return Err(error);
    }

    connection.batch_execute("COMMIT").await?;
    Ok(lane)
}

pub async fn load_lanes_by_status(
    connection: &mut AsyncPgConnection,
    project: &str,
    lane_status: &str,
) -> QueryResult<Vec<Lane>> {
    use crate::schema::lanes::dsl::{created_at, lanes, project_id, status};

    lanes
        .filter(project_id.eq(project))
        .filter(status.eq(lane_status))
        .order(created_at.asc())
        .load::<Lane>(connection)
        .await
}

pub async fn get_lane(
    connection: &mut AsyncPgConnection,
    lane_id: &str,
) -> QueryResult<Option<Lane>> {
    use crate::schema::lanes::dsl::{id, lanes};

    lanes
        .filter(id.eq(lane_id))
        .first::<Lane>(connection)
        .await
        .optional()
}

pub fn now_timestamp_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

pub fn new_lane_id() -> String {
    format!("lane-{}", Uuid::new_v4())
}
