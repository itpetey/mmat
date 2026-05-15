use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectNavItem {
    pub id: String,
    pub label: String,
    pub path: String,
}

#[server]
pub async fn list_projects() -> ServerFnResult<Vec<ProjectNavItem>> {
    let database_url = crate::cli::pg_dsn();
    let mut connection = mmat_db::connect(&database_url)
        .await
        .map_err(|error| ServerFnError::new(format!("could not connect to database: {error}")))?;

    mmat_db::load_projects(&mut connection)
        .await
        .map(|items| {
            items
                .into_iter()
                .map(|project| ProjectNavItem {
                    id: project.id.to_string(),
                    label: project.label,
                    path: project.path,
                })
                .collect()
        })
        .map_err(|error| ServerFnError::new(format!("could not load projects: {error}")))
}

#[server]
pub async fn create_project(label: String, path: String) -> ServerFnResult<ProjectNavItem> {
    use mmat_db::models::NewProject;

    let label = label.trim().to_string();
    let path = path.trim().to_string();

    if label.is_empty() || path.is_empty() {
        return Err(ServerFnError::new("Project name and path are required."));
    }

    let database_url = crate::cli::pg_dsn();
    let mut connection = mmat_db::connect(&database_url)
        .await
        .map_err(|error| ServerFnError::new(format!("could not connect to database: {error}")))?;

    mmat_db::insert_project(&mut connection, &NewProject { label, path })
        .await
        .map(|project| ProjectNavItem {
            id: project.id.to_string(),
            label: project.label,
            path: project.path,
        })
        .map_err(|error| ServerFnError::new(format!("could not create project: {error}")))
}
