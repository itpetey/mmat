use diesel::{
    ExpressionMethods, OptionalExtension, QueryDsl, QueryResult, QueryableByName, sql_types,
};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use mmat_event_stream::event::{EventId, SemanticEvent};

use crate::{
    DbError, Result,
    models::{Event, NewEvent},
    schema,
};

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
