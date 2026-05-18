use diesel::{ExpressionMethods, QueryDsl, QueryResult};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::{
    models::{NewProject, Project},
    schema,
};

pub async fn load_projects(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Project>> {
    use crate::schema::projects::dsl::{label, projects};

    projects
        .order(label.asc())
        .load::<Project>(connection)
        .await
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

pub async fn project_exists(
    connection: &mut AsyncPgConnection,
    project_id: &str,
) -> QueryResult<bool> {
    use crate::schema::projects::dsl::{id, projects};

    let Ok(parsed_project_id) = Uuid::parse_str(project_id) else {
        return Ok(false);
    };

    diesel::select(diesel::dsl::exists(
        projects.filter(id.eq(parsed_project_id)),
    ))
    .get_result(connection)
    .await
}
