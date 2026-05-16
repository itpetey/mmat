use diesel::{ConnectionResult, QueryResult, prelude::*};
use diesel_async::{AsyncConnection, RunQueryDsl, SimpleAsyncConnection};
use mmat_event_stream::event::{EventId, SemanticEvent};
use thiserror::Error;
use uuid::Uuid;

use crate::models::{Event, Lane, NewEvent, NewLane, NewProject, Project};

pub use diesel_async::AsyncPgConnection;

pub mod models;
pub mod schema;

pub type Result<T> = std::result::Result<T, DbError>;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Diesel(#[from] diesel::result::Error),

    #[error("event JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),
}

pub async fn connect(url: &str) -> ConnectionResult<AsyncPgConnection> {
    AsyncPgConnection::establish(url).await
}

pub async fn ensure_schema(connection: &mut AsyncPgConnection) -> QueryResult<()> {
    diesel::sql_query("CREATE EXTENSION IF NOT EXISTS pgcrypto")
        .execute(connection)
        .await?;
    diesel::sql_query(
        "CREATE TABLE IF NOT EXISTS projects (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            label VARCHAR NOT NULL,
            path VARCHAR NOT NULL
        )",
    )
    .execute(connection)
    .await?;
    diesel::sql_query(
        "CREATE TABLE IF NOT EXISTS events (
            id UUID PRIMARY KEY,
            rowid BIGSERIAL NOT NULL UNIQUE,
            variant TEXT NOT NULL,
            payload JSONB NOT NULL,
            timestamp_ns BIGINT NOT NULL,
            source_agent TEXT NOT NULL
        )",
    )
    .execute(connection)
    .await?;
    diesel::sql_query("CREATE INDEX IF NOT EXISTS idx_events_variant ON events(variant)")
        .execute(connection)
        .await?;
    diesel::sql_query(
        "CREATE TABLE IF NOT EXISTS lanes (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL,
            creator TEXT NOT NULL,
            parent_lane_id TEXT NULL,
            origin_event_id UUID NULL,
            origin_message_id TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            archived_at TEXT NULL
        )",
    )
    .execute(connection)
    .await?;
    diesel::sql_query(
        "CREATE INDEX IF NOT EXISTS idx_lanes_project_status ON lanes(project_id, status)",
    )
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn insert_project(
    connection: &mut AsyncPgConnection,
    project: &NewProject,
) -> QueryResult<Project> {
    diesel::insert_into(schema::projects::table)
        .values(project)
        .get_result::<Project>(connection)
        .await
}

pub async fn load_projects(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Project>> {
    use crate::schema::projects::dsl::{label, projects};

    projects
        .order(label.asc())
        .load::<Project>(connection)
        .await
}

pub async fn project_exists(connection: &mut AsyncPgConnection, project_id: &str) -> Result<bool> {
    use crate::schema::projects::dsl::{id, projects};

    let parsed_id = Uuid::parse_str(project_id)?;
    let row = projects
        .filter(id.eq(parsed_id))
        .select(id)
        .first::<Uuid>(connection)
        .await
        .optional()?;

    Ok(row.is_some())
}

pub async fn append_event(
    connection: &mut AsyncPgConnection,
    event: &SemanticEvent,
) -> Result<Event> {
    let row = NewEvent {
        id: event.event_id().0,
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
    use crate::schema::events::dsl::{events, id, rowid};

    events
        .filter(id.eq(event_id.0))
        .select(rowid)
        .first::<i64>(connection)
        .await
        .optional()
}

pub async fn get_event_by_id(
    connection: &mut AsyncPgConnection,
    event_id: EventId,
) -> Result<Option<SemanticEvent>> {
    use crate::schema::events::dsl::{events, id};

    let row = events
        .filter(id.eq(event_id.0))
        .first::<Event>(connection)
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
