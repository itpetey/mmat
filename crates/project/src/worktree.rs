use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum WorktreeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Git error: {0}")]
    Git(String),

    #[error("Worktree not found: {0}")]
    NotFound(String),

    #[error("Command failed: {0}")]
    CommandFailed(String),
}

pub struct WorktreeHandle {
    repo_path: PathBuf,
    worktree_path: PathBuf,
    branch_name: String,
    active: bool,
}

impl WorktreeHandle {
    pub async fn create(repo_path: &Path, branch_name: &str) -> Result<Self, WorktreeError> {
        let worktree_path = repo_path.join(format!(".worktrees/{}", branch_name));

        info!(
            "Creating worktree: {} from repo: {}",
            worktree_path.display(),
            repo_path.display()
        );

        let output = tokio::process::Command::new("git")
            .args([
                "-C",
                repo_path.to_str().unwrap_or(""),
                "worktree",
                "add",
                worktree_path.to_str().unwrap_or(""),
                "-b",
                branch_name,
            ])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                info!("Worktree created successfully");
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                return Err(WorktreeError::Git(format!(
                    "git worktree add failed: {}",
                    stderr
                )));
            }
            Err(e) => {
                return Err(WorktreeError::Git(format!(
                    "failed to run git worktree add: {}",
                    e
                )));
            }
        }

        Ok(Self {
            repo_path: repo_path.to_path_buf(),
            worktree_path,
            branch_name: branch_name.to_string(),
            active: true,
        })
    }

    pub fn create_fallback(repo_path: &Path, task_id: &str) -> Result<Self, WorktreeError> {
        let worktree_path = repo_path.join(format!(".mmat-worktree-{}", task_id));
        std::fs::create_dir_all(&worktree_path)?;
        Ok(Self {
            repo_path: repo_path.to_path_buf(),
            worktree_path,
            branch_name: format!("task-{}", task_id),
            active: true,
        })
    }

    pub fn path(&self) -> &Path {
        &self.worktree_path
    }

    pub fn branch_name(&self) -> &str {
        &self.branch_name
    }

    pub async fn apply_patch(&self, patch: &str) -> Result<(), WorktreeError> {
        info!(
            "Applying patch to worktree: {}",
            self.worktree_path.display()
        );

        let patch_file = self.worktree_path.join(".mmat-patch.tmp");
        tokio::fs::write(&patch_file, patch).await?;

        let output = tokio::process::Command::new("git")
            .args([
                "-C",
                self.worktree_path.to_str().unwrap_or(""),
                "apply",
                "--whitespace=nowarn",
                patch_file.to_str().unwrap_or(""),
            ])
            .output()
            .await;

        let _ = tokio::fs::remove_file(&patch_file).await;

        match output {
            Ok(out) if out.status.success() => {
                info!("Patch applied successfully");
                Ok(())
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Err(WorktreeError::CommandFailed(format!(
                    "Patch apply failed: {}",
                    stderr
                )))
            }
            Err(e) => Err(WorktreeError::CommandFailed(format!(
                "Failed to run git apply: {}",
                e
            ))),
        }
    }

    pub async fn run_command(
        &self,
        command: &str,
        args: &[&str],
    ) -> Result<(i32, String, String), WorktreeError> {
        info!("Running command in worktree: {} {:?}", command, args);

        let output = tokio::process::Command::new(command)
            .args(args)
            .current_dir(&self.worktree_path)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        Ok((exit_code, stdout, stderr))
    }

    pub async fn delete(mut self) -> Result<(), WorktreeError> {
        self.active = false;
        info!("Deleting worktree: {}", self.worktree_path.display());

        let output = tokio::process::Command::new("git")
            .args([
                "-C",
                self.repo_path.to_str().unwrap_or(""),
                "worktree",
                "remove",
                "-f",
                self.worktree_path.to_str().unwrap_or(""),
            ])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                info!("Worktree removed successfully");
            }
            _ => {
                warn!("Git worktree remove failed, falling back to directory removal");
                if self.worktree_path.exists() {
                    tokio::fs::remove_dir_all(&self.worktree_path).await?;
                }
            }
        }

        Ok(())
    }
}

impl Drop for WorktreeHandle {
    fn drop(&mut self) {
        if self.active {
            warn!(
                "WorktreeHandle dropped without explicit delete: {}",
                self.worktree_path.display()
            );
        }
    }
}
