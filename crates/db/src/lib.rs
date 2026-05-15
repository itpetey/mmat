use diesel::{ConnectionResult, QueryResult, prelude::*};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};

use crate::models::{NewProject, Project};

pub mod models;
pub mod schema;

pub async fn connect(url: &str) -> ConnectionResult<AsyncPgConnection> {
    AsyncPgConnection::establish(url).await
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
