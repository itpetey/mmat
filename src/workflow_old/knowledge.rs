use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use futures::future::LocalBoxFuture;
use naaf_core::{Attempt, RetryPolicy, Step, TaskExt, check_fn, repair_fn, task_fn};
use naaf_knowledge::ingest::{ingest_content, ingest_directory, ingest_file};
use naaf_knowledge::{
    KnowledgeGroup, KnowledgeGroupStore, KnowledgePromptConfig, SourceInfo, SourceType,
    augment_system_prompt,
};
use naaf_persistence_sqlite::SqliteKnowledgeGroupStore;
use naaf_qdrant::QdrantAgent;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    runtime::StagePromptProvider,
    workflow_old::{WorkflowError, WorkflowStageId, discovery::DiscoveryOutcome},
};

pub const UPSTREAM_NAAF_FOLLOW_UPS: &[&str] = &[
    "Add first-class web and paper acquisition helpers to naaf-knowledge.",
    "Add duplicate and near-duplicate detection to naaf-knowledge linting.",
];

pub trait KnowledgeBackend: 'static {
    fn initialise_group<'a>(
        &'a self,
        group: &'a MaterialisedKnowledgeGroup,
    ) -> LocalBoxFuture<'a, Result<(), WorkflowError>>;

    fn ingest_source<'a>(
        &'a self,
        group: &'a MaterialisedKnowledgeGroup,
        source: &'a KnowledgeSource,
    ) -> LocalBoxFuture<'a, Result<(), WorkflowError>>;
}

pub trait KnowledgePlanningAgent<R>: Send + Sync + 'static {
    fn plan<'a>(
        &'a self,
        runtime: &'a R,
        input: KnowledgePlanningInput,
        prompt: String,
    ) -> LocalBoxFuture<'a, Result<KnowledgePlan, WorkflowError>>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeGroupPlan {
    pub template: KnowledgeGroupTemplate,
    pub instance_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub query_hints: Vec<String>,
    pub stages: Vec<WorkflowStageId>,
    pub sources: Vec<KnowledgeSource>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaterialisedKnowledgeGroup {
    pub group: KnowledgeGroup,
    pub template: KnowledgeGroupTemplate,
    pub stages: Vec<WorkflowStageId>,
    pub sources: Vec<KnowledgeSource>,
    pub ingested_sources: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeSource {
    pub kind: KnowledgeSourceKind,
    pub label: String,
    pub location: Option<String>,
    pub content: Option<String>,
    pub recursive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct KnowledgePlan {
    pub groups: Vec<KnowledgeGroupPlan>,
    pub upstream_follow_ups: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnowledgeGroupTemplate {
    WorkspaceCode,
    WorkspaceDocs,
    DiscoveryTranscript,
    WebResearch,
    Papers,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnowledgeSourceKind {
    RepositoryPath,
    InlineMarkdown,
    InlinePlainText,
    DiscoveryTranscript,
    WebPage,
    ResearchPaper,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgePlanningInput {
    pub discovery: DiscoveryOutcome,
    pub turn: usize,
    pub findings: Vec<KnowledgePlanningFinding>,
    pub prior_plan: Option<KnowledgePlan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageKnowledgeSession {
    pub stage: WorkflowStageId,
    pub system_prompt: String,
    pub group_collections: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgePlanningTurn {
    pub input: KnowledgePlanningInput,
    pub plan: KnowledgePlan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnowledgePlanningFinding {
    EmptyPlan,
    DuplicateCollectionName(String),
    GroupWithoutStages(String),
    GroupWithoutSources(String),
    MissingDiscoveryTranscript,
    MissingStageCoverage(WorkflowStageId),
}

#[allow(dead_code)]
pub struct QdrantKnowledgeBackend<R> {
    agents: BTreeMap<String, Arc<QdrantAgent<R>>>,
    repo: Option<String>,
    workspace_root: Option<PathBuf>,
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

impl KnowledgeGroupTemplate {
    pub fn slug(&self) -> &'static str {
        match self {
            Self::WorkspaceCode => "workspace-code",
            Self::WorkspaceDocs => "workspace-docs",
            Self::DiscoveryTranscript => "discovery-transcript",
            Self::WebResearch => "web-research",
            Self::Papers => "papers",
        }
    }

    pub fn default_name(&self) -> &'static str {
        match self {
            Self::WorkspaceCode => "Workspace Code",
            Self::WorkspaceDocs => "Workspace Docs",
            Self::DiscoveryTranscript => "Discovery Transcript",
            Self::WebResearch => "Web Research",
            Self::Papers => "Research Papers",
        }
    }

    pub fn default_description(&self) -> &'static str {
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

impl KnowledgePlanningInput {
    pub fn new(discovery: DiscoveryOutcome) -> Self {
        Self {
            discovery,
            turn: 0,
            findings: Vec::new(),
            prior_plan: None,
        }
    }
}

impl KnowledgePlanningFinding {
    pub fn description(&self) -> String {
        match self {
            Self::EmptyPlan => "knowledge plan produced no groups".to_string(),
            Self::DuplicateCollectionName(collection) => {
                format!("knowledge plan generated a colliding collection name `{collection}`")
            }
            Self::GroupWithoutStages(group) => {
                format!("knowledge group `{group}` is not scoped to any downstream stage")
            }
            Self::GroupWithoutSources(group) => {
                format!("knowledge group `{group}` has no sources")
            }
            Self::MissingDiscoveryTranscript => {
                "knowledge plan omitted a discovery transcript despite discovery answers being present"
                    .to_string()
            }
            Self::MissingStageCoverage(stage) => {
                format!("knowledge plan does not cover the `{stage}` stage")
            }
        }
    }
}

impl KnowledgeSource {
    #[allow(dead_code)]
    pub fn repository_path(path: impl Into<String>, recursive: bool) -> Self {
        let location = path.into();
        Self {
            kind: KnowledgeSourceKind::RepositoryPath,
            label: location.clone(),
            location: Some(location),
            content: None,
            recursive,
        }
    }

    pub fn inline_markdown(label: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            kind: KnowledgeSourceKind::InlineMarkdown,
            label: label.into(),
            location: None,
            content: Some(content.into()),
            recursive: false,
        }
    }

    pub fn discovery_transcript(label: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            kind: KnowledgeSourceKind::DiscoveryTranscript,
            label: label.into(),
            location: None,
            content: Some(content.into()),
            recursive: false,
        }
    }

    #[allow(dead_code)]
    pub fn web_page(url: impl Into<String>, content: impl Into<String>) -> Self {
        let url = url.into();
        Self {
            kind: KnowledgeSourceKind::WebPage,
            label: url.clone(),
            location: Some(url),
            content: Some(content.into()),
            recursive: false,
        }
    }

    pub fn research_paper(title: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            kind: KnowledgeSourceKind::ResearchPaper,
            label: title.into(),
            location: None,
            content: Some(content.into()),
            recursive: false,
        }
    }

    #[allow(dead_code)]
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

#[allow(dead_code)]
impl<R> QdrantKnowledgeBackend<R> {
    pub fn new(repo: Option<String>) -> Self {
        Self {
            agents: BTreeMap::new(),
            repo,
            workspace_root: None,
        }
    }

    pub fn with_workspace_root(mut self, workspace_root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(workspace_root.into());
        self
    }

    pub fn with_agent(mut self, collection: impl Into<String>, agent: QdrantAgent<R>) -> Self {
        self.agents.insert(collection.into(), Arc::new(agent));
        self
    }

    fn agent_for(
        &self,
        group: &MaterialisedKnowledgeGroup,
    ) -> Result<Arc<QdrantAgent<R>>, WorkflowError> {
        self.agents
            .get(&group.group.collection)
            .cloned()
            .ok_or_else(|| {
                WorkflowError::Knowledge(format!(
                    "no Qdrant agent configured for collection `{}`",
                    group.group.collection
                ))
            })
    }
}

impl<R: 'static> KnowledgeBackend for QdrantKnowledgeBackend<R> {
    fn initialise_group<'a>(
        &'a self,
        group: &'a MaterialisedKnowledgeGroup,
    ) -> LocalBoxFuture<'a, Result<(), WorkflowError>> {
        let agent = self.agent_for(group);
        Box::pin(async move {
            let agent = agent?;
            agent
                .init_collection()
                .await
                .map_err(|error| WorkflowError::Knowledge(error.to_string()))
        })
    }

    fn ingest_source<'a>(
        &'a self,
        group: &'a MaterialisedKnowledgeGroup,
        source: &'a KnowledgeSource,
    ) -> LocalBoxFuture<'a, Result<(), WorkflowError>> {
        let repo = self.repo.clone();
        let workspace_root = self.workspace_root.clone();
        let agent = self.agent_for(group);
        Box::pin(async move {
            let agent = agent?;
            match source.kind {
                KnowledgeSourceKind::RepositoryPath => {
                    let workspace_root = workspace_root.as_ref().ok_or_else(|| {
                        WorkflowError::Knowledge(
                            "repository path ingestion requires a configured workspace root"
                                .to_string(),
                        )
                    })?;
                    let path = resolve_repository_path(
                        workspace_root,
                        source.location.clone().ok_or_else(|| {
                            WorkflowError::Knowledge(
                                "repository path source is missing a location".to_string(),
                            )
                        })?,
                    )?;

                    if source.recursive {
                        let _report = ingest_directory(agent.as_ref(), &path, repo.as_deref())
                            .await
                            .map_err(|error| WorkflowError::Knowledge(error.to_string()))?;
                    } else {
                        let _report = ingest_file(agent.as_ref(), &path, repo.as_deref())
                            .await
                            .map_err(|error| WorkflowError::Knowledge(error.to_string()))?;
                    }
                }
                _ => {
                    let source_info = source.inline_source_info().ok_or_else(|| {
                        WorkflowError::Knowledge(
                            "source cannot be converted to inline knowledge".to_string(),
                        )
                    })?;
                    let content = source.content.clone().unwrap_or_default();
                    let _report =
                        ingest_content(agent.as_ref(), &content, &source_info, repo.as_deref())
                            .await
                            .map_err(|error| WorkflowError::Knowledge(error.to_string()))?;
                }
            }

            Ok(())
        })
    }
}

pub fn build_knowledge_planning_prompt(input: &KnowledgePlanningInput) -> String {
    let state = &input.discovery.state;
    let mut lines = vec![
        "Plan the minimum useful knowledge groups for downstream MMAT stages.".to_string(),
        format!("Planning turn: {}", input.turn + 1),
        format!("Problem statement: {}", state.problem_statement),
        format!("Recommended path: {}", state.recommended_path),
    ];

    if !state.goals.is_empty() {
        lines.push(format!("Goals: {}", state.goals.join(" | ")));
    }

    if !state.constraints.is_empty() {
        lines.push(format!("Constraints: {}", state.constraints.join(" | ")));
    }

    if !input.discovery.answers.is_empty() {
        lines.push("Discovery answers: ".to_string());
        lines.extend(
            input
                .discovery
                .answers
                .iter()
                .map(|answer| format!("- {} => {}", answer.question, answer.answer)),
        );
    }

    if let Some(prior_plan) = &input.prior_plan {
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
                .map(|finding| format!("- {}", finding.description())),
        );
    }

    lines.push(
        "Only propose knowledge groups that a later stage will actually need, and record any platform-level NAAF follow-up work explicitly."
            .to_string(),
    );
    lines.join("\n")
}

fn validate_knowledge_plan(turn: &KnowledgePlanningTurn) -> Vec<KnowledgePlanningFinding> {
    let mut findings = Vec::new();
    let plan = &turn.plan;

    if plan.groups.is_empty() {
        findings.push(KnowledgePlanningFinding::EmptyPlan);
    }

    let mut seen_collections = BTreeSet::new();
    let mut covered_stages = BTreeSet::new();
    let has_discovery_transcript = plan
        .groups
        .iter()
        .flat_map(|group| group.sources.iter())
        .any(|source| matches!(source.kind, KnowledgeSourceKind::DiscoveryTranscript));

    for group in &plan.groups {
        let collection = sanitise_collection_name(group.template.slug(), &group.instance_name);
        if !seen_collections.insert(collection.clone()) {
            findings.push(KnowledgePlanningFinding::DuplicateCollectionName(
                collection,
            ));
        }

        if group.stages.is_empty() {
            findings.push(KnowledgePlanningFinding::GroupWithoutStages(
                group.instance_name.clone(),
            ));
        } else {
            covered_stages.extend(group.stages.iter().copied());
        }

        if group.sources.is_empty() {
            findings.push(KnowledgePlanningFinding::GroupWithoutSources(
                group.instance_name.clone(),
            ));
        }
    }

    if !turn.input.discovery.answers.is_empty() && !has_discovery_transcript {
        findings.push(KnowledgePlanningFinding::MissingDiscoveryTranscript);
    }

    for stage in [
        WorkflowStageId::Solutions,
        WorkflowStageId::SoftwareArchitect,
    ] {
        if !covered_stages.contains(&stage) {
            findings.push(KnowledgePlanningFinding::MissingStageCoverage(stage));
        }
    }

    findings
}

fn plan_next_knowledge_input<R>(
    _runtime: &R,
    attempts: Vec<Attempt<KnowledgePlanningInput, KnowledgePlanningTurn, KnowledgePlanningFinding>>,
) -> LocalBoxFuture<'_, Result<KnowledgePlanningInput, WorkflowError>>
where
    R: 'static,
{
    Box::pin(async move {
        let latest_attempt = attempts
            .last()
            .expect("knowledge planning repair requires an attempt");
        Ok(KnowledgePlanningInput {
            discovery: latest_attempt.artefact.input.discovery.clone(),
            turn: latest_attempt.artefact.input.turn + 1,
            findings: latest_attempt.findings.clone(),
            prior_plan: Some(latest_attempt.artefact.plan.clone()),
        })
    })
}

pub fn build_knowledge_planning_step<R: 'static, A>(
    agent: Arc<A>,
    retry_policy: RetryPolicy,
) -> Step<R, KnowledgePlanningInput, KnowledgePlanningTurn, KnowledgePlanningFinding, WorkflowError>
where
    A: KnowledgePlanningAgent<R>,
{
    Step::builder(
        task_fn(move |runtime: &R, input: KnowledgePlanningInput| {
            let prompt = build_knowledge_planning_prompt(&input);
            let agent = agent.clone();
            Box::pin(async move {
                let plan = agent.plan(runtime, input.clone(), prompt).await?;
                Ok(KnowledgePlanningTurn { input, plan })
            })
        })
        .observed_as("knowledge_planning"),
    )
    .validate(check_fn(|_runtime: &R, turn: KnowledgePlanningTurn| {
        Box::pin(async move { Ok(validate_knowledge_plan(&turn)) })
    }))
    .repair_with(repair_fn(|runtime: &R, attempts| {
        plan_next_knowledge_input(runtime, attempts)
    }))
    .retry_policy(retry_policy)
    .build()
}

pub fn build_materialisation_step<R: 'static, B>(
    store: Arc<SqliteKnowledgeGroupStore>,
    backend: Arc<B>,
) -> Step<R, KnowledgePlan, Vec<MaterialisedKnowledgeGroup>, (), WorkflowError>
where
    B: KnowledgeBackend,
{
    Step::builder(
        task_fn(move |_runtime: &R, plan: KnowledgePlan| {
            let store = store.clone();
            let backend = backend.clone();
            Box::pin(async move {
                materialise_knowledge_plan(store.as_ref(), backend.as_ref(), &plan).await
            })
        })
        .observed_as("knowledge_materialisation"),
    )
    .with_findings::<()>()
    .build()
}

pub fn build_stage_knowledge_session<R>(
    runtime: &R,
    stage: WorkflowStageId,
    materialised: &[MaterialisedKnowledgeGroup],
) -> StageKnowledgeSession
where
    R: StagePromptProvider,
{
    let groups = scoped_groups_for_stage(materialised, stage);
    let system_prompt = augment_system_prompt(
        &runtime.system_prompt_for_stage(stage),
        &groups,
        &KnowledgePromptConfig::default(),
    );

    StageKnowledgeSession {
        stage,
        system_prompt,
        group_collections: groups.into_iter().map(|group| group.collection).collect(),
    }
}

pub async fn materialise_knowledge_plan<B>(
    store: &SqliteKnowledgeGroupStore,
    backend: &B,
    plan: &KnowledgePlan,
) -> Result<Vec<MaterialisedKnowledgeGroup>, WorkflowError>
where
    B: KnowledgeBackend,
{
    let mut materialised = Vec::new();
    let mut seen_collections = BTreeSet::new();

    for group_plan in &plan.groups {
        let collection =
            sanitise_collection_name(group_plan.template.slug(), &group_plan.instance_name);
        if !seen_collections.insert(collection.clone()) {
            return Err(WorkflowError::Knowledge(format!(
                "knowledge plan generated a colliding collection name `{collection}`"
            )));
        }
        let previous_group = store
            .load_group(&collection)
            .await
            .map_err(WorkflowError::from)?;
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

        store
            .upsert_group(&group)
            .await
            .map_err(WorkflowError::from)?;

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

            Ok::<(), WorkflowError>(())
        }
        .await;

        if let Err(error) = materialisation_result {
            match previous_group {
                Some(ref previous_group) => {
                    store
                        .upsert_group(previous_group)
                        .await
                        .map_err(WorkflowError::from)?;
                }
                None => {
                    store
                        .delete_group(&materialised_group.group.collection)
                        .await
                        .map_err(WorkflowError::from)?;
                }
            }
            return Err(error);
        }

        materialised.push(materialised_group);
    }

    Ok(materialised)
}

pub fn scoped_groups_for_stage(
    materialised: &[MaterialisedKnowledgeGroup],
    stage: WorkflowStageId,
) -> Vec<KnowledgeGroup> {
    materialised
        .iter()
        .filter(|group| group.stages.contains(&stage))
        .map(|group| group.group.clone())
        .collect()
}

fn resolve_repository_path(
    workspace_root: &Path,
    location: String,
) -> Result<PathBuf, WorkflowError> {
    let requested = PathBuf::from(location);
    if requested.is_absolute() {
        return Err(WorkflowError::Knowledge(
            "repository knowledge sources must stay beneath the workspace root".to_string(),
        ));
    }
    if requested
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(WorkflowError::Knowledge(
            "repository knowledge sources must stay beneath the workspace root".to_string(),
        ));
    }

    let candidate = workspace_root.join(requested);
    let canonical_root = workspace_root.canonicalize()?;
    let canonical_candidate = candidate.canonicalize()?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(WorkflowError::Knowledge(
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use parking_lot::Mutex;

    use crate::runtime::ScriptedRuntime;

    use super::*;

    #[derive(Default)]
    struct StubKnowledgeBackend {
        initialised: Mutex<Vec<String>>,
        ingested: Mutex<Vec<(String, String)>>,
    }

    impl StubKnowledgeBackend {
        fn initialised(&self) -> Vec<String> {
            self.initialised.lock().clone()
        }

        fn ingested(&self) -> Vec<(String, String)> {
            self.ingested.lock().clone()
        }
    }

    impl KnowledgeBackend for StubKnowledgeBackend {
        fn initialise_group<'a>(
            &'a self,
            group: &'a MaterialisedKnowledgeGroup,
        ) -> LocalBoxFuture<'a, Result<(), WorkflowError>> {
            self.initialised.lock().push(group.group.collection.clone());
            Box::pin(async move { Ok(()) })
        }

        fn ingest_source<'a>(
            &'a self,
            group: &'a MaterialisedKnowledgeGroup,
            source: &'a KnowledgeSource,
        ) -> LocalBoxFuture<'a, Result<(), WorkflowError>> {
            self.ingested
                .lock()
                .push((group.group.collection.clone(), source.label.clone()));
            Box::pin(async move { Ok(()) })
        }
    }

    struct StubKnowledgePlanner {
        plans: Mutex<VecDeque<KnowledgePlan>>,
        prompts: Mutex<Vec<String>>,
    }

    impl StubKnowledgePlanner {
        fn new(plans: Vec<KnowledgePlan>) -> Self {
            Self {
                plans: Mutex::new(plans.into()),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().clone()
        }
    }

    impl KnowledgePlanningAgent<ScriptedRuntime> for StubKnowledgePlanner {
        fn plan<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            _input: KnowledgePlanningInput,
            prompt: String,
        ) -> LocalBoxFuture<'a, Result<KnowledgePlan, WorkflowError>> {
            self.prompts.lock().push(prompt);
            let plan = self
                .plans
                .lock()
                .pop_front()
                .expect("stub knowledge plan should exist");
            Box::pin(async move { Ok(plan) })
        }
    }

    fn sample_discovery() -> DiscoveryOutcome {
        DiscoveryOutcome {
            state: crate::workflow_old::discovery::DiscoveryState {
                ready_for_solution: true,
                problem_statement: "Rewrite MMAT".to_string(),
                goals: vec!["Keep stages readable".to_string()],
                constraints: vec!["Use SQLite".to_string()],
                assumptions: vec!["Live-only questions are fine".to_string()],
                risks: vec!["NAAF gaps remain".to_string()],
                notes: Vec::new(),
                recommended_path: "Plan knowledge, then branch".to_string(),
                open_questions: Vec::new(),
            },
            answers: vec![crate::workflow_old::discovery::DiscoveryAnswer {
                question: "How should knowledge metadata be stored?".to_string(),
                answer: "SQLite".to_string(),
            }],
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

    #[tokio::test]
    async fn materialisation_persists_groups_and_ingests_sources() {
        let store = SqliteKnowledgeGroupStore::open_in_memory().expect("SQLite store should open");
        let backend = StubKnowledgeBackend::default();
        let plan = KnowledgePlan {
            groups: vec![KnowledgeGroupPlan {
                template: KnowledgeGroupTemplate::DiscoveryTranscript,
                instance_name: "rewrite-answers".to_string(),
                description: "Discovery clarifications for the rewrite".to_string(),
                tags: vec!["rewrite".to_string()],
                query_hints: vec!["Use the user's constraints directly".to_string()],
                stages: vec![
                    WorkflowStageId::Solutions,
                    WorkflowStageId::SoftwareArchitect,
                ],
                sources: vec![KnowledgeSource::discovery_transcript(
                    "Discovery transcript",
                    "User chose SQLite for metadata.",
                )],
            }],
            upstream_follow_ups: UPSTREAM_NAAF_FOLLOW_UPS
                .iter()
                .map(ToString::to_string)
                .collect(),
        };

        let materialised = materialise_knowledge_plan(&store, &backend, &plan)
            .await
            .expect("knowledge plan should materialise");

        assert_eq!(materialised.len(), 1);
        assert_eq!(materialised[0].ingested_sources, 1);
        assert_eq!(
            backend.initialised(),
            vec![materialised[0].group.collection.clone()]
        );
        assert_eq!(backend.ingested().len(), 1);
        let stored = store
            .load_group(&materialised[0].group.collection)
            .await
            .expect("group load should succeed")
            .expect("group should exist");
        assert_eq!(stored.metadata["template"], json!("discovery-transcript"));
    }

    #[tokio::test]
    async fn stage_session_uses_only_groups_scoped_to_that_stage() {
        let runtime = ScriptedRuntime::new(std::iter::empty::<String>())
            .with_stage_prompt(WorkflowStageId::Solutions, "Solutions stage prompt")
            .with_stage_prompt(WorkflowStageId::SoftwareArchitect, "Architect stage prompt");
        let store = SqliteKnowledgeGroupStore::open_in_memory().expect("SQLite store should open");
        let backend = StubKnowledgeBackend::default();
        let plan = KnowledgePlan {
            groups: vec![
                KnowledgeGroupPlan {
                    template: KnowledgeGroupTemplate::WorkspaceCode,
                    instance_name: "repo".to_string(),
                    description: String::new(),
                    tags: vec!["code".to_string()],
                    query_hints: vec![],
                    stages: vec![WorkflowStageId::Solutions],
                    sources: vec![KnowledgeSource::inline_markdown(
                        "Repo summary",
                        "Code facts",
                    )],
                },
                KnowledgeGroupPlan {
                    template: KnowledgeGroupTemplate::Papers,
                    instance_name: "research".to_string(),
                    description: String::new(),
                    tags: vec!["paper".to_string()],
                    query_hints: vec![],
                    stages: vec![WorkflowStageId::SoftwareArchitect],
                    sources: vec![KnowledgeSource::research_paper(
                        "Research note",
                        "Paper facts",
                    )],
                },
            ],
            upstream_follow_ups: Vec::new(),
        };

        let materialised = materialise_knowledge_plan(&store, &backend, &plan)
            .await
            .expect("knowledge should materialise");
        let solutions_session =
            build_stage_knowledge_session(&runtime, WorkflowStageId::Solutions, &materialised);
        let architect_session = build_stage_knowledge_session(
            &runtime,
            WorkflowStageId::SoftwareArchitect,
            &materialised,
        );

        assert_eq!(solutions_session.group_collections.len(), 1);
        assert!(
            solutions_session
                .system_prompt
                .contains("Solutions stage prompt")
        );
        assert!(
            architect_session
                .system_prompt
                .contains("Architect stage prompt")
        );
        assert_ne!(
            solutions_session.group_collections,
            architect_session.group_collections
        );
    }

    #[test]
    fn knowledge_planning_prompt_records_discovery_context() {
        let prompt =
            build_knowledge_planning_prompt(&KnowledgePlanningInput::new(sample_discovery()));

        assert!(prompt.contains("Rewrite MMAT"));
        assert!(prompt.contains("Use SQLite"));
        assert!(prompt.contains("How should knowledge metadata be stored? => SQLite"));
    }

    #[tokio::test]
    async fn knowledge_planning_step_retries_with_validation_findings() {
        let runtime = ScriptedRuntime::new(std::iter::empty::<String>());
        let planner = Arc::new(StubKnowledgePlanner::new(vec![
            KnowledgePlan {
                groups: vec![KnowledgeGroupPlan {
                    template: KnowledgeGroupTemplate::WorkspaceCode,
                    instance_name: "repo".to_string(),
                    description: String::new(),
                    tags: vec!["code".to_string()],
                    query_hints: vec![],
                    stages: vec![WorkflowStageId::Solutions],
                    sources: vec![KnowledgeSource::inline_markdown(
                        "Repo summary",
                        "Workflow code facts",
                    )],
                }],
                upstream_follow_ups: Vec::new(),
            },
            valid_knowledge_plan(),
        ]));
        let step = build_knowledge_planning_step(planner.clone(), RetryPolicy::new(3));

        let traced = step
            .run_traced(&runtime, KnowledgePlanningInput::new(sample_discovery()))
            .await
            .expect("knowledge planning should recover");

        assert_eq!(traced.report().attempt_count(), 2);
        assert_eq!(
            traced.report().attempts()[0].findings,
            vec![
                KnowledgePlanningFinding::MissingDiscoveryTranscript,
                KnowledgePlanningFinding::MissingStageCoverage(WorkflowStageId::SoftwareArchitect,),
            ]
        );
        assert!(traced.report().attempts()[1].accepted());
        assert!(
            planner
                .prompts()
                .last()
                .expect("second planning prompt should exist")
                .contains("knowledge plan omitted a discovery transcript")
        );
    }

    #[tokio::test]
    async fn materialisation_rejects_colliding_collection_names() {
        let store = SqliteKnowledgeGroupStore::open_in_memory().expect("SQLite store should open");
        let backend = StubKnowledgeBackend::default();
        let error = materialise_knowledge_plan(
            &store,
            &backend,
            &KnowledgePlan {
                groups: vec![
                    KnowledgeGroupPlan {
                        template: KnowledgeGroupTemplate::WorkspaceDocs,
                        instance_name: "foo/bar".to_string(),
                        description: String::new(),
                        tags: vec![],
                        query_hints: vec![],
                        stages: vec![WorkflowStageId::Solutions],
                        sources: vec![],
                    },
                    KnowledgeGroupPlan {
                        template: KnowledgeGroupTemplate::WorkspaceDocs,
                        instance_name: "foo bar".to_string(),
                        description: String::new(),
                        tags: vec![],
                        query_hints: vec![],
                        stages: vec![WorkflowStageId::Solutions],
                        sources: vec![],
                    },
                ],
                upstream_follow_ups: vec![],
            },
        )
        .await
        .expect_err("colliding collection names should fail");

        assert!(error.to_string().contains("colliding collection name"));
    }

    #[derive(Default)]
    struct FailingKnowledgeBackend;

    impl KnowledgeBackend for FailingKnowledgeBackend {
        fn initialise_group<'a>(
            &'a self,
            _group: &'a MaterialisedKnowledgeGroup,
        ) -> LocalBoxFuture<'a, Result<(), WorkflowError>> {
            Box::pin(async move { Ok(()) })
        }

        fn ingest_source<'a>(
            &'a self,
            _group: &'a MaterialisedKnowledgeGroup,
            _source: &'a KnowledgeSource,
        ) -> LocalBoxFuture<'a, Result<(), WorkflowError>> {
            Box::pin(async move {
                Err(WorkflowError::Knowledge(
                    "simulated ingestion failure".to_string(),
                ))
            })
        }
    }

    #[tokio::test]
    async fn materialisation_rolls_back_group_metadata_on_failure() {
        let store = SqliteKnowledgeGroupStore::open_in_memory().expect("SQLite store should open");
        let error = materialise_knowledge_plan(
            &store,
            &FailingKnowledgeBackend,
            &KnowledgePlan {
                groups: vec![KnowledgeGroupPlan {
                    template: KnowledgeGroupTemplate::DiscoveryTranscript,
                    instance_name: "failing-group".to_string(),
                    description: String::new(),
                    tags: vec![],
                    query_hints: vec![],
                    stages: vec![WorkflowStageId::Solutions],
                    sources: vec![KnowledgeSource::discovery_transcript(
                        "Discovery transcript",
                        "This ingest will fail",
                    )],
                }],
                upstream_follow_ups: vec![],
            },
        )
        .await
        .expect_err("materialisation should fail");

        assert!(error.to_string().contains("simulated ingestion failure"));
        assert!(
            store
                .list_groups()
                .await
                .expect("listing groups should succeed")
                .is_empty(),
            "failed materialisation should not leave stored groups behind"
        );
    }

    #[test]
    fn repository_path_resolution_rejects_escape_attempts() {
        let root = std::env::temp_dir().join(format!("mmat-knowledge-root-{}", std::process::id()));
        std::fs::create_dir_all(root.join("allowed")).expect("temp root should be created");
        std::fs::write(root.join("allowed").join("note.md"), "hello")
            .expect("temp file should be created");

        let inside = resolve_repository_path(&root, "allowed/note.md".to_string())
            .expect("workspace file should be accepted");
        assert!(inside.starts_with(root.canonicalize().expect("root should canonicalise")));

        let escape = resolve_repository_path(&root, "../outside.md".to_string())
            .expect_err("path traversal should be rejected");
        assert!(escape.to_string().contains("workspace root"));

        std::fs::remove_dir_all(root).expect("temp root should be removed");
    }
}
