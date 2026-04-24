use std::{fmt::Display, sync::Arc};

use naaf_core::{
    EdgeSpec, GraphPatch, NodeId, NodeInput, NodeSpec, RetryPolicy, Step, StepNode, TaskExt,
    Workflow as GraphWorkflow, WorkflowError as GraphExecutionError,
    WorkflowRunReport as GraphWorkflowRunReport, task_fn,
};
use naaf_llm::HumanIO;
use naaf_persistence_sqlite::SqliteKnowledgeGroupStore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use self::{
    architect::{
        ArchitectAgent, ArchitectFinding, ArchitectInput, ArchitectPlan, ArchitectTurn,
        ArchitectTurnInput,
    },
    discovery::{
        DiscoveryInput, DiscoveryOutcome, DiscoveryTurn, DiscoveryTurnAgent, build_turn_step,
    },
    knowledge::{
        KnowledgeBackend, KnowledgePlan, KnowledgePlanningAgent, KnowledgePlanningFinding,
        KnowledgePlanningInput, KnowledgePlanningTurn, MaterialisedKnowledgeGroup,
        build_knowledge_planning_step, build_materialisation_step, build_stage_knowledge_session,
    },
    solutions::{
        SelectedSolution, SolutionBranchAgent, SolutionCollectAgent, SolutionCollectFinding,
        SolutionCollectInput, SolutionCollectTurn, SolutionCollection, SolutionInput,
        SolutionUserChoice, build_choice_step, build_collect_step, build_solution_generation_step,
    },
};
use crate::runtime::WorkflowRuntime;

pub mod architect;
pub mod discovery;
pub mod knowledge;
pub mod solutions;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowRunResult {
    ReadyForPlanning {
        planning: Box<PlanningBoundaryInput>,
        collected_solutions: SolutionCollection,
    },
    NeedsRevision {
        collected_solutions: SolutionCollection,
        feedback: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum WorkflowStageId {
    Discovery,
    KnowledgePlanning,
    KnowledgeMaterialisation,
    Solutions,
    SolutionSelection,
    SoftwareArchitect,
    ImplementationPlanning,
    Execution,
}

#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("human interaction failed: {0}")]
    Human(String),
    #[error("workflow failed: {0}")]
    Workflow(String),
    #[error("discovery failed: {0}")]
    Discovery(String),
    #[error("knowledge failed: {0}")]
    Knowledge(String),
    #[error("solution failed: {0}")]
    Solution(String),
    #[error("architect failed: {0}")]
    Architect(String),
    #[error("invalid user choice: {0}")]
    InvalidChoice(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialisation error: {0}")]
    Serialisation(#[from] serde_json::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunInput {
    pub prompt: String,
    pub max_discovery_turns: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningBoundaryInput {
    pub discovery: DiscoveryOutcome,
    pub knowledge_plan: KnowledgePlan,
    pub materialised_knowledge: Vec<MaterialisedKnowledgeGroup>,
    pub selected_solution: SelectedSolution,
    pub architect_plan: ArchitectPlan,
}

pub struct WorkflowRunOutput {
    pub result: WorkflowRunResult,
    pub report: GraphWorkflowRunReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SolutionsStageInput {
    discovery: DiscoveryOutcome,
    materialised_knowledge: Vec<MaterialisedKnowledgeGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ArchitectStageInput {
    discovery: DiscoveryOutcome,
    selected_solution: SelectedSolution,
    materialised_knowledge: Vec<MaterialisedKnowledgeGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ReadyWorkflowResultInput {
    planning: PlanningBoundaryInput,
    collected_solutions: SolutionCollection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RevisionWorkflowResultInput {
    collected_solutions: SolutionCollection,
    feedback: String,
}

pub struct SubjectWorkflow<R, D, B> {
    discovery_agent: Arc<D>,
    knowledge_planning_step: Step<
        R,
        KnowledgePlanningInput,
        KnowledgePlanningTurn,
        KnowledgePlanningFinding,
        WorkflowError,
    >,
    knowledge_materialisation_step:
        Step<R, KnowledgePlan, Vec<MaterialisedKnowledgeGroup>, (), WorkflowError>,
    solution_generation_step:
        Step<R, SolutionInput, Vec<solutions::SolutionDraft>, (), WorkflowError>,
    solution_collect_step:
        Step<R, SolutionCollectInput, SolutionCollectTurn, SolutionCollectFinding, WorkflowError>,
    solution_choice_step: Step<R, SolutionCollection, SolutionUserChoice, (), WorkflowError>,
    architect_step: Step<R, ArchitectTurnInput, ArchitectTurn, ArchitectFinding, WorkflowError>,
    prepare_solutions_step: Step<R, SolutionsStageInput, SolutionInput, (), WorkflowError>,
    prepare_architect_step: Step<R, ArchitectStageInput, ArchitectTurnInput, (), WorkflowError>,
    ready_result_step: Step<R, ReadyWorkflowResultInput, WorkflowRunResult, (), WorkflowError>,
    revision_result_step:
        Step<R, RevisionWorkflowResultInput, WorkflowRunResult, (), WorkflowError>,
    max_discovery_turns: usize,
    _backend: Arc<B>,
}

pub struct SubjectWorkflowDependencies<D, K, S, C, A, B> {
    pub discovery_agent: Arc<D>,
    pub knowledge_planner: Arc<K>,
    pub knowledge_store: Arc<SqliteKnowledgeGroupStore>,
    pub knowledge_backend: Arc<B>,
    pub solution_agent: Arc<S>,
    pub collect_agent: Arc<C>,
    pub architect_agent: Arc<A>,
    pub max_discovery_turns: usize,
}

impl WorkflowStageId {
    pub fn as_str(self) -> &'static str {
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

    pub fn default_system_prompt(self) -> String {
        match self {
            Self::Discovery => "You are the discovery stage for MMAT.".to_string(),
            Self::KnowledgePlanning => "You are the knowledge planning stage for MMAT.".to_string(),
            Self::KnowledgeMaterialisation => {
                "You are the knowledge materialisation stage for MMAT.".to_string()
            }
            Self::Solutions => "You are the solution generation stage for MMAT.".to_string(),
            Self::SolutionSelection => "You are the solution selection stage for MMAT.".to_string(),
            Self::SoftwareArchitect => {
                "You are the downstream Software Architect stage for MMAT.".to_string()
            }
            Self::ImplementationPlanning => {
                "You are the implementation planning stage for MMAT.".to_string()
            }
            Self::Execution => "You are the execution stage for MMAT.".to_string(),
        }
    }
}

impl Display for WorkflowStageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<Box<dyn std::error::Error + Send + Sync + 'static>> for WorkflowError {
    fn from(error: Box<dyn std::error::Error + Send + Sync + 'static>) -> Self {
        Self::Knowledge(error.to_string())
    }
}

impl RunInput {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            max_discovery_turns: 6,
        }
    }
}

fn discovery_outcome_from_turn(turn: DiscoveryTurn) -> DiscoveryOutcome {
    DiscoveryOutcome {
        state: turn.state,
        answers: turn.input.answers,
    }
}

fn build_prepare_solutions_step<R>()
-> Step<R, SolutionsStageInput, SolutionInput, (), WorkflowError>
where
    R: WorkflowRuntime + 'static,
{
    Step::builder(
        task_fn(move |runtime: &R, input: SolutionsStageInput| {
            let knowledge = build_stage_knowledge_session(
                runtime,
                WorkflowStageId::Solutions,
                &input.materialised_knowledge,
            );
            Box::pin(async move { Ok(SolutionInput::new(input.discovery, knowledge)) })
        })
        .observed_as("prepare_solutions_input"),
    )
    .with_findings::<()>()
    .build()
}

fn build_prepare_architect_step<R>()
-> Step<R, ArchitectStageInput, ArchitectTurnInput, (), WorkflowError>
where
    R: WorkflowRuntime + 'static,
{
    Step::builder(
        task_fn(move |runtime: &R, input: ArchitectStageInput| {
            let knowledge = build_stage_knowledge_session(
                runtime,
                WorkflowStageId::SoftwareArchitect,
                &input.materialised_knowledge,
            );
            Box::pin(async move {
                Ok(ArchitectTurnInput::new(ArchitectInput::new(
                    input.discovery,
                    input.selected_solution,
                    knowledge,
                )))
            })
        })
        .observed_as("prepare_architect_input"),
    )
    .with_findings::<()>()
    .build()
}

fn build_ready_result_step<R>()
-> Step<R, ReadyWorkflowResultInput, WorkflowRunResult, (), WorkflowError>
where
    R: 'static,
{
    Step::builder(
        task_fn(move |_runtime: &R, input: ReadyWorkflowResultInput| {
            Box::pin(async move {
                Ok(WorkflowRunResult::ReadyForPlanning {
                    planning: Box::new(input.planning),
                    collected_solutions: input.collected_solutions,
                })
            })
        })
        .observed_as("workflow_result_ready"),
    )
    .with_findings::<()>()
    .build()
}

fn build_revision_result_step<R>()
-> Step<R, RevisionWorkflowResultInput, WorkflowRunResult, (), WorkflowError>
where
    R: 'static,
{
    Step::builder(
        task_fn(move |_runtime: &R, input: RevisionWorkflowResultInput| {
            Box::pin(async move {
                Ok(WorkflowRunResult::NeedsRevision {
                    collected_solutions: input.collected_solutions,
                    feedback: input.feedback,
                })
            })
        })
        .observed_as("workflow_result_revision"),
    )
    .with_findings::<()>()
    .build()
}

fn map_graph_error(error: GraphExecutionError<WorkflowError>) -> WorkflowError {
    match error {
        GraphExecutionError::InvalidPatch(error) => {
            WorkflowError::Workflow(format!("invalid workflow graph patch: {error}"))
        }
        GraphExecutionError::Node { node_id, error } => match error {
            naaf_core::NodeExecutionError::System(error) => error,
            naaf_core::NodeExecutionError::StepSystem { error, .. } => error,
            other => WorkflowError::Workflow(format!("node `{node_id}` failed: {other}")),
        },
        GraphExecutionError::Stalled { pending } => WorkflowError::Workflow(format!(
            "workflow graph stalled with pending nodes: {}",
            pending
                .into_iter()
                .map(|node_id| node_id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn extract_graph_result(
    report: &GraphWorkflowRunReport,
) -> Result<WorkflowRunResult, WorkflowError> {
    let mut results = report
        .nodes()
        .values()
        .filter(|node| {
            matches!(
                node.name(),
                "workflow_result_ready" | "workflow_result_revision"
            )
        })
        .map(|node| serde_json::from_value::<WorkflowRunResult>(node.output().clone()))
        .collect::<Result<Vec<_>, _>>()?;

    match results.len() {
        1 => Ok(results.pop().expect("single workflow result should exist")),
        0 => Err(WorkflowError::Workflow(
            "workflow graph completed without producing a final result node".to_string(),
        )),
        _ => Err(WorkflowError::Workflow(
            "workflow graph produced multiple final result nodes".to_string(),
        )),
    }
}

impl<R, D, B> SubjectWorkflow<R, D, B>
where
    R: WorkflowRuntime + HumanIO<Error = WorkflowError> + 'static,
    D: DiscoveryTurnAgent<R>,
    B: KnowledgeBackend,
{
    pub async fn run(
        &self,
        runtime: &R,
        input: RunInput,
    ) -> Result<WorkflowRunResult, WorkflowError> {
        self.run_with_report(runtime, input)
            .await
            .map(|outcome| outcome.result)
    }

    pub async fn run_with_report(
        &self,
        runtime: &R,
        input: RunInput,
    ) -> Result<WorkflowRunOutput, WorkflowError> {
        let max_discovery_turns = input.max_discovery_turns.min(self.max_discovery_turns);
        if max_discovery_turns == 0 {
            return Err(WorkflowError::Discovery(
                "discovery turn budget must be greater than zero".to_string(),
            ));
        }
        let discovery_step = build_turn_step(
            self.discovery_agent.clone(),
            RetryPolicy::new(max_discovery_turns),
        );
        let knowledge_planning_step = self.knowledge_planning_step.clone();
        let knowledge_materialisation_step = self.knowledge_materialisation_step.clone();
        let prepare_solutions_step = self.prepare_solutions_step.clone();
        let solution_generation_step = self.solution_generation_step.clone();
        let solution_collect_step = self.solution_collect_step.clone();
        let solution_choice_step = self.solution_choice_step.clone();
        let prepare_architect_step = self.prepare_architect_step.clone();
        let architect_step = self.architect_step.clone();
        let ready_result_step = self.ready_result_step.clone();
        let revision_result_step = self.revision_result_step.clone();

        let discovery_node = NodeSpec::new(
            "discovery",
            StepNode::new(discovery_step, move |node_input: &NodeInput| {
                let run_input = node_input.seed_as::<RunInput>()?;
                Ok(DiscoveryInput::new(run_input.prompt)
                    .with_clarification_budget(max_discovery_turns))
            })
            .spawn_with(move |context, _discovery_turn| {
                let discovery_id = context.node_id();
                let knowledge_id = NodeId::new();
                let materialisation_id = NodeId::new();
                let prepare_solutions_id = NodeId::new();
                let solution_generation_id = NodeId::new();
                let collect_id = NodeId::new();
                let choice_id = NodeId::new();

                let knowledge_planning_step = knowledge_planning_step.clone();
                let knowledge_materialisation_step = knowledge_materialisation_step.clone();
                let prepare_solutions_step = prepare_solutions_step.clone();
                let solution_generation_step = solution_generation_step.clone();
                let solution_collect_step = solution_collect_step.clone();
                let solution_choice_step = solution_choice_step.clone();
                let prepare_architect_step = prepare_architect_step.clone();
                let architect_step = architect_step.clone();
                let ready_result_step = ready_result_step.clone();
                let revision_result_step = revision_result_step.clone();

                let knowledge_node = NodeSpec::new(
                    "knowledge_planning",
                    StepNode::new(knowledge_planning_step, move |node_input: &NodeInput| {
                        let discovery_turn = node_input.output_as::<DiscoveryTurn>(discovery_id)?;
                        Ok(KnowledgePlanningInput::new(discovery_outcome_from_turn(
                            discovery_turn,
                        )))
                    })
                    .spawn_with(move |context, _knowledge_turn| {
                        let knowledge_id = context.node_id();
                        let prepare_solutions_step = prepare_solutions_step.clone();
                        let solution_generation_step = solution_generation_step.clone();
                        let solution_collect_step = solution_collect_step.clone();
                        let solution_choice_step = solution_choice_step.clone();
                        let prepare_architect_step = prepare_architect_step.clone();
                        let architect_step = architect_step.clone();
                        let ready_result_step = ready_result_step.clone();
                        let revision_result_step = revision_result_step.clone();

                        let materialisation_node = NodeSpec::new(
                            "knowledge_materialisation",
                            StepNode::new(
                                knowledge_materialisation_step.clone(),
                                move |node_input: &NodeInput| {
                                    let knowledge_turn = node_input
                                        .output_as::<KnowledgePlanningTurn>(knowledge_id)?;
                                    Ok(knowledge_turn.plan)
                                },
                            )
                            .spawn_with(move |context, _materialised| {
                                let materialisation_id = context.node_id();
                                let prepare_solutions_step = prepare_solutions_step.clone();
                                let solution_generation_step = solution_generation_step.clone();
                                let solution_collect_step = solution_collect_step.clone();
                                let solution_choice_step = solution_choice_step.clone();
                                let prepare_architect_step = prepare_architect_step.clone();
                                let architect_step = architect_step.clone();
                                let ready_result_step = ready_result_step.clone();
                                let revision_result_step = revision_result_step.clone();

                                let prepare_solutions_node = NodeSpec::new(
                                    "prepare_solutions_input",
                                    StepNode::new(
                                        prepare_solutions_step.clone(),
                                        move |node_input: &NodeInput| {
                                            let discovery_turn = node_input
                                                .output_as::<DiscoveryTurn>(discovery_id)?;
                                            let materialised_knowledge = node_input.output_as::<
                                                Vec<MaterialisedKnowledgeGroup>,
                                            >(materialisation_id)?;
                                            Ok(SolutionsStageInput {
                                                discovery: discovery_outcome_from_turn(
                                                    discovery_turn,
                                                ),
                                                materialised_knowledge,
                                            })
                                        },
                                    )
                                    .spawn_with(move |context, _solution_input| {
                                        let prepare_solutions_id = context.node_id();
                                        let solution_generation_step =
                                            solution_generation_step.clone();
                                        let solution_collect_step = solution_collect_step.clone();
                                        let solution_choice_step = solution_choice_step.clone();
                                        let prepare_architect_step =
                                            prepare_architect_step.clone();
                                        let architect_step = architect_step.clone();
                                        let ready_result_step = ready_result_step.clone();
                                        let revision_result_step =
                                            revision_result_step.clone();

                                        let solution_generation_node = NodeSpec::new(
                                            "solution_generation",
                                            StepNode::new(
                                                solution_generation_step.clone(),
                                                move |node_input: &NodeInput| {
                                                    node_input.output_as::<SolutionInput>(
                                                        prepare_solutions_id,
                                                    )
                                                },
                                            )
                                            .spawn_with(move |context, _drafts| {
                                                let solution_generation_id = context.node_id();
                                                let solution_collect_step =
                                                    solution_collect_step.clone();
                                                let solution_choice_step =
                                                    solution_choice_step.clone();
                                                let prepare_architect_step =
                                                    prepare_architect_step.clone();
                                                let architect_step = architect_step.clone();
                                                let ready_result_step =
                                                    ready_result_step.clone();
                                                let revision_result_step =
                                                    revision_result_step.clone();

                                                let collect_node = NodeSpec::new(
                                                    "solution_collect",
                                                    StepNode::new(
                                                        solution_collect_step.clone(),
                                                        move |node_input: &NodeInput| {
                                                            let drafts = node_input.output_as::<
                                                                Vec<solutions::SolutionDraft>,
                                                            >(solution_generation_id)?;
                                                            Ok(SolutionCollectInput::new(drafts))
                                                        },
                                                    )
                                                    .spawn_with(move |context, _collection| {
                                                        let collect_id = context.node_id();
                                                        let solution_choice_step =
                                                            solution_choice_step.clone();
                                                        let prepare_architect_step =
                                                            prepare_architect_step.clone();
                                                        let architect_step =
                                                            architect_step.clone();
                                                        let ready_result_step =
                                                            ready_result_step.clone();
                                                        let revision_result_step =
                                                            revision_result_step.clone();

                                                        let choice_node = NodeSpec::new(
                                                            "solution_choice",
                                                            StepNode::new(
                                                                solution_choice_step.clone(),
                                                                move |node_input: &NodeInput| {
                                                                    let collect_turn = node_input
                                                                        .output_as::<
                                                                            SolutionCollectTurn,
                                                                        >(collect_id)?;
                                                                    Ok(collect_turn.collection)
                                                                },
                                                            )
                                                            .spawn_with(move |context, choice| {
                                                                let choice_id = context.node_id();
                                                                let choice = choice.clone();
                                                                let prepare_architect_step =
                                                                    prepare_architect_step.clone();
                                                                let architect_step =
                                                                    architect_step.clone();
                                                                let ready_result_step =
                                                                    ready_result_step.clone();
                                                                let revision_result_step =
                                                                    revision_result_step.clone();

                                                                match choice {
                                                                    SolutionUserChoice::Selected(
                                                                        selected_solution,
                                                                    ) => {
                                                                        let architect_input_id =
                                                                            NodeId::new();
                                                                        let architect_id =
                                                                            NodeId::new();
                                                                        let result_id =
                                                                            NodeId::new();
                                                                        let prepare_architect_step =
                                                                            prepare_architect_step
                                                                                .clone();
                                                                        let architect_step =
                                                                            architect_step.clone();
                                                                        let ready_result_step =
                                                                            ready_result_step
                                                                                .clone();
                                                                        let selected_solution_seed =
                                                                            selected_solution
                                                                                .clone();
                                                                        let selected_solution_for_result =
                                                                            selected_solution
                                                                                .clone();

                                                                        let architect_input_node = NodeSpec::new(
                                                                            "prepare_architect_input",
                                                                            StepNode::new(
                                                                                prepare_architect_step.clone(),
                                                                                move |node_input: &NodeInput| {
                                                                                    let selected_solution =
                                                                                        node_input
                                                                                            .seed_as::<SelectedSolution>()?;
                                                                                    let discovery_turn = node_input
                                                                                        .output_as::<DiscoveryTurn>(
                                                                                            discovery_id,
                                                                                        )?;
                                                                                    let materialised_knowledge = node_input
                                                                                        .output_as::<
                                                                                            Vec<MaterialisedKnowledgeGroup>,
                                                                                        >(materialisation_id)?;
                                                                                    Ok(ArchitectStageInput {
                                                                                        discovery: discovery_outcome_from_turn(discovery_turn),
                                                                                        selected_solution,
                                                                                        materialised_knowledge,
                                                                                    })
                                                                                },
                                                                            )
                                                                            .spawn_with(move |context, _architect_input| {
                                                                                let architect_input_id = context.node_id();
                                                                                let architect_step =
                                                                                    architect_step.clone();
                                                                                let ready_result_step =
                                                                                    ready_result_step.clone();
                                                                                let selected_solution =
                                                                                    selected_solution_for_result.clone();

                                                                                let architect_node = NodeSpec::new(
                                                                                    "software_architect",
                                                                                    StepNode::new(
                                                                                        architect_step.clone(),
                                                                                        move |node_input: &NodeInput| {
                                                                                            node_input.output_as::<ArchitectTurnInput>(architect_input_id)
                                                                                        },
                                                                                    )
                                                                                    .spawn_with(move |context, _architect_turn| {
                                                                                        let architect_id = context.node_id();
                                                                                        let ready_result_step =
                                                                                            ready_result_step.clone();
                                                                                        let selected_solution =
                                                                                            selected_solution.clone();

                                                                                        GraphPatch::new()
                                                                                            .with_node(
                                                                                                NodeSpec::new(
                                                                                                    "workflow_result_ready",
                                                                                                    StepNode::new(
                                                                                                        ready_result_step.clone(),
                                                                                                        move |node_input: &NodeInput| {
                                                                                                            let selected_solution = node_input.seed_as::<SelectedSolution>()?;
                                                                                                            let discovery_turn = node_input.output_as::<DiscoveryTurn>(discovery_id)?;
                                                                                                            let knowledge_turn = node_input.output_as::<KnowledgePlanningTurn>(knowledge_id)?;
                                                                                                            let materialised_knowledge = node_input.output_as::<Vec<MaterialisedKnowledgeGroup>>(materialisation_id)?;
                                                                                                            let collect_turn = node_input.output_as::<SolutionCollectTurn>(collect_id)?;
                                                                                                            let architect_turn = node_input.output_as::<ArchitectTurn>(architect_id)?;
                                                                                                            Ok(ReadyWorkflowResultInput {
                                                                                                                planning: PlanningBoundaryInput {
                                                                                                                    discovery: discovery_outcome_from_turn(discovery_turn),
                                                                                                                    knowledge_plan: knowledge_turn.plan,
                                                                                                                    materialised_knowledge,
                                                                                                                    selected_solution,
                                                                                                                    architect_plan: architect_turn.plan,
                                                                                                                },
                                                                                                                collected_solutions: collect_turn.collection,
                                                                                                            })
                                                                                                        },
                                                                                                    ),
                                                                                                )
                                                                                                .with_id(result_id)
                                                                                                .with_parent(architect_id)
                                                                                                .with_seed(selected_solution.clone())
                                                                                                .expect("selected solution seed should serialise"),
                                                                                            )
                                                                                            .with_edge(EdgeSpec::new(architect_id, result_id))
                                                                                            .with_edge(EdgeSpec::new(discovery_id, result_id))
                                                                                            .with_edge(EdgeSpec::new(knowledge_id, result_id))
                                                                                            .with_edge(EdgeSpec::new(materialisation_id, result_id))
                                                                                            .with_edge(EdgeSpec::new(collect_id, result_id))
                                                                                    }),
                                                                                )
                                                                                .with_id(architect_id)
                                                                                .with_parent(architect_input_id);

                                                                                GraphPatch::new()
                                                                                    .with_node(architect_node)
                                                                                    .with_edge(EdgeSpec::new(
                                                                                        architect_input_id,
                                                                                        architect_id,
                                                                                    ))
                                                                            }),
                                                                        )
                                                                        .with_id(architect_input_id)
                                                                        .with_parent(choice_id)
                                                                        .with_seed(selected_solution_seed)
                                                                        .expect("selected solution seed should serialise");

                                                                        GraphPatch::new()
                                                                            .with_node(architect_input_node)
                                                                            .with_edge(EdgeSpec::new(choice_id, architect_input_id))
                                                                            .with_edge(EdgeSpec::new(discovery_id, architect_input_id))
                                                                            .with_edge(EdgeSpec::new(materialisation_id, architect_input_id))
                                                                    }
                                                                    SolutionUserChoice::Revise {
                                                                        feedback,
                                                                    } => {
                                                                        let result_id =
                                                                            NodeId::new();

                                                                        GraphPatch::new()
                                                                            .with_node(
                                                                                NodeSpec::new(
                                                                                    "workflow_result_revision",
                                                                                    StepNode::new(
                                                                                        revision_result_step.clone(),
                                                                                        move |node_input: &NodeInput| {
                                                                                            let feedback = node_input.seed_as::<String>()?;
                                                                                            let collect_turn = node_input.output_as::<SolutionCollectTurn>(collect_id)?;
                                                                                            Ok(RevisionWorkflowResultInput {
                                                                                                collected_solutions: collect_turn.collection,
                                                                                                feedback,
                                                                                            })
                                                                                        },
                                                                                    ),
                                                                                )
                                                                                .with_id(result_id)
                                                                                .with_parent(choice_id)
                                                                                .with_seed(feedback.clone())
                                                                                .expect("revision feedback seed should serialise"),
                                                                            )
                                                                            .with_edge(EdgeSpec::new(choice_id, result_id))
                                                                            .with_edge(EdgeSpec::new(collect_id, result_id))
                                                                    }
                                                                }
                                                            }),
                                                        )
                                                        .with_id(choice_id)
                                                        .with_parent(collect_id);

                                                        GraphPatch::new()
                                                            .with_node(choice_node)
                                                            .with_edge(EdgeSpec::new(
                                                                collect_id, choice_id,
                                                            ))
                                                    }),
                                                )
                                                .with_id(collect_id)
                                                .with_parent(solution_generation_id);

                                                GraphPatch::new()
                                                    .with_node(collect_node)
                                                    .with_edge(EdgeSpec::new(
                                                        solution_generation_id,
                                                        collect_id,
                                                    ))
                                            }),
                                        )
                                        .with_id(solution_generation_id)
                                        .with_parent(prepare_solutions_id);

                                        GraphPatch::new()
                                            .with_node(solution_generation_node)
                                            .with_edge(EdgeSpec::new(
                                                prepare_solutions_id,
                                                solution_generation_id,
                                            ))
                                    }),
                                )
                                .with_id(prepare_solutions_id)
                                .with_parent(materialisation_id);

                                GraphPatch::new()
                                    .with_node(prepare_solutions_node)
                                    .with_edge(EdgeSpec::new(discovery_id, prepare_solutions_id))
                                    .with_edge(EdgeSpec::new(
                                        materialisation_id,
                                        prepare_solutions_id,
                                    ))
                            }),
                        )
                        .with_id(materialisation_id)
                        .with_parent(knowledge_id);

                        GraphPatch::new()
                            .with_node(materialisation_node)
                            .with_edge(EdgeSpec::new(knowledge_id, materialisation_id))
                    }),
                )
                .with_id(knowledge_id)
                .with_parent(discovery_id);

                GraphPatch::new()
                    .with_node(knowledge_node)
                    .with_edge(EdgeSpec::new(discovery_id, knowledge_id))
            }),
        )
        .with_seed(input)
        .expect("workflow run input should serialise");

        let report = GraphWorkflow::new()
            .with_max_concurrency(4)
            .with_patch(GraphPatch::new().with_node(discovery_node))
            .map_err(|error| {
                WorkflowError::Workflow(format!("invalid workflow graph patch: {error}"))
            })?
            .run(runtime)
            .await
            .map_err(map_graph_error)?;
        let result = extract_graph_result(&report)?;

        Ok(WorkflowRunOutput { result, report })
    }
}

pub fn build_subject_workflow<R, D, K, S, C, A, B>(
    dependencies: SubjectWorkflowDependencies<D, K, S, C, A, B>,
) -> SubjectWorkflow<R, D, B>
where
    R: WorkflowRuntime + HumanIO<Error = WorkflowError> + 'static,
    D: DiscoveryTurnAgent<R>,
    K: KnowledgePlanningAgent<R>,
    S: SolutionBranchAgent<R>,
    C: SolutionCollectAgent<R>,
    A: ArchitectAgent<R>,
    B: KnowledgeBackend,
{
    SubjectWorkflow {
        discovery_agent: dependencies.discovery_agent,
        knowledge_planning_step: build_knowledge_planning_step(
            dependencies.knowledge_planner,
            RetryPolicy::new(3),
        ),
        knowledge_materialisation_step: build_materialisation_step(
            dependencies.knowledge_store,
            dependencies.knowledge_backend.clone(),
        ),
        solution_generation_step: build_solution_generation_step(dependencies.solution_agent),
        solution_collect_step: build_collect_step(dependencies.collect_agent, RetryPolicy::new(3)),
        solution_choice_step: build_choice_step::<R>(),
        architect_step: architect::build_architect_step(
            dependencies.architect_agent,
            RetryPolicy::new(3),
        ),
        prepare_solutions_step: build_prepare_solutions_step::<R>(),
        prepare_architect_step: build_prepare_architect_step::<R>(),
        ready_result_step: build_ready_result_step::<R>(),
        revision_result_step: build_revision_result_step::<R>(),
        max_discovery_turns: dependencies.max_discovery_turns,
        _backend: dependencies.knowledge_backend,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Arc};

    use naaf_persistence_sqlite::SqliteKnowledgeGroupStore;
    use parking_lot::Mutex;

    use crate::runtime::ScriptedRuntime;

    use super::*;

    #[derive(Default)]
    struct StubDiscoveryAgent {
        states: Mutex<VecDeque<discovery::DiscoveryState>>,
    }

    impl StubDiscoveryAgent {
        fn new(states: Vec<discovery::DiscoveryState>) -> Self {
            Self {
                states: Mutex::new(states.into()),
            }
        }
    }

    impl discovery::DiscoveryTurnAgent<ScriptedRuntime> for StubDiscoveryAgent {
        fn run_turn<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            _input: discovery::DiscoveryInput,
            _prompt: String,
        ) -> futures::future::LocalBoxFuture<'a, Result<discovery::DiscoveryState, WorkflowError>>
        {
            let state = self
                .states
                .lock()
                .pop_front()
                .expect("discovery state should exist");
            Box::pin(async move { Ok(state) })
        }
    }

    #[derive(Default)]
    struct StubKnowledgePlanner;

    impl knowledge::KnowledgePlanningAgent<ScriptedRuntime> for StubKnowledgePlanner {
        fn plan<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            _input: knowledge::KnowledgePlanningInput,
            _prompt: String,
        ) -> futures::future::LocalBoxFuture<'a, Result<knowledge::KnowledgePlan, WorkflowError>>
        {
            Box::pin(async move {
                Ok(knowledge::KnowledgePlan {
                    groups: vec![knowledge::KnowledgeGroupPlan {
                        template: knowledge::KnowledgeGroupTemplate::DiscoveryTranscript,
                        instance_name: "rewrite".to_string(),
                        description: "Discovery answers".to_string(),
                        tags: vec!["rewrite".to_string()],
                        query_hints: vec!["Use the user's answers".to_string()],
                        stages: vec![
                            WorkflowStageId::Solutions,
                            WorkflowStageId::SoftwareArchitect,
                        ],
                        sources: vec![knowledge::KnowledgeSource::discovery_transcript(
                            "Discovery transcript",
                            "Use SQLite for metadata.",
                        )],
                    }],
                    upstream_follow_ups: knowledge::UPSTREAM_NAAF_FOLLOW_UPS
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                })
            })
        }
    }

    #[derive(Default)]
    struct StubKnowledgeBackend;

    impl knowledge::KnowledgeBackend for StubKnowledgeBackend {
        fn initialise_group<'a>(
            &'a self,
            _group: &'a knowledge::MaterialisedKnowledgeGroup,
        ) -> futures::future::LocalBoxFuture<'a, Result<(), WorkflowError>> {
            Box::pin(async move { Ok(()) })
        }

        fn ingest_source<'a>(
            &'a self,
            _group: &'a knowledge::MaterialisedKnowledgeGroup,
            _source: &'a knowledge::KnowledgeSource,
        ) -> futures::future::LocalBoxFuture<'a, Result<(), WorkflowError>> {
            Box::pin(async move { Ok(()) })
        }
    }

    #[derive(Default)]
    struct StubSolutionAgent;

    impl solutions::SolutionBranchAgent<ScriptedRuntime> for StubSolutionAgent {
        fn generate<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            branch: solutions::SolutionBranch,
            _input: solutions::SolutionInput,
            _prompt: String,
        ) -> futures::future::LocalBoxFuture<'a, Result<solutions::SolutionDraft, WorkflowError>>
        {
            Box::pin(async move {
                Ok(solutions::SolutionDraft {
                    branch,
                    title: format!("{} path", branch.slug()),
                    summary: format!("{} summary", branch.slug()),
                    scope: format!("{} scope", branch.slug()),
                    architecture: vec![format!("{} architecture", branch.slug())],
                    delivery_plan: vec![format!("{} plan", branch.slug())],
                    technologies: vec!["Rust".to_string()],
                    rationale: format!("{} rationale", branch.slug()),
                    risks: vec![format!("{} risk", branch.slug())],
                })
            })
        }
    }

    #[derive(Default)]
    struct StubCollectAgent;

    impl solutions::SolutionCollectAgent<ScriptedRuntime> for StubCollectAgent {
        fn collect<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            drafts: Vec<solutions::SolutionDraft>,
            _prompt: String,
        ) -> futures::future::LocalBoxFuture<'a, Result<SolutionCollection, WorkflowError>>
        {
            Box::pin(async move {
                Ok(SolutionCollection {
                    drafts,
                    recommendation: solutions::SolutionRecommendation {
                        recommended_branch: Some(solutions::SolutionBranch::Recommended),
                        recommended_hybrid: None,
                        rationale: "recommended is the best default".to_string(),
                    },
                })
            })
        }
    }

    #[derive(Default)]
    struct StubArchitectAgent;

    impl architect::ArchitectAgent<ScriptedRuntime> for StubArchitectAgent {
        fn design<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            _input: architect::ArchitectInput,
            _prompt: String,
        ) -> futures::future::LocalBoxFuture<'a, Result<architect::ArchitectPlan, WorkflowError>>
        {
            Box::pin(async move {
                Ok(architect::ArchitectPlan {
                    summary: "Architect output".to_string(),
                    architecture_decisions: vec!["Keep stage ownership local".to_string()],
                    implementation_guidance: vec!["Start with discovery".to_string()],
                    planning_notes: vec!["Planning-ready".to_string()],
                    risks: vec!["NAAF follow-up work remains".to_string()],
                })
            })
        }
    }

    #[tokio::test]
    async fn workflow_returns_planning_boundary_when_user_selects_a_branch() {
        let runtime = ScriptedRuntime::new(["Use SQLite", "recommended"])
            .with_stage_prompt(WorkflowStageId::Solutions, "Solutions prompt")
            .with_stage_prompt(WorkflowStageId::SoftwareArchitect, "Architect prompt");
        let store = Arc::new(
            SqliteKnowledgeGroupStore::open_in_memory().expect("SQLite store should open"),
        );
        let workflow = build_subject_workflow(SubjectWorkflowDependencies {
            discovery_agent: Arc::new(StubDiscoveryAgent::new(vec![
                discovery::DiscoveryState {
                    ready_for_solution: false,
                    problem_statement: "Rewrite MMAT".to_string(),
                    goals: vec!["Keep workflow shape".to_string()],
                    constraints: vec![],
                    assumptions: vec![],
                    risks: vec!["Architecture drift".to_string()],
                    notes: vec![],
                    recommended_path: "Clarify persistence".to_string(),
                    open_questions: vec![discovery::DiscoveryQuestion {
                        prompt: "What should store knowledge metadata?".to_string(),
                        choices: vec!["SQLite".to_string(), "Filesystem".to_string()],
                    }],
                },
                discovery::DiscoveryState {
                    ready_for_solution: true,
                    problem_statement: "Rewrite MMAT".to_string(),
                    goals: vec!["Keep workflow shape".to_string()],
                    constraints: vec!["SQLite metadata".to_string()],
                    assumptions: vec!["Live-only questions are acceptable".to_string()],
                    risks: vec!["NAAF gaps remain".to_string()],
                    notes: vec![],
                    recommended_path: "Generate branches".to_string(),
                    open_questions: vec![],
                },
            ])),
            knowledge_planner: Arc::new(StubKnowledgePlanner),
            knowledge_store: store,
            knowledge_backend: Arc::new(StubKnowledgeBackend),
            solution_agent: Arc::new(StubSolutionAgent),
            collect_agent: Arc::new(StubCollectAgent),
            architect_agent: Arc::new(StubArchitectAgent),
            max_discovery_turns: 4,
        });

        let output = workflow
            .run_with_report(&runtime, RunInput::new("Rewrite MMAT"))
            .await
            .expect("workflow should succeed");
        let node_names = output
            .report
            .nodes()
            .values()
            .map(|node| node.name())
            .collect::<Vec<_>>();

        assert!(node_names.contains(&"discovery"));
        assert!(node_names.contains(&"knowledge_planning"));
        assert!(node_names.contains(&"knowledge_materialisation"));
        assert!(node_names.contains(&"prepare_solutions_input"));
        assert!(node_names.contains(&"solution_generation"));
        assert!(node_names.contains(&"solution_collect"));
        assert!(node_names.contains(&"solution_choice"));
        assert!(node_names.contains(&"prepare_architect_input"));
        assert!(node_names.contains(&"software_architect"));
        assert!(node_names.contains(&"workflow_result_ready"));

        match output.result {
            WorkflowRunResult::ReadyForPlanning { planning, .. } => {
                assert_eq!(planning.selected_solution.choice_label, "recommended");
                assert_eq!(planning.materialised_knowledge.len(), 1);
                assert_eq!(planning.architect_plan.summary, "Architect output");
            }
            other => panic!("expected planning boundary, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn workflow_routes_revision_feedback_without_running_architect() {
        let runtime = ScriptedRuntime::new(["Use SQLite", "revise: keep the solution smaller"]);
        let store = Arc::new(
            SqliteKnowledgeGroupStore::open_in_memory().expect("SQLite store should open"),
        );
        let workflow = build_subject_workflow(SubjectWorkflowDependencies {
            discovery_agent: Arc::new(StubDiscoveryAgent::new(vec![
                discovery::DiscoveryState {
                    ready_for_solution: false,
                    problem_statement: "Rewrite MMAT".to_string(),
                    goals: vec![],
                    constraints: vec![],
                    assumptions: vec![],
                    risks: vec![],
                    notes: vec![],
                    recommended_path: "Clarify persistence".to_string(),
                    open_questions: vec![discovery::DiscoveryQuestion {
                        prompt: "What should store knowledge metadata?".to_string(),
                        choices: vec!["SQLite".to_string(), "Filesystem".to_string()],
                    }],
                },
                discovery::DiscoveryState {
                    ready_for_solution: true,
                    problem_statement: "Rewrite MMAT".to_string(),
                    goals: vec!["Keep workflow shape".to_string()],
                    constraints: vec!["SQLite metadata".to_string()],
                    assumptions: vec![],
                    risks: vec![],
                    notes: vec![],
                    recommended_path: "Generate branches".to_string(),
                    open_questions: vec![],
                },
            ])),
            knowledge_planner: Arc::new(StubKnowledgePlanner),
            knowledge_store: store,
            knowledge_backend: Arc::new(StubKnowledgeBackend),
            solution_agent: Arc::new(StubSolutionAgent),
            collect_agent: Arc::new(StubCollectAgent),
            architect_agent: Arc::new(StubArchitectAgent),
            max_discovery_turns: 4,
        });

        let result = workflow
            .run(&runtime, RunInput::new("Rewrite MMAT"))
            .await
            .expect("workflow should succeed");

        assert_eq!(
            result,
            WorkflowRunResult::NeedsRevision {
                collected_solutions: SolutionCollection {
                    drafts: vec![
                        solutions::SolutionDraft {
                            branch: solutions::SolutionBranch::Conservative,
                            title: "conservative path".to_string(),
                            summary: "conservative summary".to_string(),
                            scope: "conservative scope".to_string(),
                            architecture: vec!["conservative architecture".to_string()],
                            delivery_plan: vec!["conservative plan".to_string()],
                            technologies: vec!["Rust".to_string()],
                            rationale: "conservative rationale".to_string(),
                            risks: vec!["conservative risk".to_string()],
                        },
                        solutions::SolutionDraft {
                            branch: solutions::SolutionBranch::Recommended,
                            title: "recommended path".to_string(),
                            summary: "recommended summary".to_string(),
                            scope: "recommended scope".to_string(),
                            architecture: vec!["recommended architecture".to_string()],
                            delivery_plan: vec!["recommended plan".to_string()],
                            technologies: vec!["Rust".to_string()],
                            rationale: "recommended rationale".to_string(),
                            risks: vec!["recommended risk".to_string()],
                        },
                        solutions::SolutionDraft {
                            branch: solutions::SolutionBranch::Ambitious,
                            title: "ambitious path".to_string(),
                            summary: "ambitious summary".to_string(),
                            scope: "ambitious scope".to_string(),
                            architecture: vec!["ambitious architecture".to_string()],
                            delivery_plan: vec!["ambitious plan".to_string()],
                            technologies: vec!["Rust".to_string()],
                            rationale: "ambitious rationale".to_string(),
                            risks: vec!["ambitious risk".to_string()],
                        },
                    ],
                    recommendation: solutions::SolutionRecommendation {
                        recommended_branch: Some(solutions::SolutionBranch::Recommended),
                        recommended_hybrid: None,
                        rationale: "recommended is the best default".to_string(),
                    },
                },
                feedback: "revise: keep the solution smaller".to_string(),
            }
        );
    }
}
