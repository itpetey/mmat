//! The Worker role receives task cards, creates git worktrees, runs an implementation loop
//! (using an LLM if configured), executes validation commands, and publishes results.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use coordinator::{
    AuthorityScope, Budget, Role, RoleContext, RoleError, RoleLifecycleState, RoleSpec, RoleType,
};
use event_stream::event::{
    ArtefactRef, EventId, EventType, EvidenceRef, RoleId as EventRoleId, SemanticEvent,
};
use llm::client::LlmClient;
use llm::executor::{Executor, ExecutorConfig};
use llm::message::{CompletionRequest, Message};
use project::worktree::WorktreeHandle;
use tracing::{info, warn};
use uuid::Uuid;

use crate::tooling::{RoleToolRegistry, RoleToolRuntime};

/// The Worker role implements task cards by creating worktrees, running implementation, and validating results.
pub struct Worker {
    id: EventRoleId,
    llm_client: Option<Arc<dyn LlmClient>>,
    #[allow(dead_code)]
    executor: Executor,
    tool_registry: RoleToolRegistry,
    tool_runtime: RoleToolRuntime,
    validation_commands: Vec<String>,
    allow_fallback_worktree: bool,
}

impl Worker {
    /// Creates a new Worker with default validation commands and no LLM client.
    pub fn new() -> Self {
        Self {
            id: EventRoleId("worker-001".to_string()),
            llm_client: None,
            executor: Executor,
            tool_registry: RoleToolRegistry::new(),
            tool_runtime: RoleToolRuntime,
            validation_commands: vec![
                "cargo fmt --all -- --check".to_string(),
                "cargo test".to_string(),
            ],
            allow_fallback_worktree: false,
        }
    }

    /// Configures the Worker with an LLM client for implementation.
    pub fn with_llm_client(mut self, llm_client: Arc<dyn LlmClient>) -> Self {
        self.llm_client = Some(llm_client);
        self
    }

    /// Configures the Worker with a custom tool registry.
    pub fn with_tool_registry(mut self, tool_registry: RoleToolRegistry) -> Self {
        self.tool_registry = tool_registry;
        self
    }

    /// Sets the validation commands to run after implementation.
    pub fn with_validation_commands(mut self, commands: Vec<String>) -> Self {
        self.validation_commands = commands;
        self
    }

    /// Whether to allow a fallback worktree if git worktree creation fails.
    pub fn with_fallback_worktree(mut self, allow: bool) -> Self {
        self.allow_fallback_worktree = allow;
        self
    }

    /// Returns whether an LLM client has been configured.
    pub fn has_llm_client(&self) -> bool {
        self.llm_client.is_some()
    }

    async fn create_worktree(
        &self,
        repo_path: &Path,
        task_id: &str,
    ) -> Result<WorktreeHandle, RoleError> {
        let branch = format!("task-{}", task_id);
        match WorktreeHandle::create(repo_path, &branch).await {
            Ok(handle) => Ok(handle),
            Err(e) if self.allow_fallback_worktree => {
                warn!(
                    "Git worktree creation failed for task {}, using fallback: {}",
                    task_id, e
                );
                WorktreeHandle::create_fallback(repo_path, task_id).map_err(|fallback_error| {
                    RoleError::Internal(format!(
                        "Failed to create fallback worktree for task {task_id}: {fallback_error}"
                    ))
                })
            }
            Err(e) => Err(RoleError::Internal(format!(
                "Failed to create worktree for task {task_id}: {e}"
            ))),
        }
    }

    async fn run_implementation_loop(
        &self,
        ctx: &RoleContext,
        task_id: &str,
        task_description: &str,
        worktree: &WorktreeHandle,
    ) -> Result<String, RoleError> {
        info!(
            "Worker implementation loop for task: {} in worktree: {}",
            task_description,
            worktree.path().display()
        );

        let mut tool_event_ids = Vec::new();

        if let Some(client) = &self.llm_client {
            let prompt = format!(
                "Implement the following task. Output the file contents you would create or modify.\n\
Task: {}\n\
Worktree path: {}",
                task_description,
                worktree.path().display()
            );

            let request = CompletionRequest::new(
                "worker-implement",
                vec![
                    Message::system(
                        "You are a worker implementing a task card. \
Output file paths and contents in the format: FILE: <path>\\n<content>",
                    ),
                    Message::user(&prompt),
                ],
            );

            let response = Executor::run(
                client.as_ref(),
                &self.tool_registry,
                &ExecutorConfig {
                    max_turns: 10,
                    max_tokens: None,
                },
                &self.tool_runtime,
                request,
            )
            .await;

            let content = match response {
                Ok(Message::Assistant { content, .. }) => content.unwrap_or_default(),
                _ => String::new(),
            };

            let files_written = Self::parse_and_write_files(&content, worktree.path()).await?;

            let tool_event = SemanticEvent::new_tool_executed(
                EventRoleId(self.id.0.clone()),
                "llm_implementation",
                task_description,
                0,
                &content,
                "",
                0,
            );
            ctx.bus.publish(tool_event.clone()).map_err(|e| {
                RoleError::Internal(format!("Failed to publish tool executed event: {e:?}"))
            })?;
            tool_event_ids.push(tool_event.event_id());

            for file_path in &files_written {
                let write_event = SemanticEvent::new_tool_executed(
                    EventRoleId(self.id.0.clone()),
                    "file_write",
                    file_path,
                    0,
                    "File written to worktree",
                    "",
                    0,
                );
                ctx.bus.publish(write_event.clone()).map_err(|e| {
                    RoleError::Internal(format!("Failed to publish tool executed event: {e:?}"))
                })?;
                tool_event_ids.push(write_event.event_id());
            }

            let patch = Self::generate_patch(worktree, &files_written).await;
            return Ok(patch);
        }

        let relative_file_path = format!("task-{}.txt", task_id);
        let file_path = Self::resolve_worktree_path(worktree.path(), &relative_file_path)?;
        let content = format!(
            "Task: {}\nImplementation attempted (no LLM client)",
            task_description
        );

        tokio::fs::write(&file_path, &content)
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to write to worktree: {e}")))?;

        let display_path = file_path.to_str().unwrap_or("unknown");
        let tool_event = SemanticEvent::new_tool_executed(
            EventRoleId(self.id.0.clone()),
            "file_write",
            display_path,
            0,
            &content,
            "",
            0,
        );
        ctx.bus.publish(tool_event.clone()).map_err(|e| {
            RoleError::Internal(format!("Failed to publish tool executed event: {e:?}"))
        })?;
        tool_event_ids.push(tool_event.event_id());

        let patch = Self::generate_patch(worktree, &[relative_file_path]).await;
        Ok(patch)
    }

    pub(crate) async fn parse_and_write_files(
        content: &str,
        worktree_path: &Path,
    ) -> Result<Vec<String>, RoleError> {
        let mut files_written = Vec::new();
        let mut current_file: Option<String> = None;
        let mut current_content = String::new();

        for line in content.lines() {
            if let Some(path) = line.strip_prefix("FILE: ") {
                if let Some(prev_file) = current_file.take() {
                    let full_path = Self::resolve_worktree_path(worktree_path, &prev_file)?;
                    if let Some(parent) = full_path.parent() {
                        tokio::fs::create_dir_all(parent).await.map_err(|e| {
                            RoleError::Internal(format!("Failed to create directory: {e}"))
                        })?;
                    }
                    tokio::fs::write(&full_path, &current_content)
                        .await
                        .map_err(|e| RoleError::Internal(format!("Failed to write file: {e}")))?;
                    files_written.push(prev_file);
                }
                current_file = Some(path.trim().to_string());
                current_content = String::new();
            } else if current_file.is_some() {
                current_content.push_str(line);
                current_content.push('\n');
            }
        }

        if let Some(prev_file) = current_file {
            let full_path = Self::resolve_worktree_path(worktree_path, &prev_file)?;
            if let Some(parent) = full_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| RoleError::Internal(format!("Failed to create directory: {e}")))?;
            }
            tokio::fs::write(&full_path, &current_content)
                .await
                .map_err(|e| RoleError::Internal(format!("Failed to write file: {e}")))?;
            files_written.push(prev_file);
        }

        Ok(files_written)
    }

    pub(crate) fn resolve_worktree_path(
        worktree_path: &Path,
        file_path: &str,
    ) -> Result<PathBuf, RoleError> {
        let path = Path::new(file_path);
        let mut relative_path = PathBuf::new();

        for component in path.components() {
            match component {
                Component::Normal(part) => relative_path.push(part),
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err(RoleError::Internal(format!(
                        "Path traversal detected: {} resolves outside worktree",
                        file_path
                    )));
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(RoleError::Internal(format!(
                        "Absolute paths are not allowed: {}",
                        file_path
                    )));
                }
            }
        }

        if relative_path.as_os_str().is_empty() {
            return Err(RoleError::Internal(format!(
                "File path is empty: {}",
                file_path
            )));
        }

        let resolved = worktree_path.join(relative_path);
        let worktree_canonical = worktree_path.canonicalize().map_err(|e| {
            RoleError::Internal(format!(
                "Failed to resolve worktree path {}: {e}",
                worktree_path.display()
            ))
        })?;
        let mut existing_ancestor = resolved.parent().unwrap_or(worktree_path);
        while !existing_ancestor.exists() {
            existing_ancestor = existing_ancestor.parent().unwrap_or(worktree_path);
            if existing_ancestor == worktree_path {
                break;
            }
        }
        let ancestor_canonical = existing_ancestor.canonicalize().map_err(|e| {
            RoleError::Internal(format!(
                "Failed to resolve parent path {}: {e}",
                existing_ancestor.display()
            ))
        })?;
        if !ancestor_canonical.starts_with(&worktree_canonical) {
            return Err(RoleError::Internal(format!(
                "Path traversal detected: {} resolves outside worktree",
                file_path
            )));
        }
        Ok(resolved)
    }

    async fn generate_patch(worktree: &WorktreeHandle, files: &[String]) -> String {
        let mut patch = String::from("# Implementation Patch\n\n");
        for file in files {
            patch.push_str(&format!("## File: {}\n\n", file));
            let full_path = worktree.path().join(file);
            if let Ok(content) = tokio::fs::read_to_string(&full_path).await {
                patch.push_str("```");
                if file.ends_with(".rs") {
                    patch.push_str("rust");
                }
                patch.push('\n');
                patch.push_str(&content);
                patch.push_str("\n```\n\n");
            }
        }
        patch
    }

    async fn run_validation(
        &self,
        ctx: &RoleContext,
        worktree: &WorktreeHandle,
        validation_commands: Vec<String>,
    ) -> Result<(bool, Vec<EventId>), RoleError> {
        let mut tool_event_ids = Vec::new();

        if validation_commands.is_empty() {
            info!("No validation commands specified, marking validation as passed");
            return Ok((true, tool_event_ids));
        }

        for cmd_str in validation_commands {
            let parts: Vec<&str> = cmd_str.split_whitespace().collect();
            let command = parts.first().unwrap_or(&"echo");
            let args: Vec<&str> = parts.iter().skip(1).copied().collect();

            let result = worktree.run_command(command, &args).await;

            let (exit_code, stdout, stderr) = match result {
                Ok((code, out, err)) => (code, out, err),
                Err(e) => (-1, String::new(), format!("Command failed: {e}")),
            };

            let tool_event = SemanticEvent::new_tool_executed(
                EventRoleId(self.id.0.clone()),
                &cmd_str,
                worktree.path().to_str().unwrap_or(""),
                exit_code,
                &stdout,
                &stderr,
                0,
            );
            ctx.bus.publish(tool_event.clone()).map_err(|e| {
                RoleError::Internal(format!("Failed to publish tool executed event: {e:?}"))
            })?;
            tool_event_ids.push(tool_event.event_id());

            if exit_code != 0 {
                warn!(
                    "Validation command failed: {} (exit {})",
                    cmd_str, exit_code
                );
                return Ok((false, tool_event_ids));
            }
        }

        Ok((true, tool_event_ids))
    }

    async fn publish_artefact(
        &self,
        ctx: &RoleContext,
        patch: &str,
    ) -> Result<ArtefactRef, RoleError> {
        let reference = format!("patch-{}|{}", Uuid::new_v4(), patch);
        let event = SemanticEvent::new_artefact_produced(
            EventRoleId(self.id.0.clone()),
            "implementation_patch",
            reference.clone(),
            EventRoleId(self.id.0.clone()),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish artefact produced event: {e:?}"))
        })?;

        Ok(ArtefactRef {
            artefact_type: "implementation_patch".to_string(),
            reference,
        })
    }

    fn build_evidence_refs(tool_event_ids: &[EventId], description: &str) -> Vec<EvidenceRef> {
        tool_event_ids
            .iter()
            .map(|id| EvidenceRef {
                event_id: *id,
                description: description.to_string(),
            })
            .collect()
    }
}

#[async_trait]
impl Role for Worker {
    fn id(&self) -> EventRoleId {
        EventRoleId(self.id.0.clone())
    }

    fn spec(&self) -> RoleSpec {
        RoleSpec {
            id: EventRoleId(self.id.0.clone()),
            role_type: RoleType::Worker,
            authority_scope: AuthorityScope::Implementation,
            default_budget: Budget {
                time_limit_seconds: 1800,
                token_limit: 300_000,
                max_retries: 3,
            },
            escalation_paths: std::collections::HashMap::new(),
            input_contract: EventType::TaskAssigned,
            output_contract: vec![
                EventType::ToolExecuted,
                EventType::ClaimMade,
                EventType::ArtefactProduced,
                EventType::TaskCompleted,
            ],
        }
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::TaskAssigned]
    }

    async fn run(self: Arc<Self>, ctx: RoleContext) -> Result<(), RoleError> {
        info!("Worker starting");

        ctx.coordinator
            .report_status(EventRoleId(self.id.0.clone()), RoleLifecycleState::Running)
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        let mut receiver = ctx.bus.subscribe(&[EventType::TaskAssigned]);
        let event = receiver.recv().await.map_err(|e| {
            RoleError::Internal(format!("Failed to receive task assigned event: {e:?}"))
        })?;

        let (contract_ref, worker_id, task_id) = match event.as_ref() {
            SemanticEvent::TaskAssigned {
                contract_ref,
                worker_id,
                task_id,
                ..
            } => (contract_ref.clone(), worker_id.clone(), task_id.clone()),
            _ => {
                return Err(RoleError::Internal(
                    "Expected TaskAssigned event".to_string(),
                ));
            }
        };

        if worker_id.0 != self.id.0 {
            return Err(RoleError::ContractViolation(format!(
                "Task assigned to {} but Worker is {}",
                worker_id.0, self.id.0
            )));
        }

        let repo_path = Path::new(".");
        let worktree = self.create_worktree(repo_path, &task_id).await?;

        let task_started = SemanticEvent::new_task_started(
            EventRoleId(self.id.0.clone()),
            &task_id,
            EventRoleId(self.id.0.clone()),
        );
        ctx.bus.publish(task_started).map_err(|e| {
            RoleError::Internal(format!("Failed to publish task started event: {e:?}"))
        })?;

        let patch = self
            .run_implementation_loop(&ctx, &task_id, &contract_ref.description, &worktree)
            .await?;

        let validation_commands = self.validation_commands.clone();
        let (validation_passed, validation_event_ids) = self
            .run_validation(&ctx, &worktree, validation_commands)
            .await?;

        let tool_event = SemanticEvent::new_tool_executed(
            EventRoleId(self.id.0.clone()),
            "implementation_loop",
            &contract_ref.description,
            if validation_passed { 0 } else { 1 },
            &patch,
            "",
            0,
        );
        ctx.bus.publish(tool_event.clone()).map_err(|e| {
            RoleError::Internal(format!("Failed to publish tool executed event: {e:?}"))
        })?;

        let all_tool_ids: Vec<EventId> = std::iter::once(tool_event.event_id())
            .chain(validation_event_ids.iter().copied())
            .collect();

        let claim_event = SemanticEvent::new_claim_made(
            EventRoleId(self.id.0.clone()),
            format!("Task completed: {}", contract_ref.description),
            Self::build_evidence_refs(&all_tool_ids, "implementation and validation results"),
            if validation_passed { 0.8 } else { 0.3 },
        );
        ctx.bus.publish(claim_event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish claim made event: {e:?}"))
        })?;

        let artefact = self.publish_artefact(&ctx, &patch).await?;

        if validation_passed {
            let completed = SemanticEvent::new_task_completed(
                EventRoleId(self.id.0.clone()),
                &task_id,
                &contract_ref.contract_id,
                artefact,
            );
            ctx.bus.publish(completed).map_err(|e| {
                RoleError::Internal(format!("Failed to publish task completed event: {e:?}"))
            })?;
        } else {
            let failed = SemanticEvent::new_task_failed(
                EventRoleId(self.id.0.clone()),
                &task_id,
                "Validation failed",
            );
            ctx.bus.publish(failed).map_err(|e| {
                RoleError::Internal(format!("Failed to publish task failed event: {e:?}"))
            })?;
        }

        worktree
            .delete()
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to delete worktree: {e}")))?;

        ctx.coordinator
            .report_status(
                EventRoleId(self.id.0.clone()),
                RoleLifecycleState::Completed,
            )
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        info!("Worker completed");
        Ok(())
    }
}

impl Default for Worker {
    fn default() -> Self {
        Self::new()
    }
}
