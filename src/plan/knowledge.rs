#![allow(dead_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    convert::Infallible,
    fmt::{Debug, Display},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use futures::{future, future::LocalBoxFuture};
use naaf_core::{Attempt, RetryPolicy, Step, check_fn, repair_fn, task_fn};
use naaf_knowledge::ingest::{ingest_content, ingest_directory, ingest_file};
use naaf_knowledge::{
    KnowledgeGroup, KnowledgeGroupStore, KnowledgePromptConfig, SourceInfo, SourceType,
    augment_system_prompt,
};
use naaf_llm::{
    AdaptorError, CompletionRequest, Executor, ExecutorConfig, HumanIO, LlmAgent, LlmClient,
    Message, TaskError, ToolRegistry,
};
use naaf_persistence_sqlite::SqliteKnowledgeGroupStore;
use naaf_qdrant::{OpenAiEmbedder, QdrantAgent, QdrantClient};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::{
    plan::{
        WorkflowBuildError, WorkflowStageId, WorkflowTaskError, discovery::DiscoveryOutput,
        execute_with_turn_limit_retry, input_token_budget_for_model, parser::decode_outcome,
    },
    project::prefix_collection_id,
};

type KnowledgeStep<C, R, E> =
    Step<R, KnowledgeInput, KnowledgePlan, KnowledgeFinding, KnowledgeStepError<C, R, E>>;
type KnowledgeStepError<C, R, E> = WorkflowTaskError<C, R, E>;

const MAX_PLANNING_ATTEMPTS: usize = 3;
pub const MODEL: &str = "gpt-5.5";
pub const SYSTEM_PROMPT: &str = "You are the knowledge planning stage for MMAT. Your job is to identify the minimum useful knowledge groups for downstream work, scope each group to the stages that need it, and name the concrete sources that should be materialised.";
pub const UPSTREAM_NAAF_FOLLOW_UPS: &[&str] = &[
    "Add first-class web and paper acquisition helpers to naaf-knowledge.",
    "Add duplicate and near-duplicate detection to naaf-knowledge linting.",
];

pub(super) trait KnowledgeBackend: 'static {
    fn initialise_group<'a>(
        &'a self,
        group: &'a MaterialisedKnowledgeGroup,
    ) -> LocalBoxFuture<'a, Result<(), KnowledgeError>>;

    fn ingest_source<'a>(
        &'a self,
        group: &'a MaterialisedKnowledgeGroup,
        source: &'a KnowledgeSource,
    ) -> LocalBoxFuture<'a, Result<(), KnowledgeError>>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnowledgeFinding {
    EmptyPlan,
    DuplicateCollectionName(String),
    GroupWithoutStages(String),
    GroupWithoutSources(String),
    MissingStageCoverage(WorkflowStageId),
    LintFailed(String),
    LintFindings(usize),
}

pub(super) struct MetadataOnlyKnowledgeBackend;

#[derive(Debug, Error)]
pub(super) enum KnowledgeError {
    #[error("knowledge failed: {0}")]
    Knowledge(String),
    #[error("knowledge store failed: {0}")]
    Store(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StageKnowledgeSession {
    pub(super) stage: WorkflowStageId,
    pub(super) system_prompt: String,
    pub(super) group_collections: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum KnowledgeGroupTemplate {
    WorkspaceCode,
    WorkspaceDocs,
    DiscoveryTranscript,
    WebResearch,
    Papers,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum KnowledgeSourceKind {
    RepositoryPath,
    InlineMarkdown,
    InlinePlainText,
    DiscoveryTranscript,
    WebPage,
    ResearchPaper,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct KnowledgeSource {
    kind: KnowledgeSourceKind,
    label: String,
    location: Option<String>,
    content: Option<String>,
    recursive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct KnowledgeGroupPlan {
    template: KnowledgeGroupTemplate,
    instance_name: String,
    description: String,
    tags: Vec<String>,
    query_hints: Vec<String>,
    stages: Vec<WorkflowStageId>,
    sources: Vec<KnowledgeSource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct KnowledgePlan {
    groups: Vec<KnowledgeGroupPlan>,
    upstream_follow_ups: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeInput {
    discovery: DiscoveryOutput,
    findings: Vec<KnowledgeFinding>,
    last_plan: Option<KnowledgePlan>,
    turn: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct MaterialisedKnowledgeGroup {
    group: KnowledgeGroup,
    template: KnowledgeGroupTemplate,
    stages: Vec<WorkflowStageId>,
    sources: Vec<KnowledgeSource>,
    ingested_sources: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct QdrantKnowledgeBackendConfig {
    pub(super) url: String,
    pub(super) api_key: Option<String>,
    pub(super) embedding_api_key: String,
    pub(super) embedding_base_url: String,
    pub(super) embedding_model: String,
    pub(super) embedding_dimension: usize,
    pub(super) repo: Option<String>,
    pub(super) workspace_root: PathBuf,
}

pub(super) struct QdrantKnowledgeBackend<R> {
    agents: Mutex<BTreeMap<String, Arc<QdrantAgent<R>>>>,
    config: QdrantKnowledgeBackendConfig,
    repo: Option<String>,
    workspace_root: Option<PathBuf>,
}

impl Display for KnowledgeFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPlan => write!(f, "knowledge plan produced no groups"),
            Self::DuplicateCollectionName(collection) => {
                write!(
                    f,
                    "knowledge plan generated a colliding collection name `{collection}`"
                )
            }
            Self::GroupWithoutStages(group) => {
                write!(
                    f,
                    "knowledge group `{group}` is not scoped to any downstream stage"
                )
            }
            Self::GroupWithoutSources(group) => {
                write!(f, "knowledge group `{group}` has no sources")
            }
            Self::MissingStageCoverage(stage) => {
                write!(f, "knowledge plan does not cover the `{stage}` stage")
            }
            Self::LintFailed(error) => {
                write!(f, "knowledge lint check failed: {error}")
            }
            Self::LintFindings(count) => {
                write!(f, "knowledge lint found {count} issue(s)")
            }
        }
    }
}

impl KnowledgeBackend for MetadataOnlyKnowledgeBackend {
    fn initialise_group<'a>(
        &'a self,
        _group: &'a MaterialisedKnowledgeGroup,
    ) -> LocalBoxFuture<'a, Result<(), KnowledgeError>> {
        Box::pin(async move { Ok(()) })
    }

    fn ingest_source<'a>(
        &'a self,
        _group: &'a MaterialisedKnowledgeGroup,
        _source: &'a KnowledgeSource,
    ) -> LocalBoxFuture<'a, Result<(), KnowledgeError>> {
        Box::pin(async move { Ok(()) })
    }
}

impl KnowledgeGroupTemplate {
    fn slug(&self) -> &'static str {
        match self {
            Self::WorkspaceCode => "workspace-code",
            Self::WorkspaceDocs => "workspace-docs",
            Self::DiscoveryTranscript => "discovery-transcript",
            Self::WebResearch => "web-research",
            Self::Papers => "papers",
        }
    }

    fn default_name(&self) -> &'static str {
        match self {
            Self::WorkspaceCode => "Workspace Code",
            Self::WorkspaceDocs => "Workspace Docs",
            Self::DiscoveryTranscript => "Discovery Transcript",
            Self::WebResearch => "Web Research",
            Self::Papers => "Research Papers",
        }
    }

    fn default_description(&self) -> &'static str {
        match self {
            Self::WorkspaceCode => {
                "Repository code and implementation structure relevant to the current change."
            }
            Self::WorkspaceDocs => {
                "Repository documentation and prose relevant to the current change."
            }
            Self::DiscoveryTranscript => "Clarifications and intent captured during discovery.",
            Self::WebResearch => "Externally gathered web research relevant to the change.",
            Self::Papers => "Paper and long-form research material relevant to the change.",
        }
    }
}

impl KnowledgeSource {
    fn repository_path(path: impl Into<String>, recursive: bool) -> Self {
        let location = path.into();
        Self {
            kind: KnowledgeSourceKind::RepositoryPath,
            label: location.clone(),
            location: Some(location),
            content: None,
            recursive,
        }
    }

    fn inline_markdown(label: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            kind: KnowledgeSourceKind::InlineMarkdown,
            label: label.into(),
            location: None,
            content: Some(content.into()),
            recursive: false,
        }
    }

    fn inline_plain_text(label: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            kind: KnowledgeSourceKind::InlinePlainText,
            label: label.into(),
            location: None,
            content: Some(content.into()),
            recursive: false,
        }
    }

    fn discovery_transcript(label: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            kind: KnowledgeSourceKind::DiscoveryTranscript,
            label: label.into(),
            location: None,
            content: Some(content.into()),
            recursive: false,
        }
    }

    fn web_page(url: impl Into<String>, content: impl Into<String>) -> Self {
        let url = url.into();
        Self {
            kind: KnowledgeSourceKind::WebPage,
            label: url.clone(),
            location: Some(url),
            content: Some(content.into()),
            recursive: false,
        }
    }

    fn research_paper(title: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            kind: KnowledgeSourceKind::ResearchPaper,
            label: title.into(),
            location: None,
            content: Some(content.into()),
            recursive: false,
        }
    }

    fn inline_source_info(&self) -> Option<SourceInfo> {
        match self.kind {
            KnowledgeSourceKind::InlineMarkdown | KnowledgeSourceKind::WebPage => {
                Some(SourceInfo::markdown(
                    self.content.as_deref().unwrap_or_default(),
                    Some(self.label.clone()),
                ))
            }
            KnowledgeSourceKind::DiscoveryTranscript => Some(SourceInfo::conversation(
                self.content.as_deref().unwrap_or_default(),
                Some(self.label.clone()),
            )),
            KnowledgeSourceKind::InlinePlainText => Some(SourceInfo {
                source_type: SourceType::PlainText,
                path: None,
                title: Some(self.label.clone()),
                language: None,
                content: self.content.clone(),
            }),
            KnowledgeSourceKind::ResearchPaper => Some(SourceInfo {
                source_type: SourceType::Paper,
                path: None,
                title: Some(self.label.clone()),
                language: None,
                content: self.content.clone(),
            }),
            KnowledgeSourceKind::RepositoryPath => None,
        }
    }
}

impl KnowledgeInput {
    pub(super) fn new(discovery: DiscoveryOutput) -> Self {
        Self {
            discovery,
            findings: Vec::new(),
            last_plan: None,
            turn: 0,
        }
    }
}

impl MaterialisedKnowledgeGroup {
    pub(super) fn as_knowledge_group(&self) -> KnowledgeGroup {
        self.group.clone()
    }

    pub(super) fn is_scoped_to(&self, stage: WorkflowStageId) -> bool {
        self.stages.contains(&stage)
    }
}

impl PartialEq for MaterialisedKnowledgeGroup {
    fn eq(&self, other: &Self) -> bool {
        self.group.collection == other.group.collection
            && self.group.name == other.group.name
            && self.group.description == other.group.description
            && self.group.tags == other.group.tags
            && self.group.query_hints == other.group.query_hints
            && self.group.metadata == other.group.metadata
            && self.template == other.template
            && self.stages == other.stages
            && self.sources == other.sources
            && self.ingested_sources == other.ingested_sources
    }
}

impl Eq for MaterialisedKnowledgeGroup {}

impl<R> QdrantKnowledgeBackend<R> {
    pub(super) fn new(config: QdrantKnowledgeBackendConfig) -> Self {
        Self {
            agents: Mutex::new(BTreeMap::new()),
            repo: config.repo.clone(),
            workspace_root: Some(config.workspace_root.clone()),
            config,
        }
    }

    #[cfg(test)]
    fn with_workspace_root(mut self, workspace_root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(workspace_root.into());
        self
    }

    #[cfg(test)]
    fn with_agent(self, collection: impl Into<String>, agent: QdrantAgent<R>) -> Self {
        self.agents
            .lock()
            .insert(collection.into(), Arc::new(agent));
        self
    }

    fn agent_for(
        &self,
        group: &MaterialisedKnowledgeGroup,
    ) -> Result<Arc<QdrantAgent<R>>, KnowledgeError> {
        self.agent_for_collection(&group.group.collection)
    }

    pub(super) fn agent_for_group(
        &self,
        group: &KnowledgeGroup,
    ) -> Result<Arc<QdrantAgent<R>>, KnowledgeError> {
        self.agent_for_collection(&group.collection)
    }

    fn agent_for_collection(
        &self,
        collection: &str,
    ) -> Result<Arc<QdrantAgent<R>>, KnowledgeError> {
        if let Some(agent) = self.agents.lock().get(collection).cloned() {
            return Ok(agent);
        }

        let client = QdrantClient::from_url(&self.config.url, self.config.api_key.clone())
            .map_err(|error| KnowledgeError::Knowledge(error.to_string()))?
            .with_collection(collection);
        let embedder = OpenAiEmbedder::with_model(
            self.config.embedding_api_key.clone(),
            self.config.embedding_model.clone(),
            self.config.embedding_dimension,
        )
        .with_base_url(self.config.embedding_base_url.clone());
        let agent = Arc::new(QdrantAgent::new(client, Box::new(embedder)));

        self.agents
            .lock()
            .insert(collection.to_owned(), Arc::clone(&agent));

        Ok(agent)
    }
}

impl<R: 'static> KnowledgeBackend for QdrantKnowledgeBackend<R> {
    fn initialise_group<'a>(
        &'a self,
        group: &'a MaterialisedKnowledgeGroup,
    ) -> LocalBoxFuture<'a, Result<(), KnowledgeError>> {
        let agent = self.agent_for(group);
        Box::pin(async move {
            let agent = agent?;
            agent
                .init_collection()
                .await
                .map_err(|error| KnowledgeError::Knowledge(error.to_string()))
        })
    }

    fn ingest_source<'a>(
        &'a self,
        group: &'a MaterialisedKnowledgeGroup,
        source: &'a KnowledgeSource,
    ) -> LocalBoxFuture<'a, Result<(), KnowledgeError>> {
        let repo = self.repo.clone();
        let workspace_root = self.workspace_root.clone();
        let agent = self.agent_for(group);
        Box::pin(async move {
            let agent = agent?;
            match source.kind {
                KnowledgeSourceKind::RepositoryPath => {
                    let workspace_root = workspace_root.as_ref().ok_or_else(|| {
                        KnowledgeError::Knowledge(
                            "repository path ingestion requires a configured workspace root"
                                .to_string(),
                        )
                    })?;
                    let path = resolve_repository_path(
                        workspace_root,
                        source.location.clone().ok_or_else(|| {
                            KnowledgeError::Knowledge(
                                "repository path source is missing a location".to_string(),
                            )
                        })?,
                    )?;

                    if source.recursive {
                        ingest_directory(agent.as_ref(), &path, repo.as_deref())
                            .await
                            .map_err(|error| KnowledgeError::Knowledge(error.to_string()))?;
                    } else {
                        ingest_file(agent.as_ref(), &path, repo.as_deref())
                            .await
                            .map_err(|error| KnowledgeError::Knowledge(error.to_string()))?;
                    }
                }
                _ => {
                    let source_info = source.inline_source_info().ok_or_else(|| {
                        KnowledgeError::Knowledge(
                            "source cannot be converted to inline knowledge".to_string(),
                        )
                    })?;
                    let content = source.content.clone().unwrap_or_default();
                    ingest_content(agent.as_ref(), &content, &source_info, repo.as_deref())
                        .await
                        .map_err(|error| KnowledgeError::Knowledge(error.to_string()))?;
                }
            }

            Ok(())
        })
    }
}

pub(super) fn build_stage_knowledge_session(
    stage: WorkflowStageId,
    base_system_prompt: &str,
    materialised: &[MaterialisedKnowledgeGroup],
) -> StageKnowledgeSession {
    let groups = scoped_groups_for_stage(materialised, stage);
    let system_prompt = augment_system_prompt(
        base_system_prompt,
        &groups,
        &KnowledgePromptConfig::default(),
    );

    StageKnowledgeSession {
        stage,
        system_prompt,
        group_collections: groups.into_iter().map(|group| group.collection).collect(),
    }
}

pub(super) fn materialisation_step<C, R, E, B>(
    store: Arc<SqliteKnowledgeGroupStore>,
    backend: Arc<B>,
    collection_prefix: String,
) -> Step<
    R,
    KnowledgePlan,
    Vec<MaterialisedKnowledgeGroup>,
    KnowledgeFinding,
    WorkflowTaskError<C, R, E>,
>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
    E: Debug + 'static,
    B: KnowledgeBackend,
{
    Step::builder(task_fn(move |_runtime: &R, plan: KnowledgePlan| {
        let store = store.clone();
        let backend = backend.clone();
        let collection_prefix = collection_prefix.clone();
        Box::pin(async move {
            materialise_knowledge_plan(store.as_ref(), backend.as_ref(), &collection_prefix, &plan)
                .await
                .map_err(|error| TaskError::Build(WorkflowBuildError::Knowledge(error)))
        })
    }))
    .with_findings::<KnowledgeFinding>()
    .build_persistent()
}

pub(super) fn step<C, R, E>(agent: &LlmAgent<C, R, E>) -> KnowledgeStep<C, R, E>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    Step::builder(agent.json_task(
        MODEL.into(),
        SYSTEM_PROMPT.into(),
        |i| Ok::<_, WorkflowBuildError<R::Error>>(build_prompt(i)),
        decode_outcome,
        "knowledge-planning-turn".into(),
    ))
    .validate(check_fn(|r, i, o| Box::pin(future::ok(validate(r, i, o)))))
    .repair_with(repair_fn(|_r, a| {
        Box::pin(async move { repair(a).await.map_err(|error| match error {}) })
    }))
    .retry_policy(RetryPolicy::new(MAX_PLANNING_ATTEMPTS))
    .build_persistent()
}

/// Builds a knowledge planning step that includes lint validation.
///
/// Runs the knowledge planning step, then validates the plan with lint. If lint
/// finds issues, feeds findings back to the knowledge planning step for retry.
pub(super) fn step_with_lint<C, R, E>(
    agent: &LlmAgent<C, R, E>,
    store: Arc<SqliteKnowledgeGroupStore>,
    collection_prefix: String,
    workspace_root: PathBuf,
) -> KnowledgeStep<C, R, E>
where
    C: LlmClient<Runtime = R> + Clone + 'static,
    C::Error: Debug + Display + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    let lint_check = knowledge_lint_check(store.as_ref(), &collection_prefix);
    let client = (*agent.executor().client()).clone();
    let system_prompt = format!(
        "{}\n\nYou have access to repository tools: `glob_paths`, `search_files`, and `read_file`. Use them to inspect the workspace before choosing repository knowledge sources.",
        SYSTEM_PROMPT
    );

    let task = task_fn(move |runtime: &R, input: KnowledgeInput| {
        let client = client.clone();
        let system_prompt = system_prompt.clone();
        let workspace_root = workspace_root.clone();
        Box::pin(async move {
            let request = CompletionRequest::new(
                MODEL.to_string(),
                vec![
                    Message::system(system_prompt),
                    Message::user(build_prompt(input)),
                ],
            )
            .with_metadata(json!({
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "knowledge_plan",
                        "strict": false,
                        "schema": {
                            "type": "object",
                            "properties": {},
                            "additionalProperties": true
                        }
                    }
                }
            }));

            let mut tools = ToolRegistry::<R, Infallible>::new();
            register_repository_tools(&mut tools, workspace_root);
            let executor = Executor::with_tools(client, tools).with_config(
                ExecutorConfig::default()
                    .with_max_input_tokens(input_token_budget_for_model(MODEL)),
            );
            let outcome = execute_with_turn_limit_retry(&executor, runtime, request)
                .await
                .map_err(|error| {
                    AdaptorError::Build(WorkflowBuildError::Workflow(format!(
                        "executor failed: {error}"
                    )))
                })?;

            decode_outcome(outcome).map_err(AdaptorError::Decode)
        })
    });

    Step::builder(task)
        .validate(check_fn(|r, i, o| Box::pin(future::ok(validate(r, i, o)))))
        .validate(check_fn(move |_, _, o| {
            Box::pin(future::ok(lint_check(&(), o)))
        }))
        .repair_with(repair_fn(|_r, a| {
            Box::pin(async move { repair(a).await.map_err(|error| match error {}) })
        }))
        .retry_policy(RetryPolicy::new(MAX_PLANNING_ATTEMPTS))
        .build_persistent()
}

fn build_prompt(input: KnowledgeInput) -> String {
    let mut lines = vec![
        format!("Planning turn: {}", input.turn + 1),
        format!("Problem statement: {}", input.discovery.problem_statement),
        format!("Recommended path: {}", input.discovery.recommended_path),
    ];

    if !input.discovery.goals.is_empty() {
        lines.push(format!("Goals: {}", input.discovery.goals.join(" | ")));
    }

    if !input.discovery.constraints.is_empty() {
        lines.push(format!(
            "Constraints: {}",
            input.discovery.constraints.join(" | ")
        ));
    }

    if !input.discovery.assumptions.is_empty() {
        lines.push(format!(
            "Assumptions: {}",
            input.discovery.assumptions.join(" | ")
        ));
    }

    if !input.discovery.risks.is_empty() {
        lines.push(format!("Risks: {}", input.discovery.risks.join(" | ")));
    }

    if !input.discovery.notes.is_empty() {
        lines.push(format!("Notes: {}", input.discovery.notes.join(" | ")));
    }

    if let Some(prior_plan) = &input.last_plan {
        lines.push(String::new());
        lines.push("Prior knowledge plan:".to_string());
        lines.extend(prior_plan.groups.iter().map(|group| {
            format!(
                "- {} / {} => stages: {} ; sources: {}",
                group.template.slug(),
                group.instance_name,
                group
                    .stages
                    .iter()
                    .map(|stage| stage.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                group
                    .sources
                    .iter()
                    .map(|source| source.label.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }));
    }

    if !input.findings.is_empty() {
        lines.push(String::new());
        lines.push("Validation findings to address:".to_string());
        lines.extend(
            input
                .findings
                .iter()
                .map(|finding| format!("- {}", finding)),
        );
    }

    lines.push(String::new());
    lines.push(
        "Return only one JSON object. Do not include markdown, prose, code fences, or hidden reasoning in the assistant content."
            .to_string(),
    );
    lines.push(
        "The JSON object must use this exact shape: {\"groups\":[{\"template\":string,\"instance_name\":string,\"description\":string,\"tags\":string[],\"query_hints\":string[],\"stages\":string[],\"sources\":[{\"kind\":string,\"label\":string,\"location\":string|null,\"content\":string|null,\"recursive\":boolean}]}],\"upstream_follow_ups\":string[]}."
            .to_string(),
    );
    lines.push(
        "Valid template values: WorkspaceCode, WorkspaceDocs, DiscoveryTranscript, WebResearch, Papers. Valid stage values: Solutions, SoftwareArchitect. Valid source kind values: RepositoryPath, InlineMarkdown, InlinePlainText, DiscoveryTranscript, WebPage, ResearchPaper."
            .to_string(),
    );
    lines.push("Only propose groups that a later stage will actually need.".to_string());
    lines.push(
        "Prefer repository paths or discovery transcripts for first-party context; list web pages and papers only when their content is already available or acquisition is explicitly required."
            .to_string(),
    );
    lines.push(
        "Record platform-level NAAF follow-up work in upstream_follow_ups instead of hiding it inside source labels."
            .to_string(),
    );

    lines.join("\n")
}

fn knowledge_lint_check(
    _store: &SqliteKnowledgeGroupStore,
    _collection_prefix: &str,
) -> impl Fn(&(), KnowledgePlan) -> Vec<KnowledgeFinding> + 'static {
    move |_runtime: &(), plan: KnowledgePlan| {
        let mut findings = Vec::new();

        if plan.groups.is_empty() {
            findings.push(KnowledgeFinding::EmptyPlan);
            return findings;
        }

        let mut seen_collections = std::collections::BTreeSet::new();
        for group_plan in &plan.groups {
            let collection =
                sanitise_collection_name(group_plan.template.slug(), &group_plan.instance_name);
            if !seen_collections.insert(collection.clone()) {
                findings.push(KnowledgeFinding::DuplicateCollectionName(collection));
            }
            if group_plan.sources.is_empty() {
                findings.push(KnowledgeFinding::GroupWithoutSources(
                    group_plan.instance_name.clone(),
                ));
            }
            if group_plan.stages.is_empty() {
                findings.push(KnowledgeFinding::GroupWithoutStages(
                    group_plan.instance_name.clone(),
                ));
            }
        }

        if !findings.is_empty() {
            findings.push(KnowledgeFinding::LintFindings(findings.len()));
        }

        findings
    }
}

async fn materialise_knowledge_plan<B>(
    store: &SqliteKnowledgeGroupStore,
    backend: &B,
    collection_prefix: &str,
    plan: &KnowledgePlan,
) -> Result<Vec<MaterialisedKnowledgeGroup>, KnowledgeError>
where
    B: KnowledgeBackend,
{
    let mut materialised = Vec::new();
    let mut seen_collections = BTreeSet::new();

    for group_plan in &plan.groups {
        let base_collection =
            sanitise_collection_name(group_plan.template.slug(), &group_plan.instance_name);
        let collection = prefix_collection_id(collection_prefix, &base_collection);
        if !seen_collections.insert(collection.clone()) {
            return Err(KnowledgeError::Knowledge(format!(
                "knowledge plan generated a colliding collection name `{collection}`"
            )));
        }

        let previous_group = store.load_group(&collection).await?;
        let mut group = KnowledgeGroup::new(
            collection,
            format!(
                "{}: {}",
                group_plan.template.default_name(),
                group_plan.instance_name
            ),
            if group_plan.description.trim().is_empty() {
                group_plan.template.default_description().to_string()
            } else {
                group_plan.description.clone()
            },
        )
        .with_tags(group_plan.tags.clone())
        .with_query_hints(group_plan.query_hints.clone())
        .with_metadata_field("template", json!(group_plan.template.slug()))
        .with_metadata_field(
            "stages",
            json!(
                group_plan
                    .stages
                    .iter()
                    .map(|stage| stage.as_str())
                    .collect::<Vec<_>>()
            ),
        )
        .with_metadata_field("source_count", json!(group_plan.sources.len()));

        if !group_plan.sources.is_empty() {
            group = group.with_metadata_field(
                "source_labels",
                json!(
                    group_plan
                        .sources
                        .iter()
                        .map(|source| source.label.clone())
                        .collect::<Vec<_>>()
                ),
            );
        }

        store.upsert_group(&group).await?;

        let mut materialised_group = MaterialisedKnowledgeGroup {
            group,
            template: group_plan.template.clone(),
            stages: group_plan.stages.clone(),
            sources: group_plan.sources.clone(),
            ingested_sources: 0,
        };

        let materialisation_result = async {
            backend.initialise_group(&materialised_group).await?;

            for source in &materialised_group.sources {
                backend.ingest_source(&materialised_group, source).await?;
                materialised_group.ingested_sources += 1;
            }

            Ok::<(), KnowledgeError>(())
        }
        .await;

        if let Err(error) = materialisation_result {
            match previous_group {
                Some(ref previous_group) => store.upsert_group(previous_group).await?,
                None => {
                    store
                        .delete_group(&materialised_group.group.collection)
                        .await?
                }
            }
            return Err(error);
        }

        materialised.push(materialised_group);
    }

    Ok(materialised)
}

fn register_repository_tools<R>(tools: &mut ToolRegistry<R, Infallible>, workspace_root: PathBuf)
where
    R: 'static,
{
    if let Err(error) = tools.register(naaf_llm::repository::ReadFileTool::<R>::new(
        workspace_root.clone(),
    )) {
        tracing::warn!(%error, "failed to register repository read tool");
    }
    if let Err(error) = tools.register(naaf_llm::repository::GlobPathsTool::<R>::new(
        workspace_root.clone(),
    )) {
        tracing::warn!(%error, "failed to register repository glob tool");
    }
    if let Err(error) = tools.register(naaf_llm::repository::SearchFilesTool::<R>::new(
        workspace_root,
    )) {
        tracing::warn!(%error, "failed to register repository search tool");
    }
}

async fn repair(
    attempts: Vec<Attempt<KnowledgeInput, KnowledgePlan, KnowledgeFinding>>,
) -> Result<KnowledgeInput, Infallible> {
    let latest_attempt = attempts
        .last()
        .expect("knowledge planning repair requires an attempt");

    Ok(KnowledgeInput {
        discovery: latest_attempt.input.discovery.clone(),
        findings: latest_attempt.findings.clone(),
        last_plan: Some(latest_attempt.output.clone()),
        turn: latest_attempt.input.turn + 1,
    })
}

fn resolve_repository_path(
    workspace_root: &Path,
    location: String,
) -> Result<PathBuf, KnowledgeError> {
    let requested = PathBuf::from(location);
    if requested.is_absolute() {
        return Err(KnowledgeError::Knowledge(
            "repository knowledge sources must stay beneath the workspace root".to_string(),
        ));
    }
    if requested
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(KnowledgeError::Knowledge(
            "repository knowledge sources must stay beneath the workspace root".to_string(),
        ));
    }

    let candidate = workspace_root.join(requested);
    let canonical_root = workspace_root.canonicalize()?;
    let canonical_candidate = candidate.canonicalize()?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(KnowledgeError::Knowledge(
            "repository knowledge sources must stay beneath the workspace root".to_string(),
        ));
    }

    Ok(canonical_candidate)
}

fn sanitise_collection_name(template_slug: &str, instance_name: &str) -> String {
    let mut collection = format!("{}-{}", template_slug, instance_name)
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();

    while collection.contains("--") {
        collection = collection.replace("--", "-");
    }

    collection.trim_matches('-').to_string()
}

fn scoped_groups_for_stage(
    materialised: &[MaterialisedKnowledgeGroup],
    stage: WorkflowStageId,
) -> Vec<KnowledgeGroup> {
    materialised
        .iter()
        .filter(|group| group.stages.contains(&stage))
        .map(|group| group.group.clone())
        .collect()
}

fn validate<R>(_runtime: &R, _input: KnowledgeInput, plan: KnowledgePlan) -> Vec<KnowledgeFinding> {
    let mut findings = Vec::new();

    if plan.groups.is_empty() {
        findings.push(KnowledgeFinding::EmptyPlan);
    }

    let mut seen_collections = BTreeSet::new();
    let mut covered_stages = BTreeSet::new();
    for group in &plan.groups {
        let collection = sanitise_collection_name(group.template.slug(), &group.instance_name);
        if !seen_collections.insert(collection.clone()) {
            findings.push(KnowledgeFinding::DuplicateCollectionName(collection));
        }

        if group.stages.is_empty() {
            findings.push(KnowledgeFinding::GroupWithoutStages(
                group.instance_name.clone(),
            ));
        } else {
            covered_stages.extend(group.stages.iter().copied());
        }

        if group.sources.is_empty() {
            findings.push(KnowledgeFinding::GroupWithoutSources(
                group.instance_name.clone(),
            ));
        }
    }

    for stage in [
        WorkflowStageId::Solutions,
        WorkflowStageId::SoftwareArchitect,
    ] {
        if !covered_stages.contains(&stage) {
            findings.push(KnowledgeFinding::MissingStageCoverage(stage));
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Arc};

    use naaf_llm::{
        AssistantMessage, CompletionRequest, CompletionResponse, HumanAnswer, HumanQuestion,
        Message,
    };
    use parking_lot::Mutex;

    use super::*;

    #[derive(Default)]
    struct StubKnowledgeBackend {
        initialised: Mutex<Vec<String>>,
        ingested: Mutex<Vec<(String, String)>>,
    }

    #[derive(Clone)]
    struct ScriptedClient {
        responses: Arc<Mutex<VecDeque<String>>>,
        prompts: Arc<Mutex<Vec<String>>>,
    }

    struct NoopRuntime;

    impl StubKnowledgeBackend {
        fn initialised(&self) -> Vec<String> {
            self.initialised.lock().clone()
        }

        fn ingested(&self) -> Vec<(String, String)> {
            self.ingested.lock().clone()
        }
    }

    impl ScriptedClient {
        fn new(responses: impl IntoIterator<Item = String>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
                prompts: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().clone()
        }
    }

    impl KnowledgeBackend for StubKnowledgeBackend {
        fn initialise_group<'a>(
            &'a self,
            group: &'a MaterialisedKnowledgeGroup,
        ) -> LocalBoxFuture<'a, Result<(), KnowledgeError>> {
            self.initialised.lock().push(group.group.collection.clone());
            Box::pin(async move { Ok(()) })
        }

        fn ingest_source<'a>(
            &'a self,
            group: &'a MaterialisedKnowledgeGroup,
            source: &'a KnowledgeSource,
        ) -> LocalBoxFuture<'a, Result<(), KnowledgeError>> {
            self.ingested
                .lock()
                .push((group.group.collection.clone(), source.label.clone()));
            Box::pin(async move { Ok(()) })
        }
    }

    impl LlmClient for ScriptedClient {
        type Error = Infallible;
        type Runtime = NoopRuntime;

        fn complete<'a>(
            &'a self,
            _runtime: &'a Self::Runtime,
            request: CompletionRequest,
        ) -> LocalBoxFuture<'a, Result<CompletionResponse, Self::Error>> {
            let prompt = request
                .messages
                .iter()
                .filter_map(|message| match message {
                    Message::User { content } => Some(content.clone()),
                    _ => None,
                })
                .next_back()
                .expect("request should include a user prompt");
            self.prompts.lock().push(prompt);
            let response = self
                .responses
                .lock()
                .pop_front()
                .expect("scripted response should exist");

            Box::pin(async move {
                Ok(CompletionResponse::new(AssistantMessage::from_text(
                    response,
                )))
            })
        }
    }

    impl HumanIO for NoopRuntime {
        type Error = Infallible;

        fn ask<'a>(
            &'a self,
            _question: HumanQuestion,
        ) -> LocalBoxFuture<'a, Result<HumanAnswer, Self::Error>> {
            Box::pin(async move { unreachable!("knowledge planning does not ask human questions") })
        }
    }

    fn sample_discovery() -> DiscoveryOutput {
        DiscoveryOutput {
            assistant_message: String::new(),
            ready_for_solution: true,
            problem_statement: "Rewrite MMAT".to_string(),
            goals: vec!["Keep stages readable".to_string()],
            constraints: vec!["Use SQLite".to_string()],
            assumptions: vec!["Live-only questions are fine".to_string()],
            risks: vec!["NAAF gaps remain".to_string()],
            notes: Vec::new(),
            recommended_path: "Plan knowledge, then branch".to_string(),
            open_questions: Vec::new(),
            sub_domains: Vec::new(),
        }
    }

    fn valid_knowledge_plan() -> KnowledgePlan {
        KnowledgePlan {
            groups: vec![
                KnowledgeGroupPlan {
                    template: KnowledgeGroupTemplate::DiscoveryTranscript,
                    instance_name: "rewrite-answers".to_string(),
                    description: "Discovery clarifications for the rewrite".to_string(),
                    tags: vec!["rewrite".to_string()],
                    query_hints: vec!["Use the user's constraints directly".to_string()],
                    stages: vec![WorkflowStageId::Solutions],
                    sources: vec![KnowledgeSource::discovery_transcript(
                        "Discovery transcript",
                        "User chose SQLite for metadata.",
                    )],
                },
                KnowledgeGroupPlan {
                    template: KnowledgeGroupTemplate::WorkspaceCode,
                    instance_name: "repo".to_string(),
                    description: "Repository code for architect follow-through".to_string(),
                    tags: vec!["code".to_string()],
                    query_hints: vec!["Use the repository structure".to_string()],
                    stages: vec![WorkflowStageId::SoftwareArchitect],
                    sources: vec![KnowledgeSource::inline_markdown(
                        "Repo summary",
                        "Workflow code facts",
                    )],
                },
            ],
            upstream_follow_ups: Vec::new(),
        }
    }

    fn invalid_knowledge_plan() -> KnowledgePlan {
        KnowledgePlan {
            groups: vec![KnowledgeGroupPlan {
                template: KnowledgeGroupTemplate::WorkspaceCode,
                instance_name: "repo".to_string(),
                description: "Repository facts".to_string(),
                tags: Vec::new(),
                query_hints: Vec::new(),
                stages: vec![WorkflowStageId::Solutions],
                sources: vec![KnowledgeSource::inline_markdown("Repo", "facts")],
            }],
            upstream_follow_ups: Vec::new(),
        }
    }

    #[test]
    fn prompt_records_discovery_context() {
        let prompt = build_prompt(KnowledgeInput::new(sample_discovery()));

        assert!(prompt.contains("Problem statement: Rewrite MMAT"));
        assert!(prompt.contains("Constraints: Use SQLite"));
        assert!(prompt.contains("Recommended path: Plan knowledge, then branch"));
        assert!(prompt.contains("Return only one JSON object"));
        assert!(prompt.contains("\"groups\""));
        assert!(prompt.contains("Do not include markdown"));
    }

    #[test]
    fn validation_requires_stage_coverage() {
        let findings = validate(
            &(),
            KnowledgeInput::new(sample_discovery()),
            invalid_knowledge_plan(),
        );

        assert_eq!(
            findings,
            vec![KnowledgeFinding::MissingStageCoverage(
                WorkflowStageId::SoftwareArchitect
            )]
        );
    }

    #[tokio::test]
    async fn planning_step_retries_with_validation_findings() {
        let client = ScriptedClient::new(vec![
            serde_json::to_string(&invalid_knowledge_plan()).expect("plan should serialise"),
            serde_json::to_string(&valid_knowledge_plan()).expect("plan should serialise"),
        ]);
        let agent = LlmAgent::new(client.clone());
        let output = step(&agent)
            .run(&NoopRuntime, KnowledgeInput::new(sample_discovery()))
            .await
            .expect("knowledge planning should recover");

        assert_eq!(output, valid_knowledge_plan());
        let prompts = client.prompts();
        assert_eq!(prompts.len(), 2);
        assert!(
            prompts[1].contains("knowledge plan does not cover the `software-architect` stage")
        );
    }

    #[tokio::test]
    async fn materialisation_persists_groups_and_ingests_sources() {
        let store = SqliteKnowledgeGroupStore::open_in_memory().expect("SQLite store should open");
        let backend = StubKnowledgeBackend::default();
        let plan = valid_knowledge_plan();

        let materialised = materialise_knowledge_plan(&store, &backend, "p_test", &plan)
            .await
            .expect("knowledge plan should materialise");

        assert_eq!(materialised.len(), 2);
        assert_eq!(materialised[0].ingested_sources, 1);
        assert_eq!(
            backend.initialised(),
            vec![
                "p_test__discovery_transcript_rewrite_answers".to_string(),
                "p_test__workspace_code_repo".to_string(),
            ]
        );
        assert_eq!(
            backend.ingested(),
            vec![
                (
                    "p_test__discovery_transcript_rewrite_answers".to_string(),
                    "Discovery transcript".to_string(),
                ),
                (
                    "p_test__workspace_code_repo".to_string(),
                    "Repo summary".to_string()
                ),
            ]
        );
    }

    #[tokio::test]
    async fn stage_session_contains_only_scoped_groups() {
        let store = SqliteKnowledgeGroupStore::open_in_memory().expect("SQLite store should open");
        let backend = StubKnowledgeBackend::default();
        let materialised =
            materialise_knowledge_plan(&store, &backend, "p_session", &valid_knowledge_plan())
                .await
                .expect("knowledge should materialise");

        let session = build_stage_knowledge_session(
            WorkflowStageId::Solutions,
            "Solutions stage prompt",
            &materialised,
        );

        assert!(session.system_prompt.contains("Solutions stage prompt"));
        assert!(
            session
                .group_collections
                .contains(&"p_session__discovery_transcript_rewrite_answers".to_string())
        );
        assert!(
            !session
                .group_collections
                .contains(&"p_session__workspace_code_repo".to_string())
        );
    }

    #[tokio::test]
    async fn same_plan_uses_distinct_project_collections() {
        let first_store =
            SqliteKnowledgeGroupStore::open_in_memory().expect("first SQLite store should open");
        let second_store =
            SqliteKnowledgeGroupStore::open_in_memory().expect("second SQLite store should open");
        let backend = StubKnowledgeBackend::default();
        let plan = valid_knowledge_plan();

        let first = materialise_knowledge_plan(&first_store, &backend, "p_first", &plan)
            .await
            .expect("first plan should materialise");
        let second = materialise_knowledge_plan(&second_store, &backend, "p_second", &plan)
            .await
            .expect("second plan should materialise");

        assert_ne!(first[0].group.collection, second[0].group.collection);
        assert!(first[0].group.collection.starts_with("p_first__"));
        assert!(second[0].group.collection.starts_with("p_second__"));
    }
}
