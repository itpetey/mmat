use std::{
    fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::{artifacts::RunArtifact, error::AppError, parsing::to_pretty_json};

const RUNS_DIR: &str = ".mmat/runs";

#[derive(Clone, Debug)]
pub(crate) struct RunStore {
    run_id: String,
    run_root: PathBuf,
}

impl RunStore {
    pub(crate) fn create(project_root: &Path) -> Result<Self, AppError> {
        let run_id = generate_run_id()?;
        Self::create_with_run_id(project_root, run_id)
    }

    fn create_with_run_id(project_root: &Path, run_id: String) -> Result<Self, AppError> {
        let run_root = project_root.join(RUNS_DIR).join(&run_id);
        fs::create_dir_all(&run_root).map_err(|error| {
            AppError::Workflow(format!(
                "failed to create run directory `{}`: {error}",
                run_root.display()
            ))
        })?;

        Ok(Self { run_id, run_root })
    }

    pub(crate) fn run_id(&self) -> &str {
        &self.run_id
    }

    pub(crate) fn run_root(&self) -> &Path {
        &self.run_root
    }

    pub(crate) fn write_json<T>(&self, artifact: RunArtifact, value: &T) -> Result<(), AppError>
    where
        T: Serialize + ?Sized,
    {
        let path = self.run_root.join(artifact.file_name());
        let payload = to_pretty_json(value)?;
        fs::write(&path, payload).map_err(|error| {
            AppError::Workflow(format!(
                "failed to write run artifact `{}`: {error}",
                path.display()
            ))
        })
    }
}

fn generate_run_id() -> Result<String, AppError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            AppError::Workflow(format!("failed to read system clock for run id: {error}"))
        })?;

    Ok(format!(
        "run-{}-{:09}-{}",
        now.as_secs(),
        now.subsec_nanos(),
        process::id()
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde_json::json;

    use super::RunStore;
    use crate::artifacts::RunArtifact;

    fn test_root() -> std::path::PathBuf {
        let unique = format!(
            "mmat-run-store-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn creates_run_directory_and_writes_json_artifacts() {
        let root = test_root();
        fs::create_dir_all(&root).expect("temp root should be created");

        let store = RunStore::create(&root).expect("run store should be created");
        assert!(store.run_root().exists());
        assert!(store.run_root().starts_with(root.join(".mmat/runs")));

        store
            .write_json(RunArtifact::RunSummary, &json!({"status": "running"}))
            .expect("artifact should be written");

        let summary_path = store.run_root().join("run-summary.json");
        let written = fs::read_to_string(summary_path).expect("artifact should be readable");
        assert!(written.contains("\"status\": \"running\""));

        fs::remove_dir_all(root).expect("temp root should be removed");
    }

    #[test]
    fn generated_run_ids_use_expected_prefix() {
        let root = test_root();
        fs::create_dir_all(&root).expect("temp root should be created");

        let store = RunStore::create(&root).expect("run store should be created");
        assert!(store.run_id().starts_with("run-"));

        fs::remove_dir_all(root).expect("temp root should be removed");
    }
}
