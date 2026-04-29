use std::{convert::Infallible, env, fmt::Debug, path::Path, sync::Arc};

use futures::future::LocalBoxFuture;
use naaf_knowledge::{KnowledgeGroupStore, KnowledgeSearchTool};
use naaf_llm::{
    CompletionRequest, Executor, ExecutorConfig, HumanAnswer, HumanIO, HumanQuestion, LlmAgent,
    Message, OpenAiClient, OpenAiConfig, Tool, ToolRegistry,
};
use naaf_persistence_sqlite::{SqliteCheckpointer, SqliteKnowledgeGroupStore};
use naaf_qdrant::{OpenAiEmbedder, QdrantClient};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;
use tokio::process::Command;

use crate::{
    MmatError,
    deliver::{
        artifacts::{DeliveryArtifact, DeliveryArtifactError, DeliveryArtifactStore},
        models::{
            CommandEvidence, DeliveryOutcome, EvidenceLog, ExecutionPlan, FinalReview,
            FinalReviewInput, ImplementationDelta, ImplementationDraft, ImplementationItemResult,
            ImplementationTaskInput, StageFinding, StageReview, TaskCard,
        },
        queue::{BuildJob, BuildJobId},
    },
    plan::{DesignHandoff, KnowledgeRuntimeConfig, parser::decode_outcome},
    project::ProjectConfig,
};

const DEFAULT_LLM_BASE_URL: &str = "http://127.0.0.1:1234/v1";
const DEFAULT_MODEL: &str = "qwen/qwen3.6-27b";
const EXECUTOR_TURNS: usize = 12;
const IMPLEMENTATION_RETRY_LIMIT: usize = 3;
const MAX_REMEDIATION_PASSES: usize = 3;
type DeliveryAgent = LlmAgent<OpenAiClient<DeliveryRuntime>, DeliveryRuntime, DeliveryError>;

#[derive(Debug, Error)]
pub enum DeliveryError {
    #[error("delivery configuration error: {0}")]
    Config(String),
    #[error("delivery workflow failed: {0}")]
    Workflow(String),
    #[error("delivery workspace failed: {0}")]
    Workspace(String),
    #[error("delivery command failed: {0}")]
    Command(String),
    #[error("delivery model output failed: {0}")]
    Model(String),
    #[error("delivery JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("delivery IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Artefact(#[from] DeliveryArtifactError),
}

#[derive(Clone)]
pub struct BuildEngine {
    project: ProjectConfig,
    events: Arc<dyn BuildEventSink>,
}

pub trait BuildEventSink: Send + Sync + 'static {
    fn log(&self, project: &ProjectConfig, level: tracing::Level, message: String);
}

#[derive(Clone, Debug)]
struct NoopEventSink;

#[derive(Clone, Debug)]
struct DeliveryRuntime {
    artefacts: DeliveryArtifactStore,
}

struct ToolAdapter<T> {
    inner: T,
}

impl BuildEngine {
    pub fn new(project: ProjectConfig) -> Self {
        Self {
            project,
            events: Arc::new(NoopEventSink),
        }
    }

    pub fn with_event_sink(project: ProjectConfig, events: Arc<dyn BuildEventSink>) -> Self {
        Self { project, events }
    }

    pub async fn run(&self, job: &BuildJob) -> Result<(), DeliveryError> {
        self.log(
            tracing::Level::INFO,
            format!("Delivery job {} started.", job.id),
        );

        let runtime = DeliveryRuntime::new(self.project.clone())?;
        let checkpoint_path = self.project.data_dir.join("delivery-checkpoints.sqlite3");
        if let Some(parent) = checkpoint_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _checkpointer = SqliteCheckpointer::open(&checkpoint_path.to_string_lossy())
            .map_err(|error| DeliveryError::Workflow(error.to_string()))?;

        let mut plan = self.plan_implementation(&runtime, job).await?;
        runtime.write_json(&job.id, DeliveryArtifact::ExecutionPlan, &plan)?;
        for task_card in &plan.task_cards {
            runtime.write_key_json(&job.id, &format!("task-card/{}", task_card.id), task_card)?;
        }

        let mut completed_items = Vec::new();
        let mut remediation_pass = 0usize;
        loop {
            let current_items = self
                .execute_task_cards(&runtime, job, &plan, completed_items.clone())
                .await?;
            completed_items = current_items;
            let evidence = EvidenceLog {
                task_results: completed_items.clone(),
            };
            runtime.write_json(&job.id, DeliveryArtifact::EvidenceLog, &evidence)?;

            let review = self
                .final_review(&runtime, &job.handoff, &plan, &completed_items)
                .await?;
            runtime.write_json(&job.id, DeliveryArtifact::FinalReview, &review)?;

            if review.ready {
                let outcome = DeliveryOutcome {
                    status: "succeeded".to_string(),
                    plan,
                    completed_items,
                    final_review: review.clone(),
                    next_step: review.next_step,
                };
                runtime.write_json(&job.id, DeliveryArtifact::Outcome, &outcome)?;
                self.log(
                    tracing::Level::INFO,
                    format!("Delivery job {} completed.", job.id),
                );
                return Ok(());
            }

            if review.remediation_items.is_empty() || remediation_pass + 1 >= MAX_REMEDIATION_PASSES
            {
                let outcome = DeliveryOutcome {
                    status: "failed".to_string(),
                    plan,
                    completed_items,
                    final_review: review.clone(),
                    next_step: review.next_step.clone(),
                };
                runtime.write_json(&job.id, DeliveryArtifact::Outcome, &outcome)?;
                return Err(DeliveryError::Workflow(format!(
                    "final review did not accept the delivery: {}",
                    review.summary
                )));
            }

            remediation_pass += 1;
            self.log(
                tracing::Level::WARN,
                format!("Final review requested remediation pass {remediation_pass}."),
            );
            plan = remediation_plan(&plan, &review);
            runtime.write_json(&job.id, DeliveryArtifact::ExecutionPlan, &plan)?;
        }
    }

    async fn plan_implementation(
        &self,
        runtime: &DeliveryRuntime,
        job: &BuildJob,
    ) -> Result<ExecutionPlan, DeliveryError> {
        self.log(tracing::Level::INFO, "Planning delivery tasks.".to_string());
        let agent = build_agent(
            self.project.root.as_path(),
            &self.project,
            &job.handoff.knowledge_collections,
        )
        .await?;
        let request = CompletionRequest::new(
            delivery_model(),
            vec![
                Message::system(planning_system_prompt()),
                Message::user(to_pretty_json(&job.handoff)?),
            ],
        );
        execute_json_stage(&agent, runtime, request, "implementation planning").await
    }

    async fn execute_task_cards(
        &self,
        runtime: &DeliveryRuntime,
        job: &BuildJob,
        plan: &ExecutionPlan,
        completed_items: Vec<ImplementationItemResult>,
    ) -> Result<Vec<ImplementationItemResult>, DeliveryError> {
        let mut completed = completed_items;
        for task_card in ordered_task_cards(&plan.task_cards)? {
            if completed.iter().any(|item| item.item_id == task_card.id) {
                continue;
            }

            self.log(
                tracing::Level::INFO,
                format!("Implementing task {}: {}", task_card.id, task_card.title),
            );
            let result = self
                .execute_task_card(runtime, job, plan, task_card, completed.clone())
                .await?;
            runtime.write_key_json(&job.id, &format!("task-result/{}", result.item_id), &result)?;
            completed.push(result);
        }
        Ok(completed)
    }

    async fn execute_task_card(
        &self,
        runtime: &DeliveryRuntime,
        job: &BuildJob,
        plan: &ExecutionPlan,
        task_card: TaskCard,
        completed_items: Vec<ImplementationItemResult>,
    ) -> Result<ImplementationItemResult, DeliveryError> {
        let mut prior_feedback = Vec::new();
        let worktree_name = sanitise_worktree_name(&format!("{}-{}", job.id, task_card.id));

        for attempt in 0..IMPLEMENTATION_RETRY_LIMIT {
            let baseline_root = naaf_workspace::create_baseline_snapshot(
                &self.project.root,
                &format!("baseline-{worktree_name}-{attempt}"),
            )
            .map_err(|error| DeliveryError::Workspace(error.to_string()))?;
            let worktree_root =
                naaf_workspace::prepare_worktree(&self.project.root, &worktree_name)
                    .await
                    .map_err(|error| DeliveryError::Workspace(error.to_string()))?;

            let input = ImplementationTaskInput {
                handoff: job.handoff.clone(),
                plan: plan.clone(),
                work_item: task_card.clone(),
                completed_items: completed_items.clone(),
                prior_feedback: prior_feedback.clone(),
            };
            let draft = self
                .implementation_draft(runtime, &worktree_root, input, worktree_name.clone())
                .await?;
            apply_file_deltas(&worktree_root, &draft.delta)?;

            let mut commands_run = Vec::new();
            commands_run.push(
                run_cargo_command(&worktree_root, "cargo fmt --all", &["fmt", "--all"]).await?,
            );
            commands_run.push(run_cargo_command(&worktree_root, "cargo check", &["check"]).await?);
            commands_run.push(run_cargo_command(&worktree_root, "cargo test", &["test"]).await?);
            commands_run.push(
                run_cargo_command(
                    &worktree_root,
                    "cargo clippy -- -D warnings",
                    &["clippy", "--", "-D", "warnings"],
                )
                .await?,
            );

            let findings = self.validate_draft(runtime, &worktree_root, &draft).await?;
            if !findings.is_empty() {
                prior_feedback.extend(findings);
                let _ = naaf_workspace::remove_worktree(&self.project.root, &worktree_name).await;
                let _ = naaf_workspace::remove_directory_if_exists(&baseline_root);
                continue;
            }

            let workspace_delta = naaf_workspace::FileDeltaSet {
                summary: draft.delta.summary.clone(),
                rationale: draft.delta.rationale.clone(),
                changes: draft
                    .delta
                    .changes
                    .iter()
                    .map(|change| naaf_workspace::FileDelta {
                        path: change.path.clone(),
                        action: change.action.clone(),
                        content: change.content.clone(),
                    })
                    .collect(),
            };
            let merged_changes = naaf_workspace::merge_item_worktree(
                &self.project.root,
                &baseline_root,
                &worktree_name,
                &workspace_delta,
            )
            .await
            .map_err(|error| DeliveryError::Workspace(error.to_string()))?;
            naaf_workspace::remove_directory_if_exists(&baseline_root)
                .map_err(|error| DeliveryError::Workspace(error.to_string()))?;

            return Ok(ImplementationItemResult {
                item_id: task_card.id,
                source: task_card.source,
                milestone_id: task_card.milestone_id,
                title: task_card.title,
                objective: task_card.objective,
                summary: draft.delta.summary,
                contract_refs: task_card.contract_refs,
                changed_files: merged_changes
                    .into_iter()
                    .map(|change| change.path)
                    .collect(),
                rationale: draft.delta.rationale,
                commands_run,
                reviewer_findings: Vec::new(),
                manual_checks: task_card.acceptance_criteria,
                known_gaps: Vec::new(),
                scope_deviation: None,
                worktree_name,
            });
        }

        Err(DeliveryError::Workflow(format!(
            "task {} failed validation after {IMPLEMENTATION_RETRY_LIMIT} attempt(s): {}",
            task_card.id,
            prior_feedback
                .iter()
                .map(|finding| finding.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )))
    }

    async fn implementation_draft(
        &self,
        runtime: &DeliveryRuntime,
        worktree_root: &Path,
        input: ImplementationTaskInput,
        worktree_name: String,
    ) -> Result<ImplementationDraft, DeliveryError> {
        let agent = build_agent(
            worktree_root,
            &self.project,
            &input.handoff.knowledge_collections,
        )
        .await?;
        let request = CompletionRequest::new(
            delivery_model(),
            vec![
                Message::system(implementation_system_prompt()),
                Message::user(to_pretty_json(&input)?),
            ],
        );
        let delta = execute_json_stage::<ImplementationDelta>(
            &agent,
            runtime,
            request,
            "implementation task",
        )
        .await?;
        Ok(ImplementationDraft {
            input,
            worktree_name,
            delta,
        })
    }

    async fn validate_draft(
        &self,
        runtime: &DeliveryRuntime,
        worktree_root: &Path,
        draft: &ImplementationDraft,
    ) -> Result<Vec<StageFinding>, DeliveryError> {
        let agent = build_agent(
            worktree_root,
            &self.project,
            &draft.input.handoff.knowledge_collections,
        )
        .await?;
        let mut findings = Vec::new();
        for (stage, system_prompt) in [
            ("peer review", peer_review_system_prompt()),
            ("contract validation", contract_validation_system_prompt()),
        ] {
            let request = CompletionRequest::new(
                delivery_model(),
                vec![
                    Message::system(system_prompt),
                    Message::user(to_pretty_json(draft)?),
                ],
            );
            let review = execute_json_stage::<StageReview>(&agent, runtime, request, stage).await?;
            findings.extend(review.findings);
        }
        Ok(findings)
    }

    async fn final_review(
        &self,
        runtime: &DeliveryRuntime,
        handoff: &DesignHandoff,
        plan: &ExecutionPlan,
        completed_items: &[ImplementationItemResult],
    ) -> Result<FinalReview, DeliveryError> {
        self.log(
            tracing::Level::INFO,
            "Running final delivery review.".to_string(),
        );
        let agent = build_agent(
            self.project.root.as_path(),
            &self.project,
            &handoff.knowledge_collections,
        )
        .await?;
        let input = FinalReviewInput {
            handoff: handoff.clone(),
            plan: plan.clone(),
            completed_items: completed_items.to_vec(),
        };
        let request = CompletionRequest::new(
            delivery_model(),
            vec![
                Message::system(final_review_system_prompt()),
                Message::user(to_pretty_json(&input)?),
            ],
        );
        execute_json_stage(&agent, runtime, request, "final review").await
    }

    fn log(&self, level: tracing::Level, message: String) {
        self.events.log(&self.project, level, message);
    }
}

impl NoopEventSink {
    fn log_inner(&self, _project: &ProjectConfig, _level: tracing::Level, _message: String) {}
}

impl BuildEventSink for NoopEventSink {
    fn log(&self, project: &ProjectConfig, level: tracing::Level, message: String) {
        self.log_inner(project, level, message);
    }
}

impl DeliveryRuntime {
    fn new(project: ProjectConfig) -> Result<Self, DeliveryError> {
        let artefacts =
            DeliveryArtifactStore::open(project.data_dir.join("delivery-artifacts.sqlite3"))?;
        Ok(Self { artefacts })
    }

    fn write_json<T>(
        &self,
        job_id: &BuildJobId,
        artifact: DeliveryArtifact,
        value: &T,
    ) -> Result<(), DeliveryError>
    where
        T: Serialize + ?Sized,
    {
        self.artefacts.write_json(job_id, artifact, value)?;
        Ok(())
    }

    fn write_key_json<T>(
        &self,
        job_id: &BuildJobId,
        key: &str,
        value: &T,
    ) -> Result<(), DeliveryError>
    where
        T: Serialize + ?Sized,
    {
        self.artefacts.write_key_json(job_id, key, value)?;
        Ok(())
    }
}

impl HumanIO for DeliveryRuntime {
    type Error = DeliveryError;

    fn ask<'a>(
        &'a self,
        question: HumanQuestion,
    ) -> LocalBoxFuture<'a, Result<HumanAnswer, Self::Error>> {
        Box::pin(async move {
            Err(DeliveryError::Workflow(format!(
                "delivery jobs are non-interactive; model requested input: {}",
                question.question
            )))
        })
    }
}

impl<T> ToolAdapter<T> {
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T> Tool for ToolAdapter<T>
where
    T: Tool<Runtime = DeliveryRuntime>,
    T::Error: Debug,
{
    type Runtime = DeliveryRuntime;
    type Error = DeliveryError;

    fn spec(&self) -> naaf_llm::ToolSpec {
        self.inner.spec()
    }

    fn call<'a>(
        &'a self,
        runtime: &'a Self::Runtime,
        arguments: serde_json::Value,
    ) -> LocalBoxFuture<'a, Result<serde_json::Value, Self::Error>> {
        Box::pin(async move {
            self.inner
                .call(runtime, arguments)
                .await
                .map_err(|error| DeliveryError::Workflow(format!("tool failed: {error:?}")))
        })
    }
}

async fn build_agent(
    workspace_root: &Path,
    project: &ProjectConfig,
    knowledge_collections: &[String],
) -> Result<DeliveryAgent, DeliveryError> {
    let mut tools = ToolRegistry::<DeliveryRuntime, DeliveryError>::new();
    tools = register_tool(
        tools,
        ToolAdapter::new(naaf_llm::repository::ReadFileTool::<DeliveryRuntime>::new(
            workspace_root.to_path_buf(),
        )),
    )?;
    tools = register_tool(
        tools,
        ToolAdapter::new(naaf_llm::repository::GlobPathsTool::<DeliveryRuntime>::new(
            workspace_root.to_path_buf(),
        )),
    )?;
    tools = register_tool(
        tools,
        ToolAdapter::new(
            naaf_llm::repository::SearchFilesTool::<DeliveryRuntime>::new(
                workspace_root.to_path_buf(),
            ),
        ),
    )?;

    if !knowledge_collections.is_empty()
        && let Some(tool) = build_knowledge_search_tool(project, knowledge_collections).await?
    {
        tools = register_tool(tools, ToolAdapter::new(tool))?;
    }

    let client = OpenAiClient::new(workflow_llm_config());
    let executor =
        Executor::with_tools(client, tools).with_config(ExecutorConfig::new(EXECUTOR_TURNS));
    Ok(LlmAgent::with_executor(executor))
}

async fn build_knowledge_search_tool(
    project: &ProjectConfig,
    collections: &[String],
) -> Result<Option<KnowledgeSearchTool<DeliveryRuntime>>, DeliveryError> {
    let config = KnowledgeRuntimeConfig::from_project(project)?;
    let store = SqliteKnowledgeGroupStore::open(&config.sqlite_path.to_string_lossy())
        .map_err(|error| DeliveryError::Workflow(error.to_string()))?;
    let embedder = OpenAiEmbedder::with_model(
        config.embedding_api_key,
        config.embedding_model,
        config.embedding_dimension,
    )
    .with_base_url(config.embedding_base_url);
    let mut tool = KnowledgeSearchTool::new(Box::new(embedder), 5, 0.7);
    if let Some(repo) = config.repo {
        tool = tool.with_repo(repo);
    }

    let mut loaded = 0usize;
    for collection in collections {
        let Some(group) = store
            .load_group(collection)
            .await
            .map_err(|error| DeliveryError::Workflow(error.to_string()))?
        else {
            continue;
        };
        let client = QdrantClient::from_url(&config.qdrant_url, config.qdrant_api_key.clone())
            .map_err(|error| DeliveryError::Workflow(error.to_string()))?
            .with_collection(collection);
        tool = tool.with_group(group, client);
        loaded += 1;
    }

    Ok((loaded > 0).then_some(tool))
}

fn register_tool<T>(
    tools: ToolRegistry<DeliveryRuntime, DeliveryError>,
    tool: T,
) -> Result<ToolRegistry<DeliveryRuntime, DeliveryError>, DeliveryError>
where
    T: Tool<Runtime = DeliveryRuntime, Error = DeliveryError> + 'static,
{
    tools
        .with_tool(tool)
        .map_err(|error| DeliveryError::Config(error.to_string()))
}

async fn execute_json_stage<T>(
    agent: &DeliveryAgent,
    runtime: &DeliveryRuntime,
    request: CompletionRequest,
    stage: &str,
) -> Result<T, DeliveryError>
where
    T: DeserializeOwned,
{
    let outcome = agent
        .executor()
        .execute(runtime, request)
        .await
        .map_err(|error| DeliveryError::Model(format!("{stage} failed: {error}")))?;
    decode_outcome(outcome).map_err(DeliveryError::from)
}

fn apply_file_deltas(root: &Path, delta: &ImplementationDelta) -> Result<(), DeliveryError> {
    let delta = naaf_workspace::FileDeltaSet {
        summary: delta.summary.clone(),
        rationale: delta.rationale.clone(),
        changes: delta
            .changes
            .iter()
            .map(|change| naaf_workspace::FileDelta {
                path: change.path.clone(),
                action: change.action.clone(),
                content: change.content.clone(),
            })
            .collect(),
    };
    naaf_workspace::apply_file_deltas(root, &delta)
        .map_err(|error| DeliveryError::Workspace(error.to_string()))
}

async fn run_cargo_command(
    root: &Path,
    label: &str,
    args: &[&str],
) -> Result<CommandEvidence, DeliveryError> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(root)
        .output()
        .await
        .map_err(|error| DeliveryError::Command(format!("{label} failed to start: {error}")))?;

    if output.status.success() {
        return Ok(CommandEvidence {
            command: label.to_string(),
            outcome: "passed".to_string(),
        });
    }

    Err(DeliveryError::Command(format!(
        "{label} failed: {}",
        naaf_workspace::command_failure_summary(&output.stdout, &output.stderr)
    )))
}

fn ordered_task_cards(task_cards: &[TaskCard]) -> Result<Vec<TaskCard>, DeliveryError> {
    let mut ordered = Vec::new();
    let mut remaining = task_cards.to_vec();
    while !remaining.is_empty() {
        let completed_ids = ordered
            .iter()
            .map(|item: &TaskCard| item.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let Some(index) = remaining.iter().position(|item| {
            item.dependencies
                .iter()
                .all(|dependency| completed_ids.contains(dependency.as_str()))
        }) else {
            return Err(DeliveryError::Workflow(
                "task card dependencies contain a cycle or missing dependency".to_string(),
            ));
        };
        ordered.push(remaining.remove(index));
    }
    Ok(ordered)
}

fn remediation_plan(plan: &ExecutionPlan, review: &FinalReview) -> ExecutionPlan {
    let task_cards = review
        .remediation_items
        .iter()
        .map(|item| TaskCard {
            id: item.id.clone(),
            source: "final_review".to_string(),
            milestone_id: None,
            title: item.title.clone(),
            objective: item.description.clone(),
            contract_refs: item.related_item_ids.clone(),
            acceptance_criteria: item.acceptance_criteria.clone(),
            expected_files: Vec::new(),
            verification_commands: vec![
                "cargo fmt --all".to_string(),
                "cargo check".to_string(),
                "cargo test".to_string(),
                "cargo clippy -- -D warnings".to_string(),
            ],
            dependencies: Vec::new(),
            rollback_notes: vec!["Revert the remediation changes.".to_string()],
        })
        .collect();
    ExecutionPlan {
        summary: format!("Remediation for final review: {}", review.summary),
        milestones: Vec::new(),
        task_cards,
        risks: plan.risks.clone(),
    }
}

fn sanitise_worktree_name(name: &str) -> String {
    let mut output = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "delivery-job".to_string()
    } else {
        output
    }
}

fn to_pretty_json<T>(value: &T) -> Result<String, DeliveryError>
where
    T: Serialize + ?Sized,
{
    Ok(serde_json::to_string_pretty(value)?)
}

fn delivery_model() -> String {
    env::var("MMAT_DELIVERY_MODEL")
        .or_else(|_| env::var("MMAT_LLM_MODEL"))
        .unwrap_or_else(|_| DEFAULT_MODEL.to_string())
}

fn workflow_llm_config() -> OpenAiConfig {
    let api_key = read_env("MMAT_DELIVERY_LLM_API_KEY")
        .or_else(|| read_env("MMAT_LLM_API_KEY"))
        .or_else(|| read_env("OPENAI_API_KEY"))
        .unwrap_or_default();
    let base_url = read_env("MMAT_DELIVERY_LLM_BASE_URL")
        .or_else(|| read_env("MMAT_LLM_BASE_URL"))
        .unwrap_or_else(|| DEFAULT_LLM_BASE_URL.to_string());

    OpenAiConfig::new(api_key).with_base_url(base_url)
}

fn read_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn planning_system_prompt() -> String {
    "You are MMAT's non-interactive delivery planning stage. Convert the approved software architect handoff into concrete execution milestones and task cards. Do not ask the user questions. If ambiguity remains, record explicit assumptions inside task acceptance criteria or risks. Return raw JSON only with this shape: {\"summary\":string,\"milestones\":[{\"id\":string,\"title\":string,\"objective\":string,\"task_card_ids\":string[]}],\"task_cards\":[{\"id\":string,\"source\":string,\"milestone_id\":string|null,\"title\":string,\"objective\":string,\"contract_refs\":string[],\"acceptance_criteria\":string[],\"expected_files\":string[],\"verification_commands\":string[],\"dependencies\":string[],\"rollback_notes\":string[]}],\"risks\":string[]}."
        .to_string()
}

fn implementation_system_prompt() -> String {
    "You are MMAT's non-interactive delivery implementation stage. Inspect the repository using tools before proposing changes. Implement exactly the requested task card and return complete file deltas only. Do not ask the user questions. Return raw JSON only with this shape: {\"summary\":string,\"rationale\":string[],\"changes\":[{\"path\":string,\"action\":string,\"content\":string|null}]}. Allowed actions are `write` and `delete`; write actions must include complete file contents."
        .to_string()
}

fn peer_review_system_prompt() -> String {
    "You are MMAT's delivery peer reviewer. Review the proposed implementation delta for correctness, code quality, maintainability, and fit with the existing project. Return raw JSON only with this shape: {\"summary\":string,\"findings\":[{\"severity\":string,\"category\":string,\"message\":string}]}. Use an empty findings array only when the work is ready to merge."
        .to_string()
}

fn contract_validation_system_prompt() -> String {
    "You are MMAT's delivery contract validator. Check whether the implementation satisfies the task card, approved architect handoff, and acceptance criteria. Flag stubs, TODO placeholders, fake values, missing behaviour, or unproven claims. Return raw JSON only with this shape: {\"summary\":string,\"findings\":[{\"severity\":string,\"category\":string,\"message\":string}]}. Use an empty findings array only when the work genuinely satisfies the contract."
        .to_string()
}

fn final_review_system_prompt() -> String {
    "You are MMAT's final delivery reviewer. Assess the completed delivery against the approved architect handoff, execution plan, task results, evidence, and repository state. Be strict and non-interactive. Return raw JSON only with this shape: {\"summary\":string,\"ready\":boolean,\"strengths\":string[],\"findings\":[{\"severity\":string,\"category\":string,\"message\":string}],\"remediation_items\":[{\"id\":string,\"title\":string,\"description\":string,\"acceptance_criteria\":string[],\"related_item_ids\":string[]}],\"next_step\":string}. When ready is true, remediation_items must be empty."
        .to_string()
}

impl From<MmatError> for DeliveryError {
    fn from(value: MmatError) -> Self {
        Self::Config(value.to_string())
    }
}

impl From<Infallible> for DeliveryError {
    fn from(value: Infallible) -> Self {
        match value {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deliver::models::RemediationItem;

    #[test]
    fn orders_task_cards_by_dependencies() {
        let first = TaskCard {
            id: "first".to_string(),
            source: "plan".to_string(),
            milestone_id: None,
            title: "First".to_string(),
            objective: "First".to_string(),
            contract_refs: Vec::new(),
            acceptance_criteria: Vec::new(),
            expected_files: Vec::new(),
            verification_commands: Vec::new(),
            dependencies: Vec::new(),
            rollback_notes: Vec::new(),
        };
        let second = TaskCard {
            id: "second".to_string(),
            dependencies: vec!["first".to_string()],
            ..first.clone()
        };

        let ordered =
            ordered_task_cards(&[second.clone(), first.clone()]).expect("tasks should order");

        assert_eq!(ordered, vec![first, second]);
    }

    #[test]
    fn remediation_plan_maps_review_items_to_task_cards() {
        let plan = ExecutionPlan {
            summary: "plan".to_string(),
            milestones: Vec::new(),
            task_cards: Vec::new(),
            risks: vec!["risk".to_string()],
        };
        let review = FinalReview {
            summary: "needs work".to_string(),
            ready: false,
            strengths: Vec::new(),
            findings: Vec::new(),
            remediation_items: vec![RemediationItem {
                id: "rem-1".to_string(),
                title: "Fix issue".to_string(),
                description: "Patch the bug".to_string(),
                acceptance_criteria: vec!["passes".to_string()],
                related_item_ids: vec!["task-1".to_string()],
            }],
            next_step: "remediate".to_string(),
        };

        let remediation = remediation_plan(&plan, &review);

        assert_eq!(remediation.task_cards[0].id, "rem-1");
        assert_eq!(remediation.task_cards[0].source, "final_review");
        assert_eq!(remediation.risks, vec!["risk"]);
    }
}
