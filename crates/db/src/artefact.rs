use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, QueryResult};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::{
    models::{Artefact, NewArtefact},
    schema,
};

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

pub async fn insert_artefact(
    connection: &mut AsyncPgConnection,
    artefact: &NewArtefact,
) -> QueryResult<Artefact> {
    diesel::insert_into(schema::artefacts::table)
        .values(artefact)
        .get_result::<Artefact>(connection)
        .await
}
