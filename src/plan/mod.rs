use std::{
    convert::Infallible,
    env,
    fmt::{Debug, Display},
    fs,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use naaf_core::{Step, StepReport, task_fn};
use naaf_llm::{
    CompletionRequest, ExecutionOutcome, Executor, ExecutorError, HumanIO, LlmAgent, LlmClient,
    OpenAiClient, OpenAiConfig, OpenAiStreamObserver, TaskError,
};
use naaf_persistence_sqlite::SqliteKnowledgeGroupStore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{MmatError, project::ProjectConfig};

mod architect;
mod discovery;
mod knowledge;
pub mod parser;
mod solutions;

type WorkflowStep<C, R, E, I, O> = Step<R, I, O, WorkflowFinding, WorkflowTaskError<C, R, E>>;
type WorkflowTaskError<C, R, E> = TaskError<
    WorkflowBuildError<<R as HumanIO>::Error>,
    <C as LlmClient>::Error,
    E,
    serde_json::Error,
>;

#[cfg(not(test))]
const EXECUTOR_RETRY_BASE_DELAY_MS: u64 = 1000;
#[cfg(test)]
const EXECUTOR_RETRY_BASE_DELAY_MS: u64 = 1;
const DEFAULT_EMBEDDING_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_EMBEDDING_DIMENSION: usize = 1536;
const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";
const DEFAULT_LLM_BASE_URL: &str = "http://127.0.0.1:1234/v1";
const DEFAULT_QDRANT_URL: &str = "http://127.0.0.1:6333";
const MAX_EXECUTOR_RETRIES: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnowledgeRuntimeConfig {
    pub sqlite_path: PathBuf,
    pub qdrant_url: String,
    pub qdrant_api_key: Option<String>,
    pub embedding_api_key: String,
    pub embedding_base_url: String,
    pub embedding_model: String,
    pub embedding_dimension: usize,
    pub repo: Option<String>,
    pub workspace_root: PathBuf,
    pub qdrant_collection_prefix: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignHandoff {
    pub design_run_id: uuid::Uuid,
    pub prompt: String,
    pub architect_plan: String,
    pub knowledge_collections: Vec<String>,
}

#[derive(Debug, Error)]
enum WorkflowBuildError<H> {
    #[error("human interaction failed: {0}")]
    Human(H),
    #[error(transparent)]
    Knowledge(#[from] knowledge::KnowledgeError),
    #[error("invalid solution choice: {0}")]
    InvalidChoice(String),
    #[error("plan step failed: {0}")]
    Workflow(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum WorkflowFinding {
    Discovery(discovery::DiscoveryFinding),
    Knowledge(knowledge::KnowledgeFinding),
    SolutionBranch(solutions::SolutionBranchFinding),
    SolutionCollect(solutions::SolutionCollectFinding),
    Architect(architect::ArchitectFinding),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(super) enum WorkflowStageId {
    Discovery,
    KnowledgePlanning,
    KnowledgeMaterialisation,
    Solutions,
    SolutionSelection,
    SoftwareArchitect,
    ImplementationPlanning,
    Execution,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum WorkflowRunResult {
    ReadyForPlanning {
        architect_plan: architect::ArchitectPlan,
        knowledge_collections: Vec<String>,
    },
    NeedsRevision {
        feedback: String,
    },
}

pub struct GreenfieldReport {
    run_id: uuid::Uuid,
    result: WorkflowRunResult,
    step_report: StepReport<WorkflowFinding>,
    design_handoff: Option<DesignHandoff>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct KnowledgeMaterialisationInput {
    discovery: discovery::DiscoveryOutput,
    plan: knowledge::KnowledgePlan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct KnowledgeMaterialisationOutput {
    discovery: discovery::DiscoveryOutput,
    materialised: Vec<knowledge::MaterialisedKnowledgeGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SolutionCollectionContext {
    discovery: discovery::DiscoveryOutput,
    materialised: Vec<knowledge::MaterialisedKnowledgeGroup>,
    collection: solutions::SolutionCollection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SolutionChoiceContext {
    discovery: discovery::DiscoveryOutput,
    materialised: Vec<knowledge::MaterialisedKnowledgeGroup>,
    choice: solutions::SolutionUserChoice,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ArchitectStageInput {
    discovery: discovery::DiscoveryOutput,
    selected_solution: solutions::SelectedSolution,
    materialised: Vec<knowledge::MaterialisedKnowledgeGroup>,
}

impl KnowledgeRuntimeConfig {
    pub fn from_env() -> Result<Self, MmatError> {
        let workspace_root = read_env("MMAT_PROJECT_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| env::current_dir().expect("failed to resolve current workspace"));
        let sqlite_path = read_env("MMAT_KNOWLEDGE_SQLITE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                read_env("MMAT_DATA_PATH")
                    .map(|base| PathBuf::from(base).join("knowledge.sqlite3"))
                    .unwrap_or_else(|| workspace_root.join(".mmat").join("knowledge.sqlite3"))
            });
        let embedding_dimension = match read_env("MMAT_EMBEDDING_DIMENSION") {
            Some(value) => value.parse::<usize>().map_err(|error| {
                MmatError::Config(format!(
                    "invalid MMAT_EMBEDDING_DIMENSION `{value}`: {error}"
                ))
            })?,
            None => DEFAULT_EMBEDDING_DIMENSION,
        };

        Ok(Self {
            sqlite_path,
            qdrant_url: read_env("MMAT_QDRANT_URL")
                .unwrap_or_else(|| DEFAULT_QDRANT_URL.to_string()),
            qdrant_api_key: read_env("MMAT_QDRANT_API_KEY"),
            embedding_api_key: read_env("MMAT_EMBEDDING_API_KEY")
                .or_else(|| read_env("OPENAI_API_KEY"))
                .unwrap_or_default(),
            embedding_base_url: read_env("MMAT_EMBEDDING_BASE_URL")
                .unwrap_or_else(|| DEFAULT_EMBEDDING_BASE_URL.to_string()),
            embedding_model: read_env("MMAT_EMBEDDING_MODEL")
                .unwrap_or_else(|| DEFAULT_EMBEDDING_MODEL.to_string()),
            embedding_dimension,
            repo: read_env("MMAT_KNOWLEDGE_REPO"),
            workspace_root,
            qdrant_collection_prefix: read_env("MMAT_QDRANT_COLLECTION_PREFIX")
                .unwrap_or_else(|| "p_default".to_string()),
        })
    }

    pub fn from_project(project: &ProjectConfig) -> Result<Self, MmatError> {
        let mut config = Self::from_env()?;
        config.sqlite_path = project.data_dir.join("knowledge.sqlite3");
        config.workspace_root = project.root.clone();
        config.repo = project.repo_label.clone();
        config.qdrant_collection_prefix = project.qdrant_collection_prefix.clone();
        Ok(config)
    }

    fn open_store(&self) -> Result<SqliteKnowledgeGroupStore, MmatError> {
        if let Some(parent) = self.sqlite_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                MmatError::Config(format!(
                    "failed to create SQLite knowledge directory `{}`: {error}",
                    parent.display()
                ))
            })?;
        }

        let path = self.sqlite_path.to_string_lossy().into_owned();
        SqliteKnowledgeGroupStore::open(&path)
            .map_err(|error| MmatError::Workflow(error.to_string()))
    }

    fn qdrant_backend<R>(&self) -> knowledge::QdrantKnowledgeBackend<R> {
        knowledge::QdrantKnowledgeBackend::new(knowledge::QdrantKnowledgeBackendConfig {
            url: self.qdrant_url.clone(),
            api_key: self.qdrant_api_key.clone(),
            embedding_api_key: self.embedding_api_key.clone(),
            embedding_base_url: self.embedding_base_url.clone(),
            embedding_model: self.embedding_model.clone(),
            embedding_dimension: self.embedding_dimension,
            repo: self.repo.clone(),
            workspace_root: self.workspace_root.clone(),
        })
    }
}

impl From<discovery::DiscoveryFinding> for WorkflowFinding {
    fn from(finding: discovery::DiscoveryFinding) -> Self {
        Self::Discovery(finding)
    }
}

impl From<knowledge::KnowledgeFinding> for WorkflowFinding {
    fn from(finding: knowledge::KnowledgeFinding) -> Self {
        Self::Knowledge(finding)
    }
}

impl From<solutions::SolutionBranchFinding> for WorkflowFinding {
    fn from(finding: solutions::SolutionBranchFinding) -> Self {
        Self::SolutionBranch(finding)
    }
}

impl From<solutions::SolutionCollectFinding> for WorkflowFinding {
    fn from(finding: solutions::SolutionCollectFinding) -> Self {
        Self::SolutionCollect(finding)
    }
}

impl From<architect::ArchitectFinding> for WorkflowFinding {
    fn from(finding: architect::ArchitectFinding) -> Self {
        Self::Architect(finding)
    }
}

impl Display for WorkflowFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discovery(finding) => Display::fmt(finding, f),
            Self::Knowledge(finding) => Display::fmt(finding, f),
            Self::SolutionBranch(finding) => Display::fmt(finding, f),
            Self::SolutionCollect(finding) => Display::fmt(finding, f),
            Self::Architect(finding) => Display::fmt(finding, f),
        }
    }
}

impl WorkflowStageId {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::KnowledgePlanning => "knowledge-planning",
            Self::KnowledgeMaterialisation => "knowledge-materialisation",
            Self::Solutions => "solutions",
            Self::SolutionSelection => "solution-selection",
            Self::SoftwareArchitect => "software-architect",
            Self::ImplementationPlanning => "implementation-planning",
            Self::Execution => "execution",
        }
    }

    pub(super) fn default_system_prompt(self) -> &'static str {
        match self {
            Self::Discovery => "You are the discovery stage for MMAT.",
            Self::KnowledgePlanning => "You are the knowledge planning stage for MMAT.",
            Self::KnowledgeMaterialisation => {
                "You are the knowledge materialisation stage for MMAT."
            }
            Self::Solutions => "You are the solution generation stage for MMAT.",
            Self::SolutionSelection => "You are the solution selection stage for MMAT.",
            Self::SoftwareArchitect => "You are the downstream Software Architect stage for MMAT.",
            Self::ImplementationPlanning => "You are the implementation planning stage for MMAT.",
            Self::Execution => "You are the execution stage for MMAT.",
        }
    }
}

impl std::fmt::Display for WorkflowStageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl GreenfieldReport {
    pub fn run_id(&self) -> uuid::Uuid {
        self.run_id
    }

    pub fn attempt_count(&self) -> usize {
        self.step_report.attempt_count()
    }

    pub fn outcome_label(&self) -> &'static str {
        match &self.result {
            WorkflowRunResult::ReadyForPlanning { .. } => "ready-for-planning",
            WorkflowRunResult::NeedsRevision { .. } => "needs-revision",
        }
    }

    pub fn design_handoff(&self) -> Option<DesignHandoff> {
        self.design_handoff.clone()
    }
}

pub async fn greenfield<R>(
    init_prompt: String,
    runtime: R,
    stream_observer: Option<Arc<dyn OpenAiStreamObserver<R>>>,
) -> Result<GreenfieldReport, MmatError>
where
    R: HumanIO + Clone + 'static,
    R::Error: Debug + Display + 'static,
{
    let knowledge_config = KnowledgeRuntimeConfig::from_env()?;
    greenfield_with_knowledge_config(init_prompt, runtime, stream_observer, knowledge_config).await
}

pub async fn greenfield_for_project<R>(
    init_prompt: String,
    runtime: R,
    stream_observer: Option<Arc<dyn OpenAiStreamObserver<R>>>,
    project: &ProjectConfig,
) -> Result<GreenfieldReport, MmatError>
where
    R: HumanIO + Clone + 'static,
    R::Error: Debug + Display + 'static,
{
    let knowledge_config = KnowledgeRuntimeConfig::from_project(project)?;
    greenfield_with_knowledge_config(init_prompt, runtime, stream_observer, knowledge_config).await
}

pub async fn greenfield_with_knowledge_config<R>(
    init_prompt: String,
    runtime: R,
    stream_observer: Option<Arc<dyn OpenAiStreamObserver<R>>>,
    knowledge_config: KnowledgeRuntimeConfig,
) -> Result<GreenfieldReport, MmatError>
where
    R: HumanIO + Clone + 'static,
    R::Error: Debug + Display + 'static,
{
    let cfg = workflow_llm_config();
    let mut oai_client = OpenAiClient::<R>::new(cfg);
    if let Some(stream_observer) = stream_observer {
        oai_client = oai_client.with_stream_observer(stream_observer);
    }
    let agent = LlmAgent::new(oai_client);
    let knowledge_store = Arc::new(knowledge_config.open_store()?);
    let knowledge_backend = Arc::new(knowledge_config.qdrant_backend::<R>());
    let plan = build_greenfield_step::<OpenAiClient<R>, R, Infallible>(
        &agent,
        knowledge_store,
        knowledge_backend,
        knowledge_config.qdrant_collection_prefix.clone(),
        knowledge_config.workspace_root.clone(),
    );

    let traced = plan
        .run_traced(
            &runtime,
            discovery::DiscoveryInput::new(init_prompt.clone()),
        )
        .await
        .map_err(|error| MmatError::Workflow(error.to_string()))?;
    let (result, step_report) = traced.into_parts();
    let run_id = uuid::Uuid::new_v4();
    let design_handoff = match &result {
        WorkflowRunResult::ReadyForPlanning {
            architect_plan,
            knowledge_collections,
        } => Some(DesignHandoff {
            design_run_id: run_id,
            prompt: init_prompt,
            architect_plan: serde_json::to_string(architect_plan)?,
            knowledge_collections: knowledge_collections.clone(),
        }),
        WorkflowRunResult::NeedsRevision { .. } => None,
    };

    Ok(GreenfieldReport {
        run_id,
        result,
        step_report,
        design_handoff,
    })
}

pub(crate) async fn execute_with_turn_limit_retry<C, R, E>(
    executor: &Executor<C, R, E>,
    runtime: &R,
    request: CompletionRequest,
) -> Result<ExecutionOutcome, ExecutorError<C::Error, E>>
where
    C: LlmClient<Runtime = R>,
    C::Error: 'static,
    E: 'static,
{
    let mut delay = Duration::from_millis(EXECUTOR_RETRY_BASE_DELAY_MS);
    for attempt in 1..=MAX_EXECUTOR_RETRIES {
        match executor.execute(runtime, request.clone()).await {
            Ok(result) => return Ok(result),
            Err(ExecutorError::TurnLimitExceeded { max_turns }) => {
                if attempt == MAX_EXECUTOR_RETRIES {
                    return Err(ExecutorError::TurnLimitExceeded { max_turns });
                }
                tracing::warn!(
                    attempt,
                    max_attempts = MAX_EXECUTOR_RETRIES,
                    max_turns,
                    "executor hit turn limit, backing off and retrying"
                );
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

fn architect_input_from_stage(input: ArchitectStageInput) -> architect::ArchitectInput {
    let knowledge = knowledge::build_stage_knowledge_session(
        WorkflowStageId::SoftwareArchitect,
        WorkflowStageId::SoftwareArchitect.default_system_prompt(),
        &input.materialised,
    );
    let materialised = input
        .materialised
        .iter()
        .map(|g| g.as_knowledge_group())
        .collect();
    architect::ArchitectInput::new(
        input.discovery,
        input.selected_solution,
        knowledge,
        materialised,
    )
}

fn build_greenfield_step<C, R, E>(
    agent: &LlmAgent<C, R, E>,
    knowledge_store: Arc<SqliteKnowledgeGroupStore>,
    knowledge_backend: Arc<knowledge::QdrantKnowledgeBackend<R>>,
    knowledge_collection_prefix: String,
    workspace_root: PathBuf,
) -> WorkflowStep<C, R, E, discovery::DiscoveryInput, WorkflowRunResult>
where
    C: LlmClient<Runtime = R> + Clone + 'static,
    C::Error: Debug + Display + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + Display + 'static,
    E: Debug + Display + 'static,
{
    let discovery = discovery::step_with_repository_tools(agent, workspace_root.clone())
        .map_findings(WorkflowFinding::from);
    let knowledge = knowledge::step_with_lint(
        agent,
        knowledge_store.clone(),
        knowledge_collection_prefix.clone(),
        workspace_root.clone(),
    )
    .map_input(knowledge::KnowledgeInput::new)
    .map_findings(WorkflowFinding::from);
    let knowledge_context = knowledge
        .with_input()
        .map(|(discovery, plan)| KnowledgeMaterialisationInput { discovery, plan });

    let materialisation =
        knowledge::materialisation_step::<C, R, E, knowledge::QdrantKnowledgeBackend<R>>(
            knowledge_store.clone(),
            knowledge_backend.clone(),
            knowledge_collection_prefix.clone(),
        )
        .map_input(|input: KnowledgeMaterialisationInput| input.plan)
        .map_with_input(|input, materialised| KnowledgeMaterialisationOutput {
            discovery: input.discovery,
            materialised,
        })
        .map_findings(WorkflowFinding::from);

    let solution_branches = solution_branch_step::<C, R, E>(
        solutions::branch_step(agent, workspace_root.clone()),
        solutions::SolutionBranch::Conservative,
    )
    .join(solution_branch_step::<C, R, E>(
        solutions::branch_step(agent, workspace_root.clone()),
        solutions::SolutionBranch::Recommended,
    ))
    .join(solution_branch_step::<C, R, E>(
        solutions::branch_step(agent, workspace_root.clone()),
        solutions::SolutionBranch::Ambitious,
    ))
    .map(|((conservative, recommended), ambitious)| vec![conservative, recommended, ambitious]);
    let solution_collection = solution_branches
        .map_input(solution_input_from_materialisation)
        .then(
            solutions::collect_step(agent)
                .map_input(solutions::SolutionCollectInput::new)
                .map_findings(WorkflowFinding::from),
        );

    let collection_context = solution_collection
        .with_input()
        .map(|(context, collection)| SolutionCollectionContext {
            discovery: context.discovery,
            materialised: context.materialised,
            collection,
        });

    let choice_context = solutions::choice_step::<C, R, E>()
        .map_input(|context: SolutionCollectionContext| context.collection)
        .map_findings(|()| unreachable!("choice step has no findings"))
        .map_with_input(|context, choice| SolutionChoiceContext {
            discovery: context.discovery,
            materialised: context.materialised,
            choice,
        });

    let architect = architect::step_with_knowledge_tools(
        agent,
        knowledge_backend.clone(),
        workspace_root.clone(),
    )
    .map_input(architect_input_from_stage)
    .map_findings(WorkflowFinding::from);

    discovery
        .then(knowledge_context)
        .then(materialisation)
        .then(collection_context)
        .then(choice_context)
        .then(finalise_choice_step::<C, R, E>(architect))
}

fn finalise_choice_step<C, R, E>(
    architect: WorkflowStep<C, R, E, ArchitectStageInput, architect::ArchitectPlan>,
) -> WorkflowStep<C, R, E, SolutionChoiceContext, WorkflowRunResult>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + Display + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + Display + 'static,
    E: Debug + Display + 'static,
{
    Step::builder(task_fn(
        move |runtime: &R, context: SolutionChoiceContext| {
            let architect = architect.clone();
            Box::pin(async move {
                match context.choice {
                    solutions::SolutionUserChoice::Selected(selected_solution) => {
                        let plan = architect
                            .run(
                                runtime,
                                ArchitectStageInput {
                                    discovery: context.discovery,
                                    selected_solution,
                                    materialised: context.materialised.clone(),
                                },
                            )
                            .await
                            .map_err(|error| {
                                TaskError::Build(WorkflowBuildError::Workflow(error.to_string()))
                            })?;
                        let knowledge_collections = context
                            .materialised
                            .into_iter()
                            .filter(|group| {
                                group.is_scoped_to(WorkflowStageId::ImplementationPlanning)
                                    || group.is_scoped_to(WorkflowStageId::Execution)
                            })
                            .map(|group| group.as_knowledge_group().collection)
                            .collect();
                        Ok(WorkflowRunResult::ReadyForPlanning {
                            architect_plan: plan,
                            knowledge_collections,
                        })
                    }
                    solutions::SolutionUserChoice::Revise { feedback } => {
                        Ok(WorkflowRunResult::NeedsRevision { feedback })
                    }
                }
            })
        },
    ))
    .with_findings::<WorkflowFinding>()
    .build()
}

fn read_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn solution_branch_step<C, R, E>(
    branch_step: solutions::SolutionBranchStep<C, R, E>,
    branch: solutions::SolutionBranch,
) -> WorkflowStep<C, R, E, solutions::SolutionInput, solutions::SolutionDraft>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
    E: Debug + 'static,
{
    branch_step
        .map_input(move |input| solutions::SolutionBranchInput::new(branch, input))
        .map_findings(WorkflowFinding::from)
}

fn solution_input_from_materialisation(
    input: KnowledgeMaterialisationOutput,
) -> solutions::SolutionInput {
    let knowledge = knowledge::build_stage_knowledge_session(
        WorkflowStageId::Solutions,
        WorkflowStageId::Solutions.default_system_prompt(),
        &input.materialised,
    );
    solutions::SolutionInput::new(input.discovery, knowledge)
}

fn workflow_llm_config() -> OpenAiConfig {
    let api_key = read_env("MMAT_LLM_API_KEY")
        .or_else(|| read_env("OPENAI_API_KEY"))
        .unwrap_or_default();
    let base_url =
        read_env("MMAT_LLM_BASE_URL").unwrap_or_else(|| DEFAULT_LLM_BASE_URL.to_string());

    OpenAiConfig::new(api_key).with_base_url(base_url)
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use futures::future::LocalBoxFuture;
    use naaf_llm::{
        AssistantMessage, CompletionRequest, CompletionResponse, Executor, ExecutorConfig,
        ExecutorError, LlmClient, Message, Tool, ToolCall, ToolRegistry,
    };

    use crate::project::{ProjectConfig, ProjectId};

    use super::{KnowledgeRuntimeConfig, MAX_EXECUTOR_RETRIES, execute_with_turn_limit_retry};

    #[test]
    fn project_runtime_config_uses_project_paths_and_prefix() {
        let root = PathBuf::from("/tmp/mmat-runtime-project");
        let project = ProjectConfig {
            id: ProjectId::new("runtime").expect("project id should parse"),
            name: "Runtime".to_string(),
            root: root.clone(),
            data_dir: root.join(".mmat-data"),
            enabled: true,
            qdrant_collection_prefix: "p_runtime".to_string(),
            repo_label: Some("runtime-repo".to_string()),
        };

        let config =
            KnowledgeRuntimeConfig::from_project(&project).expect("config should be built");

        assert_eq!(config.workspace_root, root);
        assert_eq!(
            config.sqlite_path,
            project.data_dir.join("knowledge.sqlite3")
        );
        assert_eq!(config.qdrant_collection_prefix, "p_runtime");
        assert_eq!(config.repo.as_deref(), Some("runtime-repo"));
    }

    struct CountingClient {
        calls: std::sync::Arc<AtomicUsize>,
        fail_until: usize,
    }

    impl LlmClient for CountingClient {
        type Runtime = ();
        type Error = std::convert::Infallible;

        fn complete<'a>(
            &'a self,
            _runtime: &'a Self::Runtime,
            _request: CompletionRequest,
        ) -> LocalBoxFuture<'a, Result<CompletionResponse, Self::Error>> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if call < self.fail_until {
                    Ok(CompletionResponse::new(AssistantMessage::with_tool_calls(
                        None,
                        vec![ToolCall {
                            call_id: format!("call-{call}"),
                            tool_name: "noop".to_string(),
                            arguments: serde_json::json!({}),
                        }],
                    )))
                } else {
                    Ok(CompletionResponse::new(AssistantMessage::from_text(
                        "success",
                    )))
                }
            })
        }
    }

    struct NoopTool;

    impl Tool for NoopTool {
        type Runtime = ();
        type Error = std::convert::Infallible;

        fn spec(&self) -> naaf_llm::ToolSpec {
            naaf_llm::ToolSpec {
                name: "noop".to_string(),
                description: "does nothing".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                }),
            }
        }

        fn call<'a>(
            &'a self,
            _runtime: &'a Self::Runtime,
            _arguments: serde_json::Value,
        ) -> LocalBoxFuture<'a, Result<serde_json::Value, Self::Error>> {
            Box::pin(async move { Ok(serde_json::json!({})) })
        }
    }

    #[tokio::test]
    async fn execute_retry_succeeds_after_backoff() {
        let client = CountingClient {
            calls: std::sync::Arc::new(AtomicUsize::new(0)),
            fail_until: 1,
        };
        let calls = std::sync::Arc::clone(&client.calls);
        let mut tools = ToolRegistry::new();
        tools.register(NoopTool).expect("register noop tool");
        let executor = Executor::with_tools(client, tools).with_config(ExecutorConfig::new(1));

        let result = execute_with_turn_limit_retry(
            &executor,
            &(),
            CompletionRequest::new("test", vec![Message::user("hi")]),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().final_message().content.as_deref(),
            Some("success")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn execute_retry_fails_after_max_retries() {
        let client = CountingClient {
            calls: std::sync::Arc::new(AtomicUsize::new(0)),
            fail_until: usize::MAX,
        };
        let calls = std::sync::Arc::clone(&client.calls);
        let mut tools = ToolRegistry::new();
        tools.register(NoopTool).expect("register noop tool");
        let executor = Executor::with_tools(client, tools).with_config(ExecutorConfig::new(1));

        let result = execute_with_turn_limit_retry(
            &executor,
            &(),
            CompletionRequest::new("test", vec![Message::user("hi")]),
        )
        .await;

        assert!(matches!(
            result,
            Err(ExecutorError::TurnLimitExceeded { max_turns: 1 })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), MAX_EXECUTOR_RETRIES);
    }

    #[derive(Debug, Clone)]
    struct FakeError(&'static str);

    impl std::fmt::Display for FakeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for FakeError {}

    struct ErrorClient;

    impl LlmClient for ErrorClient {
        type Runtime = ();
        type Error = FakeError;

        fn complete<'a>(
            &'a self,
            _runtime: &'a Self::Runtime,
            _request: CompletionRequest,
        ) -> LocalBoxFuture<'a, Result<CompletionResponse, Self::Error>> {
            Box::pin(async move { Err(FakeError("client failure")) })
        }
    }

    #[tokio::test]
    async fn execute_retry_returns_non_turn_limit_errors_immediately() {
        let executor = Executor::new(ErrorClient);
        let result = execute_with_turn_limit_retry(
            &executor,
            &(),
            CompletionRequest::new("test", vec![Message::user("hi")]),
        )
        .await;

        assert!(matches!(
            result,
            Err(ExecutorError::Client(FakeError("client failure")))
        ));
    }
}
