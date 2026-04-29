use std::path::PathBuf;

#[cfg(test)]
use rusqlite::OptionalExtension;
use rusqlite::{Connection, params};
use serde::Serialize;
#[cfg(test)]
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::deliver::queue::BuildJobId;

#[derive(Debug, Error)]
pub enum DeliveryArtifactError {
    #[error("delivery artifact SQLite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("delivery artifact IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("delivery artifact JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug)]
pub struct DeliveryArtifactStore {
    path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryArtifact {
    ExecutionPlan,
    EvidenceLog,
    FinalReview,
    Outcome,
}

impl DeliveryArtifact {
    pub fn key(self) -> &'static str {
        match self {
            Self::ExecutionPlan => "execution-plan",
            Self::EvidenceLog => "evidence-log",
            Self::FinalReview => "final-review",
            Self::Outcome => "delivery-outcome",
        }
    }
}

impl DeliveryArtifactStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, DeliveryArtifactError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { path };
        store.initialise()?;
        Ok(store)
    }

    pub fn write_json<T>(
        &self,
        job_id: &BuildJobId,
        artifact: DeliveryArtifact,
        value: &T,
    ) -> Result<(), DeliveryArtifactError>
    where
        T: Serialize + ?Sized,
    {
        self.write_key_json(job_id, artifact.key(), value)
    }

    pub fn write_key_json<T>(
        &self,
        job_id: &BuildJobId,
        key: &str,
        value: &T,
    ) -> Result<(), DeliveryArtifactError>
    where
        T: Serialize + ?Sized,
    {
        let data = serde_json::to_string_pretty(value)?;
        self.connection()?.execute(
            "INSERT OR REPLACE INTO delivery_artifacts (job_id, artifact_key, data)
             VALUES (?1, ?2, ?3)",
            params![job_id.as_str(), key, data],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub fn read_key_json<T>(
        &self,
        job_id: &BuildJobId,
        key: &str,
    ) -> Result<Option<T>, DeliveryArtifactError>
    where
        T: DeserializeOwned,
    {
        let data = self
            .connection()?
            .query_row(
                "SELECT data FROM delivery_artifacts WHERE job_id = ?1 AND artifact_key = ?2",
                params![job_id.as_str(), key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data))
            .transpose()
            .map_err(DeliveryArtifactError::from)
    }

    fn initialise(&self) -> Result<(), DeliveryArtifactError> {
        self.connection()?.execute_batch(
            "CREATE TABLE IF NOT EXISTS delivery_artifacts (
                 job_id TEXT NOT NULL,
                 artifact_key TEXT NOT NULL,
                 data TEXT NOT NULL,
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 PRIMARY KEY (job_id, artifact_key)
             );",
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection, DeliveryArtifactError> {
        Ok(Connection::open(&self.path)?)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn writes_and_reads_json_artifacts() {
        let path = std::env::temp_dir().join(format!(
            "mmat-delivery-artifacts-{}.sqlite3",
            uuid::Uuid::new_v4().simple()
        ));
        let store = DeliveryArtifactStore::open(&path).expect("store should open");
        let job_id = BuildJobId::new("job_1");

        store
            .write_key_json(&job_id, "sample", &json!({"status": "ok"}))
            .expect("artifact should write");

        let loaded: serde_json::Value = store
            .read_key_json(&job_id, "sample")
            .expect("artifact should read")
            .expect("artifact should exist");

        assert_eq!(loaded["status"], "ok");
        let _ = std::fs::remove_file(path);
    }
}
