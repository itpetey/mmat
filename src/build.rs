//! Per-project build queues and serial workers.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Notify;

use crate::{
    MmatError,
    project::{ProjectConfig, ProjectId},
    workflow::DesignHandoff,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BuildJobId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildJobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildJob {
    pub id: BuildJobId,
    pub project_id: ProjectId,
    pub status: BuildJobStatus,
    pub handoff: DesignHandoff,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct BuildQueueStore {
    path: PathBuf,
}

#[derive(Debug, Error)]
pub enum BuildQueueError {
    #[error("build queue failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("build queue io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("build queue JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("build job not found: {0}")]
    NotFound(BuildJobId),
    #[error("unknown build job status: {0}")]
    UnknownStatus(String),
}

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("implementation planning is not implemented yet")]
    PlanningNotImplemented,
    #[error("execution is not implemented yet")]
    ExecutionNotImplemented,
}

pub struct BuildWorkerHandle {
    project_id: ProjectId,
    notify: Arc<Notify>,
    join_handle: tokio::task::JoinHandle<()>,
}

pub struct BuildEngine {
    project: ProjectConfig,
}

impl std::fmt::Display for BuildJobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl BuildJobId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn generated() -> Self {
        Self(format!("job_{}", uuid::Uuid::new_v4().simple()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl BuildJobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn from_db(value: String) -> Result<Self, BuildQueueError> {
        match value.as_str() {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => Err(BuildQueueError::UnknownStatus(value)),
        }
    }
}

impl BuildQueueStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, BuildQueueError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let store = Self { path };
        store.initialise()?;
        Ok(store)
    }

    pub fn for_project(project: &ProjectConfig) -> Result<Self, BuildQueueError> {
        Self::open(project.data_dir.join("build-queue.sqlite3"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn enqueue(
        &self,
        project_id: &ProjectId,
        handoff: DesignHandoff,
    ) -> Result<BuildJob, BuildQueueError> {
        let job = BuildJob {
            id: BuildJobId::generated(),
            project_id: project_id.clone(),
            status: BuildJobStatus::Pending,
            handoff,
            error: None,
            created_at: now_unix_seconds(),
            updated_at: now_unix_seconds(),
            started_at: None,
            completed_at: None,
        };
        let handoff_json = serde_json::to_string(&job.handoff)?;

        self.connection()?.execute(
            "INSERT INTO build_jobs (
                 id, project_id, status, handoff_json, error,
                 created_at, updated_at, started_at, completed_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                job.id.as_str(),
                job.project_id.as_str(),
                job.status.as_str(),
                handoff_json,
                job.error,
                job.created_at,
                job.updated_at,
                job.started_at,
                job.completed_at,
            ],
        )?;

        Ok(job)
    }

    pub fn list_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<BuildJob>, BuildQueueError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, status, handoff_json, error,
                    created_at, updated_at, started_at, completed_at
             FROM build_jobs
             WHERE project_id = ?1
             ORDER BY created_at ASC",
        )?;
        let rows = statement.query_map([project_id.as_str()], decode_build_job_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(BuildQueueError::from)
    }

    pub fn next_pending(
        &self,
        project_id: &ProjectId,
    ) -> Result<Option<BuildJob>, BuildQueueError> {
        self.connection()?
            .query_row(
                "SELECT id, project_id, status, handoff_json, error,
                        created_at, updated_at, started_at, completed_at
                 FROM build_jobs
                 WHERE project_id = ?1 AND status = 'pending'
                 ORDER BY created_at ASC
                 LIMIT 1",
                [project_id.as_str()],
                decode_build_job_row,
            )
            .optional()
            .map_err(BuildQueueError::from)
    }

    pub fn mark_running(&self, job_id: &BuildJobId) -> Result<(), BuildQueueError> {
        let now = now_unix_seconds();
        self.update_status(job_id, BuildJobStatus::Running, None, Some(now), None, now)
    }

    pub fn mark_succeeded(&self, job_id: &BuildJobId) -> Result<(), BuildQueueError> {
        let now = now_unix_seconds();
        self.update_status(
            job_id,
            BuildJobStatus::Succeeded,
            None,
            None,
            Some(now),
            now,
        )
    }

    pub fn mark_failed(
        &self,
        job_id: &BuildJobId,
        error: impl Into<String>,
    ) -> Result<(), BuildQueueError> {
        let now = now_unix_seconds();
        self.update_status(
            job_id,
            BuildJobStatus::Failed,
            Some(error.into()),
            None,
            Some(now),
            now,
        )
    }

    pub fn recover_stale_running(&self, project_id: &ProjectId) -> Result<usize, BuildQueueError> {
        let changed = self.connection()?.execute(
            "UPDATE build_jobs
             SET status = 'pending',
                 error = NULL,
                 started_at = NULL,
                 updated_at = ?2
             WHERE project_id = ?1 AND status = 'running'",
            params![project_id.as_str(), now_unix_seconds()],
        )?;
        Ok(changed)
    }

    fn update_status(
        &self,
        job_id: &BuildJobId,
        status: BuildJobStatus,
        error: Option<String>,
        started_at: Option<i64>,
        completed_at: Option<i64>,
        updated_at: i64,
    ) -> Result<(), BuildQueueError> {
        let changed = self.connection()?.execute(
            "UPDATE build_jobs
             SET status = ?2,
                 error = COALESCE(?3, error),
                 started_at = COALESCE(?4, started_at),
                 completed_at = COALESCE(?5, completed_at),
                 updated_at = ?6
             WHERE id = ?1",
            params![
                job_id.as_str(),
                status.as_str(),
                error,
                started_at,
                completed_at,
                updated_at,
            ],
        )?;

        if changed == 0 {
            return Err(BuildQueueError::NotFound(job_id.clone()));
        }

        Ok(())
    }

    fn initialise(&self) -> Result<(), BuildQueueError> {
        self.connection()?.execute_batch(
            "CREATE TABLE IF NOT EXISTS build_jobs (
                 id TEXT PRIMARY KEY NOT NULL,
                 project_id TEXT NOT NULL,
                 status TEXT NOT NULL,
                 handoff_json TEXT NOT NULL,
                 error TEXT,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 started_at INTEGER,
                 completed_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS idx_build_jobs_project_status
             ON build_jobs(project_id, status, created_at);",
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection, BuildQueueError> {
        Ok(Connection::open(&self.path)?)
    }
}

impl BuildEngine {
    pub fn new(project: ProjectConfig) -> Self {
        Self { project }
    }

    pub async fn run(&self, _job: &BuildJob) -> Result<(), BuildError> {
        self.plan_implementation().await?;
        self.execute().await
    }

    async fn plan_implementation(&self) -> Result<(), BuildError> {
        let _project_root = &self.project.root;
        Err(BuildError::PlanningNotImplemented)
    }

    async fn execute(&self) -> Result<(), BuildError> {
        Err(BuildError::ExecutionNotImplemented)
    }
}

impl BuildWorkerHandle {
    pub fn project_id(&self) -> &ProjectId {
        &self.project_id
    }

    pub fn notify(&self) {
        self.notify.notify_one();
    }

    pub fn abort(&self) {
        self.join_handle.abort();
    }
}

pub fn spawn_project_worker(
    project: ProjectConfig,
    store: Arc<BuildQueueStore>,
) -> BuildWorkerHandle {
    let project_id = project.id.clone();
    let notify = Arc::new(Notify::new());
    let worker_notify = notify.clone();
    let worker_project_id = project_id.clone();
    let join_handle = tokio::spawn(async move {
        let engine = BuildEngine::new(project);
        loop {
            if let Err(error) =
                drain_project_queue(&worker_project_id, store.as_ref(), &engine).await
            {
                tracing::error!(
                    target: "mmat::build",
                    project_id = %worker_project_id,
                    "build worker failed: {error}"
                );
            }
            worker_notify.notified().await;
        }
    });

    BuildWorkerHandle {
        project_id,
        notify,
        join_handle,
    }
}

pub async fn drain_project_queue(
    project_id: &ProjectId,
    store: &BuildQueueStore,
    engine: &BuildEngine,
) -> Result<(), BuildQueueError> {
    while let Some(job) = store.next_pending(project_id)? {
        store.mark_running(&job.id)?;
        match engine.run(&job).await {
            Ok(()) => store.mark_succeeded(&job.id)?,
            Err(error) => store.mark_failed(&job.id, error.to_string())?,
        }
    }

    Ok(())
}

fn decode_build_job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BuildJob> {
    let status = BuildJobStatus::from_db(row.get(2)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let handoff_json: String = row.get(3)?;
    let handoff = serde_json::from_str(&handoff_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(error))
    })?;

    Ok(BuildJob {
        id: BuildJobId(row.get(0)?),
        project_id: ProjectId::new(row.get::<_, String>(1)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        status,
        handoff,
        error: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        started_at: row.get(7)?,
        completed_at: row.get(8)?,
    })
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

impl From<BuildQueueError> for MmatError {
    fn from(value: BuildQueueError) -> Self {
        Self::Workflow(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        build::{
            BuildEngine, BuildJobStatus, BuildQueueStore, drain_project_queue, spawn_project_worker,
        },
        project::{ProjectConfig, ProjectId},
        workflow::DesignHandoff,
    };

    #[test]
    fn queues_are_independent_per_project() {
        let store = BuildQueueStore::open(tempfile_path("queue-independent").join("queue.sqlite3"))
            .expect("queue should open");
        let first = ProjectId::new("first").expect("id should parse");
        let second = ProjectId::new("second").expect("id should parse");

        store
            .enqueue(&first, handoff("one"))
            .expect("first job should enqueue");
        store
            .enqueue(&second, handoff("two"))
            .expect("second job should enqueue");

        assert_eq!(
            store
                .list_for_project(&first)
                .expect("first queue should list")
                .len(),
            1
        );
        assert_eq!(
            store
                .list_for_project(&second)
                .expect("second queue should list")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn worker_drains_only_its_project_queue() {
        let store = BuildQueueStore::open(tempfile_path("queue-drain").join("queue.sqlite3"))
            .expect("queue should open");
        let first = project("first");
        let second = project("second");

        store
            .enqueue(&first.id, handoff("one"))
            .expect("first job should enqueue");
        store
            .enqueue(&second.id, handoff("two"))
            .expect("second job should enqueue");

        let engine = BuildEngine::new(first.clone());
        drain_project_queue(&first.id, &store, &engine)
            .await
            .expect("drain should complete");

        let first_jobs = store
            .list_for_project(&first.id)
            .expect("first jobs should list");
        let second_jobs = store
            .list_for_project(&second.id)
            .expect("second jobs should list");
        assert_eq!(first_jobs[0].status, BuildJobStatus::Failed);
        assert_eq!(second_jobs[0].status, BuildJobStatus::Pending);
    }

    #[test]
    fn stale_running_jobs_recover_to_pending() {
        let store = BuildQueueStore::open(tempfile_path("queue-recover").join("queue.sqlite3"))
            .expect("queue should open");
        let project_id = ProjectId::new("recover").expect("id should parse");
        let job = store
            .enqueue(&project_id, handoff("recover"))
            .expect("job should enqueue");
        store
            .mark_running(&job.id)
            .expect("job should mark running");

        let recovered = store
            .recover_stale_running(&project_id)
            .expect("stale running should recover");
        let jobs = store
            .list_for_project(&project_id)
            .expect("jobs should list");

        assert_eq!(recovered, 1);
        assert_eq!(jobs[0].status, BuildJobStatus::Pending);
    }

    #[tokio::test]
    async fn spawned_worker_records_not_implemented_failure() {
        let store = Arc::new(
            BuildQueueStore::open(tempfile_path("queue-worker").join("queue.sqlite3"))
                .expect("queue should open"),
        );
        let project = project("worker");
        store
            .enqueue(&project.id, handoff("worker"))
            .expect("job should enqueue");
        let worker = spawn_project_worker(project.clone(), store.clone());
        worker.notify();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        worker.abort();

        let jobs = store
            .list_for_project(&project.id)
            .expect("jobs should list");
        assert_eq!(jobs[0].status, BuildJobStatus::Failed);
        assert!(
            matches!(jobs[0].error.as_deref(), Some(error) if error.contains("not implemented"))
        );
    }

    fn handoff(prompt: &str) -> DesignHandoff {
        DesignHandoff {
            design_run_id: uuid::Uuid::new_v4(),
            prompt: prompt.to_string(),
            architect_plan: serde_json::json!({"summary": prompt}),
        }
    }

    fn project(id: &str) -> ProjectConfig {
        let project_id = ProjectId::new(id).expect("id should parse");
        let root = tempfile_path(id).join("repo");
        std::fs::create_dir_all(&root).expect("project root should be created");
        ProjectConfig {
            id: project_id,
            name: id.to_string(),
            root: root.clone(),
            data_dir: root.join(".mmat"),
            enabled: true,
            qdrant_collection_prefix: format!("p_{id}"),
            repo_label: Some(id.to_string()),
        }
    }

    fn tempfile_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("mmat-{name}-{}", uuid::Uuid::new_v4()))
    }
}
