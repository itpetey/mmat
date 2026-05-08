//! Git worktree isolation and lifecycle management.
//!
//! Worktrees allow simultaneous checkout of multiple branches from the
//! same repository. This module provides creation, patching, command
//! execution, and cleanup helpers built on `git worktree`.

use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::{info, warn};

/// Errors that can occur during worktree operations.
#[derive(Error, Debug)]
pub enum WorktreeError {
    /// An I/O operation failed while reading or writing files.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A git command returned a non-zero exit status or failed to start.
    #[error("Git error: {0}")]
    Git(String),

    /// The requested worktree does not exist or could not be located.
    #[error("Worktree not found: {0}")]
    NotFound(String),

    /// A command executed inside the worktree returned a non-zero exit code.
    #[error("Command failed: {0}")]
    CommandFailed(String),
}

/// Handle representing an active git worktree.
///
/// Tracks the repository path, worktree directory, branch name, and active
/// state. Dropping the handle without calling [`delete`](Self::delete) logs a
/// warning but does not automatically remove the worktree from disk.
pub struct WorktreeHandle {
    repo_path: PathBuf,
    worktree_path: PathBuf,
    branch_name: String,
    active: bool,
}

impl WorktreeHandle {
    /// Create a new git worktree at
    /// `<repo_path>/.worktrees/<branch_name>` using `git worktree add -b`.
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

    /// Create a fallback directory on disk when `git worktree` is unavailable.
    ///
    /// Creates `<repo_path>/.mmat-worktree-<task_id>` without invoking git.
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

    /// Return the filesystem path of the worktree directory.
    pub fn path(&self) -> &Path {
        &self.worktree_path
    }

    /// Return the git branch name associated with this worktree.
    pub fn branch_name(&self) -> &str {
        &self.branch_name
    }

    /// Apply a unified diff patch to the worktree using `git apply`.
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

    /// Run an arbitrary command inside the worktree directory.
    ///
    /// Returns a tuple of `(exit_code, stdout, stderr)`.
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

    /// Remove the git worktree via `git worktree remove -f`, falling back to
    /// a recursive directory removal if the git command fails.
    ///
    /// Consumes the handle — after this call returns the worktree is no longer
    /// tracked and should be considered deleted.
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
