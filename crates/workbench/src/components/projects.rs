use dioxus::prelude::*;
use dioxus_icons::lucide::Plus;
use serde::{Deserialize, Serialize};

use crate::components::{
    button::{Button, ButtonSize, ButtonVariant},
    dialog::{Dialog, DialogDescription, DialogTitle},
    sidebar::{
        SidebarGroup, SidebarGroupContent, SidebarGroupLabel, SidebarMenu, SidebarMenuButton,
        SidebarMenuItem, SidebarMenuSkeleton,
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ProjectNavItem {
    id: String,
    label: String,
    path: String,
}

#[component]
pub fn Projects() -> Element {
    let mut projects = use_resource(|| async move { list_projects().await });
    let mut dialog_open = use_signal(|| false);
    let mut project_label = use_signal(String::new);
    let mut project_path = use_signal(String::new);
    let mut create_error = use_signal(|| None::<String>);
    let mut is_creating = use_signal(|| false);

    rsx! {
        SidebarGroup {
            SidebarGroupLabel { "Projects" }
            SidebarGroupContent {
                SidebarMenu {
                    match &*projects.read_unchecked() {
                        Some(Ok(items)) => rsx! {
                            for project in items.iter() {
                                ProjectNavItemLink { key: "{project.id}", project: project.clone() }
                            }
                        },
                        Some(Err(error)) => rsx! {
                            SidebarMenuItem {
                                SidebarMenuButton { "Unable to load projects: {error}" }
                            }
                        },
                        None => rsx! {
                            SidebarMenuItem { SidebarMenuSkeleton {} }
                            SidebarMenuItem { SidebarMenuSkeleton {} }
                            SidebarMenuItem { SidebarMenuSkeleton {} }
                        },
                    }
                    SidebarMenuItem {
                        SidebarMenuButton {
                            as: move |attributes: Vec<Attribute>| {
                                rsx! {
                                    Button {
                                        variant: ButtonVariant::Outline,
                                        size: ButtonSize::Sm,
                                        attributes,
                                        onclick: move |_| {
                                            create_error.set(None);
                                            dialog_open.set(true);
                                        },
                                        Plus { size: "16px" },
                                        "Add Project"
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
        Dialog {
            open: dialog_open(),
            on_open_change: move |open| dialog_open.set(open),
            DialogTitle { "Add Project" }
            DialogDescription { "Create a project entry in the memory database." }
            form {
                class: "mt-4 flex flex-col gap-4",
                onsubmit: move |event| {
                    event.prevent_default();

                    let label = project_label().trim().to_string();
                    let path = project_path().trim().to_string();

                    if label.is_empty() || path.is_empty() {
                        create_error.set(Some("Project name and path are required.".to_string()));
                        return;
                    }

                    is_creating.set(true);
                    create_error.set(None);

                    spawn(async move {
                        match create_project(label, path).await {
                            Ok(_) => {
                                project_label.set(String::new());
                                project_path.set(String::new());
                                dialog_open.set(false);
                                projects.restart();
                            }
                            Err(error) => {
                                create_error.set(Some(error.to_string()));
                            }
                        }

                        is_creating.set(false);
                    });
                },
                label {
                    class: "flex flex-col gap-1 text-sm",
                    r#for: "project-label",
                    "Project Name"
                    input {
                        id: "project-label",
                        class: "rounded-md border px-3 py-2 text-sm",
                        value: "{project_label}",
                        disabled: is_creating(),
                        oninput: move |event| project_label.set(event.value()),
                    }
                }
                label {
                    class: "flex flex-col gap-1 text-sm",
                    r#for: "project-path",
                    "Project Path"
                    input {
                        id: "project-path",
                        class: "rounded-md border px-3 py-2 text-sm",
                        value: "{project_path}",
                        disabled: is_creating(),
                        oninput: move |event| project_path.set(event.value()),
                    }
                }
                if let Some(error) = create_error() {
                    p { class: "text-sm text-red-600", "{error}" }
                }
                div { class: "flex justify-end gap-2",
                    Button {
                        variant: ButtonVariant::Secondary,
                        r#type: "button",
                        disabled: is_creating(),
                        onclick: move |_| dialog_open.set(false),
                        "Cancel"
                    }
                    Button {
                        r#type: "submit",
                        disabled: is_creating(),
                        if is_creating() {
                            "Creating..."
                        } else {
                            "Create Project"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ProjectNavItemLink(project: ProjectNavItem) -> Element {
    let href = format!("/projects/{}", project.id);
    let label = project.label.clone();
    let title = project.path.clone();

    rsx! {
        SidebarMenuItem {
            SidebarMenuButton {
                as: move |attributes: Vec<Attribute>| {
                    let href = href.clone();
                    let label = label.clone();
                    let title = title.clone();

                    rsx! {
                        a {
                            href: href,
                            title: title,
                            ..attributes,
                            "{label}"
                        }
                    }
                },
            }
        }
    }
}

#[server]
async fn list_projects() -> ServerFnResult<Vec<ProjectNavItem>> {
    #[cfg(feature = "server")]
    {
        tokio::task::spawn_blocking(load_projects)
            .await
            .map_err(|error| ServerFnError::new(format!("project query task failed: {error}")))?
    }

    #[cfg(not(feature = "server"))]
    {
        Err(ServerFnError::new(
            "projects can only be loaded on the server",
        ))
    }
}

#[server]
async fn create_project(label: String, path: String) -> ServerFnResult<ProjectNavItem> {
    #[cfg(feature = "server")]
    {
        tokio::task::spawn_blocking(move || insert_project(label, path))
            .await
            .map_err(|error| ServerFnError::new(format!("project insert task failed: {error}")))?
    }

    #[cfg(not(feature = "server"))]
    {
        Err(ServerFnError::new(
            "projects can only be created on the server",
        ))
    }
}

#[cfg(feature = "server")]
fn load_projects() -> ServerFnResult<Vec<ProjectNavItem>> {
    use diesel::prelude::*;
    use mmat_db::models::Project;
    use mmat_db::schema::projects::dsl::{label, projects};

    let database_url = crate::cli::pg_dsn();
    let mut connection = PgConnection::establish(&database_url)
        .map_err(|error| ServerFnError::new(format!("could not connect to database: {error}")))?;

    projects
        .order(label.asc())
        .load::<Project>(&mut connection)
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

#[cfg(feature = "server")]
fn insert_project(label: String, path: String) -> ServerFnResult<ProjectNavItem> {
    use diesel::prelude::*;
    use mmat_db::models::{NewProject, Project};
    use mmat_db::schema::projects;

    let label = label.trim().to_string();
    let path = path.trim().to_string();

    if label.is_empty() || path.is_empty() {
        return Err(ServerFnError::new("Project name and path are required."));
    }

    let database_url = crate::cli::pg_dsn();
    let mut connection = PgConnection::establish(&database_url)
        .map_err(|error| ServerFnError::new(format!("could not connect to database: {error}")))?;

    diesel::insert_into(projects::table)
        .values(&NewProject { label, path })
        .get_result::<Project>(&mut connection)
        .map(|project| ProjectNavItem {
            id: project.id.to_string(),
            label: project.label,
            path: project.path,
        })
        .map_err(|error| ServerFnError::new(format!("could not create project: {error}")))
}
