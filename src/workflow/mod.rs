use std::{
    convert::Infallible,
    fmt::{Debug, Display},
    sync::Arc,
};

use naaf_core::{Step, StepReport, task_fn};
use naaf_llm::{
    HumanIO, LlmAgent, LlmClient, OpenAiClient, OpenAiConfig, OpenAiStreamObserver, TaskError,
};
use naaf_persistence_sqlite::SqliteKnowledgeGroupStore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::MmatError;

mod architect;
mod discovery;
mod knowledge;
mod parser;
mod solutions;

type WorkflowTaskError<C, R, E> = TaskError<
    WorkflowBuildError<<R as HumanIO>::Error>,
    <C as LlmClient>::Error,
    E,
    serde_json::Error,
>;
type WorkflowStep<C, R, E, I, O> = Step<R, I, O, WorkflowFinding, WorkflowTaskError<C, R, E>>;

#[derive(Debug, Error)]
enum WorkflowBuildError<H> {
    #[error("human interaction failed: {0}")]
    Human(H),
    #[error(transparent)]
    Knowledge(#[from] knowledge::KnowledgeError),
    #[error("invalid solution choice: {0}")]
    InvalidChoice(String),
    #[error("workflow step failed: {0}")]
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

pub async fn greenfield<R>(
    init_prompt: String,
    runtime: R,
    stream_observer: Option<Arc<dyn OpenAiStreamObserver<R>>>,
) -> Result<GreenfieldReport, MmatError>
where
    R: HumanIO + 'static,
    R::Error: Debug + Display + 'static,
{
    let cfg = OpenAiConfig::new("").with_base_url("http://127.0.0.1:1234/v1");
    let mut oai_client = OpenAiClient::<R>::new(cfg);
    if let Some(stream_observer) = stream_observer {
        oai_client = oai_client.with_stream_observer(stream_observer);
    }
    let agent = LlmAgent::new(oai_client);
    let knowledge_store = Arc::new(
        SqliteKnowledgeGroupStore::open_in_memory()
            .map_err(|error| MmatError::Workflow(error.to_string()))?,
    );
    let knowledge_backend = Arc::new(knowledge::MetadataOnlyKnowledgeBackend);
    let workflow = build_greenfield_step::<OpenAiClient<R>, R, Infallible, _>(
        &agent,
        knowledge_store,
        knowledge_backend,
    );

    let traced = workflow
        .run_traced(&runtime, discovery::DiscoveryInput::new(init_prompt))
        .await
        .map_err(|error| MmatError::Workflow(error.to_string()))?;
    let (result, step_report) = traced.into_parts();

    Ok(GreenfieldReport {
        run_id: uuid::Uuid::new_v4(),
        result,
        step_report,
    })
}

pub struct GreenfieldReport {
    run_id: uuid::Uuid,
    result: WorkflowRunResult,
    step_report: StepReport<WorkflowFinding>,
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum WorkflowRunResult {
    ReadyForPlanning {
        architect_plan: architect::ArchitectPlan,
    },
    NeedsRevision {
        feedback: String,
    },
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

fn build_greenfield_step<C, R, E, B>(
    agent: &LlmAgent<C, R, E>,
    knowledge_store: Arc<SqliteKnowledgeGroupStore>,
    knowledge_backend: Arc<B>,
) -> WorkflowStep<C, R, E, discovery::DiscoveryInput, WorkflowRunResult>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + Display + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + Display + 'static,
    E: Debug + Display + 'static,
    B: knowledge::KnowledgeBackend,
{
    let discovery = discovery::step(agent).map_findings(WorkflowFinding::from);
    let knowledge = knowledge::step(agent)
        .map_input(knowledge::KnowledgeInput::new)
        .map_findings(WorkflowFinding::from);
    let knowledge_context = knowledge
        .with_input()
        .map(|(discovery, plan)| KnowledgeMaterialisationInput { discovery, plan });

    let materialisation =
        knowledge::materialisation_step::<C, R, E, B>(knowledge_store, knowledge_backend)
            .map_input(|input: KnowledgeMaterialisationInput| input.plan)
            .map_with_input(|input, materialised| KnowledgeMaterialisationOutput {
                discovery: input.discovery,
                materialised,
            })
            .map_findings(WorkflowFinding::from);

    let solution_branches = solution_branch_step::<C, R, E>(
        solutions::branch_step(agent),
        solutions::SolutionBranch::Conservative,
    )
    .join(solution_branch_step::<C, R, E>(
        solutions::branch_step(agent),
        solutions::SolutionBranch::Recommended,
    ))
    .join(solution_branch_step::<C, R, E>(
        solutions::branch_step(agent),
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

    let architect = architect::step(agent)
        .map_input(architect_input_from_stage)
        .map_findings(WorkflowFinding::from);

    discovery
        .then(knowledge_context)
        .then(materialisation)
        .then(collection_context)
        .then(choice_context)
        .then(finalise_choice_step::<C, R, E>(architect))
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

fn architect_input_from_stage(input: ArchitectStageInput) -> architect::ArchitectInput {
    let knowledge = knowledge::build_stage_knowledge_session(
        WorkflowStageId::SoftwareArchitect,
        WorkflowStageId::SoftwareArchitect.default_system_prompt(),
        &input.materialised,
    );
    architect::ArchitectInput::new(input.discovery, input.selected_solution, knowledge)
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
                                    materialised: context.materialised,
                                },
                            )
                            .await
                            .map_err(|error| {
                                TaskError::Build(WorkflowBuildError::Workflow(error.to_string()))
                            })?;
                        Ok(WorkflowRunResult::ReadyForPlanning {
                            architect_plan: plan,
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
