use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, QueryResult};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::{
    models::{Memory, NewMemory},
    schema,
};

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

pub async fn insert_memory(
    connection: &mut AsyncPgConnection,
    memory: &NewMemory,
) -> QueryResult<Memory> {
    diesel::insert_into(schema::memories::table)
        .values(memory)
        .get_result::<Memory>(connection)
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

pub async fn query_all_memories(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Memory>> {
    use crate::schema::memories::dsl::memories;

    memories.load::<Memory>(connection).await
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
