//! Persistent project registry for multi-project MMAT runs.

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::MmatError;

const REGISTRY_ENV: &str = "MMAT_PROJECT_REGISTRY_SQLITE_PATH";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: ProjectId,
    pub name: String,
    pub root: PathBuf,
    pub data_dir: PathBuf,
    pub enabled: bool,
    pub qdrant_collection_prefix: String,
    pub repo_label: Option<String>,
}

#[derive(Debug, Error)]
pub enum ProjectRegistryError {
    #[error("project registry failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("project registry io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("project root already exists in registry: {0}")]
    DuplicateRoot(String),
    #[error("project not found: {0}")]
    NotFound(ProjectId),
    #[error("invalid project id: {0}")]
    InvalidProjectId(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProjectId(String);

#[derive(Clone, Debug)]
pub struct NewProject {
    pub name: String,
    pub root: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub enabled: bool,
    pub qdrant_collection_prefix: Option<String>,
    pub repo_label: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProjectRegistryStore {
    path: PathBuf,
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl ProjectId {
    pub fn new(value: impl Into<String>) -> Result<Self, ProjectRegistryError> {
        let value = value.into();
        if value.trim().is_empty()
            || !value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(ProjectRegistryError::InvalidProjectId(value));
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn generated() -> Self {
        Self(format!("project_{}", uuid::Uuid::new_v4().simple()))
    }
}

impl NewProject {
    pub fn from_root(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            root: root.into(),
            data_dir: None,
            enabled: true,
            qdrant_collection_prefix: None,
            repo_label: None,
        }
    }
}

impl ProjectConfig {
    pub fn default_for_root(root: impl Into<PathBuf>) -> Result<Self, ProjectRegistryError> {
        let root = normalise_root(root.into())?;
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_string();
        let id = ProjectId::new("default")?;

        Ok(Self {
            qdrant_collection_prefix: default_collection_prefix(&id),
            data_dir: root.join(".mmat"),
            repo_label: Some(name.clone()),
            enabled: true,
            id,
            name,
            root,
        })
    }
}

impl ProjectRegistryStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ProjectRegistryError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let store = Self { path };
        store.initialise()?;
        Ok(store)
    }

    pub fn open_default() -> Result<Self, ProjectRegistryError> {
        Self::open(default_registry_path()?)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn register_project(
        &self,
        project: NewProject,
    ) -> Result<ProjectConfig, ProjectRegistryError> {
        let root = normalise_root(project.root)?;
        let data_dir = project.data_dir.unwrap_or_else(|| root.join(".mmat"));
        let id = ProjectId::generated();
        let qdrant_collection_prefix = project
            .qdrant_collection_prefix
            .unwrap_or_else(|| default_collection_prefix(&id));
        let config = ProjectConfig {
            id,
            name: project.name,
            root,
            data_dir,
            enabled: project.enabled,
            qdrant_collection_prefix,
            repo_label: project.repo_label,
        };

        self.insert_project(&config)?;
        Ok(config)
    }

    pub fn ensure_default_project(
        &self,
        root: impl Into<PathBuf>,
    ) -> Result<ProjectConfig, ProjectRegistryError> {
        let root = normalise_root(root.into())?;
        if let Some(project) = self.find_by_root(&root)? {
            return Ok(project);
        }

        let config = ProjectConfig::default_for_root(root)?;
        self.insert_project(&config)?;
        Ok(config)
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectConfig>, ProjectRegistryError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, root, data_dir, enabled, qdrant_collection_prefix, repo_label
             FROM projects
             ORDER BY created_at ASC, name ASC",
        )?;
        let rows = statement.query_map([], decode_project_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(ProjectRegistryError::from)
    }

    pub fn enabled_projects(&self) -> Result<Vec<ProjectConfig>, ProjectRegistryError> {
        Ok(self
            .list_projects()?
            .into_iter()
            .filter(|project| project.enabled)
            .collect())
    }

    pub fn update_project(&self, project: &ProjectConfig) -> Result<(), ProjectRegistryError> {
        let root = normalise_root(project.root.clone())?;
        if let Some(existing) = self.find_by_root(&root)?
            && existing.id != project.id
        {
            return Err(ProjectRegistryError::DuplicateRoot(
                root.display().to_string(),
            ));
        }

        let changed = self.connection()?.execute(
            "UPDATE projects
             SET name = ?2,
                 root = ?3,
                 data_dir = ?4,
                 enabled = ?5,
                 qdrant_collection_prefix = ?6,
                 repo_label = ?7,
                 updated_at = ?8
             WHERE id = ?1",
            params![
                project.id.as_str(),
                project.name,
                root.to_string_lossy(),
                project.data_dir.to_string_lossy(),
                project.enabled,
                project.qdrant_collection_prefix,
                project.repo_label,
                now_unix_seconds(),
            ],
        )?;

        if changed == 0 {
            return Err(ProjectRegistryError::NotFound(project.id.clone()));
        }

        Ok(())
    }

    pub fn get_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<ProjectConfig, ProjectRegistryError> {
        self.connection()?
            .query_row(
                "SELECT id, name, root, data_dir, enabled, qdrant_collection_prefix, repo_label
                 FROM projects
                 WHERE id = ?1",
                [project_id.as_str()],
                decode_project_row,
            )
            .optional()?
            .ok_or_else(|| ProjectRegistryError::NotFound(project_id.clone()))
    }

    fn insert_project(&self, project: &ProjectConfig) -> Result<(), ProjectRegistryError> {
        if self.find_by_root(&project.root)?.is_some() {
            return Err(ProjectRegistryError::DuplicateRoot(
                project.root.display().to_string(),
            ));
        }

        self.connection()?.execute(
            "INSERT INTO projects (
                 id, name, root, data_dir, enabled, qdrant_collection_prefix, repo_label,
                 created_at, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                project.id.as_str(),
                project.name,
                project.root.to_string_lossy(),
                project.data_dir.to_string_lossy(),
                project.enabled,
                project.qdrant_collection_prefix,
                project.repo_label,
                now_unix_seconds(),
            ],
        )?;
        Ok(())
    }

    fn find_by_root(&self, root: &Path) -> Result<Option<ProjectConfig>, ProjectRegistryError> {
        let root = normalise_root(root.to_path_buf())?;
        self.connection()?
            .query_row(
                "SELECT id, name, root, data_dir, enabled, qdrant_collection_prefix, repo_label
                 FROM projects
                 WHERE root = ?1",
                [root.to_string_lossy().to_string()],
                decode_project_row,
            )
            .optional()
            .map_err(ProjectRegistryError::from)
    }

    fn initialise(&self) -> Result<(), ProjectRegistryError> {
        self.connection()?.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS projects (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL,
                 root TEXT NOT NULL UNIQUE,
                 data_dir TEXT NOT NULL,
                 enabled INTEGER NOT NULL,
                 qdrant_collection_prefix TEXT NOT NULL,
                 repo_label TEXT,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );",
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection, ProjectRegistryError> {
        Ok(Connection::open(&self.path)?)
    }
}

impl From<ProjectRegistryError> for MmatError {
    fn from(value: ProjectRegistryError) -> Self {
        Self::Config(value.to_string())
    }
}

pub fn default_collection_prefix(project_id: &ProjectId) -> String {
    format!("p_{}", sanitise_collection_component(project_id.as_str()))
}

pub fn prefix_collection_id(prefix: &str, collection: &str) -> String {
    let prefix = sanitise_collection_component(prefix);
    let collection = sanitise_collection_component(collection);
    if prefix.is_empty() {
        return collection;
    }

    format!("{prefix}__{collection}",)
}

fn decode_project_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectConfig> {
    Ok(ProjectConfig {
        id: ProjectId(row.get(0)?),
        name: row.get(1)?,
        root: PathBuf::from(row.get::<_, String>(2)?),
        data_dir: PathBuf::from(row.get::<_, String>(3)?),
        enabled: row.get(4)?,
        qdrant_collection_prefix: row.get(5)?,
        repo_label: row.get(6)?,
    })
}

fn default_registry_path() -> Result<PathBuf, ProjectRegistryError> {
    if let Some(path) = env::var(REGISTRY_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(PathBuf::from(path));
    }

    Ok(env::current_dir()?.join(".mmat").join("projects.sqlite3"))
}

fn normalise_root(root: PathBuf) -> Result<PathBuf, ProjectRegistryError> {
    if root.exists() {
        Ok(root.canonicalize()?)
    } else if root.is_absolute() {
        Ok(root)
    } else {
        Ok(env::current_dir()?.join(root))
    }
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn sanitise_collection_component(value: &str) -> String {
    let mut output = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();

    while output.contains("__") {
        output = output.replace("__", "_");
    }

    output.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        NewProject, ProjectConfig, ProjectId, ProjectRegistryError, ProjectRegistryStore,
        prefix_collection_id,
    };

    #[test]
    fn creates_lists_and_updates_projects() {
        let temp = tempfile_path("registry-create-list-update");
        let root = temp.join("repo");
        std::fs::create_dir_all(&root).expect("project root should be created");
        let store = ProjectRegistryStore::open(temp.join("projects.sqlite3"))
            .expect("registry should open");

        let project = store
            .register_project(NewProject::from_root("Test", &root))
            .expect("project should register");
        assert_eq!(
            store.list_projects().expect("projects should list").len(),
            1
        );

        let mut updated = project.clone();
        updated.name = "MMAT Test".to_string();
        updated.enabled = false;
        store
            .update_project(&updated)
            .expect("project should update");

        let loaded = store.get_project(&project.id).expect("project should load");
        assert_eq!(loaded.name, "MMAT Test");
        assert!(!loaded.enabled);
    }

    #[test]
    fn rejects_duplicate_project_roots() {
        let temp = tempfile_path("registry-duplicates");
        let root = temp.join("repo");
        std::fs::create_dir_all(&root).expect("project root should be created");
        let store = ProjectRegistryStore::open(temp.join("projects.sqlite3"))
            .expect("registry should open");

        store
            .register_project(NewProject::from_root("One", &root))
            .expect("first project should register");
        let error = store
            .register_project(NewProject::from_root("Two", &root))
            .expect_err("duplicate root should be rejected");

        assert!(matches!(error, ProjectRegistryError::DuplicateRoot(_)));
    }

    #[test]
    fn default_project_uses_project_data_dir() {
        let temp = tempfile_path("registry-default-project");
        let root = temp.join("repo");
        std::fs::create_dir_all(&root).expect("project root should be created");

        let project =
            ProjectConfig::default_for_root(&root).expect("default project should be created");
        assert_eq!(
            project.id,
            ProjectId::new("default").expect("id should parse")
        );
        assert_eq!(project.data_dir, root.canonicalize().unwrap().join(".mmat"));
    }

    #[test]
    fn collection_prefixes_are_deterministic_and_distinct() {
        let first = prefix_collection_id("p_alpha", "workspace-code-repo");
        let second = prefix_collection_id("p_beta", "workspace-code-repo");

        assert_eq!(first, "p_alpha__workspace_code_repo");
        assert_eq!(second, "p_beta__workspace_code_repo");
        assert_ne!(first, second);
    }

    fn tempfile_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("mmat-{name}-{}", uuid::Uuid::new_v4()))
    }
}
