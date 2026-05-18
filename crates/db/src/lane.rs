use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, QueryResult};
use diesel_async::{AsyncPgConnection, RunQueryDsl, SimpleAsyncConnection};
use mmat_event_stream::event::SemanticEvent;
use uuid::Uuid;

use crate::{
    DbError, Result,
    event::append_event,
    models::{Lane, NewLane},
    schema,
};

pub async fn create_lane(connection: &mut AsyncPgConnection, lane: &NewLane) -> QueryResult<Lane> {
    diesel::insert_into(schema::lanes::table)
        .values(lane)
        .get_result::<Lane>(connection)
        .await
}

pub async fn create_lane_with_event(
    connection: &mut AsyncPgConnection,
    lane: NewLane,
    event_for_lane: impl FnOnce(&Lane) -> SemanticEvent,
) -> Result<(Lane, SemanticEvent)> {
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
    let event = event_for_lane(&lane);

    if let Err(error) = append_event(connection, &event).await {
        let _rollback = connection.batch_execute("ROLLBACK").await;
        return Err(error);
    }

    connection.batch_execute("COMMIT").await?;
    Ok((lane, event))
}

pub async fn archive_lane(
    connection: &mut AsyncPgConnection,
    lane_id: &str,
    archived_at_value: String,
) -> QueryResult<Lane> {
    use crate::schema::lanes::dsl::{archived_at, id, lanes, status, updated_at};
    let parsed_lane_id = parse_lane_id(lane_id)?;

    diesel::update(lanes.filter(id.eq(parsed_lane_id)))
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
    let Ok(parsed_lane_id) = Uuid::parse_str(lane_id) else {
        return Ok(None);
    };

    lanes
        .filter(id.eq(parsed_lane_id))
        .first::<Lane>(connection)
        .await
        .optional()
}

fn parse_lane_id(lane_id: &str) -> QueryResult<Uuid> {
    Uuid::parse_str(lane_id).map_err(|_| diesel::result::Error::NotFound)
}
