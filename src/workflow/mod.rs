use std::{fmt::Display, sync::Arc};

use naaf_core::Step;
use naaf_llm::HumanIO;
use naaf_persistence_sqlite::SqliteKnowledgeGroupStore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use self::{
    architect::{ArchitectAgent, ArchitectInput, ArchitectPlan},
    discovery::{
        DiscoveryInput, DiscoveryOutcome, DiscoveryState, DiscoveryTurnAgent, build_turn_step,
        run_live_discovery,
    },
    knowledge::{
        KnowledgeBackend, KnowledgePlan, KnowledgePlanningAgent, KnowledgePlanningInput,
        MaterialisedKnowledgeGroup, build_knowledge_planning_step, build_materialisation_step,
        build_stage_knowledge_session,
    },
    solutions::{
        SelectedSolution, SolutionBranchAgent, SolutionCollectAgent, SolutionCollection,
        SolutionInput, SolutionUserChoice, build_choice_step, build_collect_step,
        build_solution_generation_step,
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

pub struct SubjectWorkflow<R, B> {
    discovery_step: Step<R, DiscoveryInput, DiscoveryState, (), WorkflowError>,
    knowledge_planning_step: Step<R, KnowledgePlanningInput, KnowledgePlan, (), WorkflowError>,
    knowledge_materialisation_step:
        Step<R, KnowledgePlan, Vec<MaterialisedKnowledgeGroup>, (), WorkflowError>,
    solution_generation_step:
        Step<R, SolutionInput, Vec<solutions::SolutionDraft>, (), WorkflowError>,
    solution_collect_step:
        Step<R, Vec<solutions::SolutionDraft>, SolutionCollection, (), WorkflowError>,
    solution_choice_step: Step<R, SolutionCollection, SolutionUserChoice, (), WorkflowError>,
    architect_step: Step<R, ArchitectInput, ArchitectPlan, (), WorkflowError>,
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

impl<R, B> SubjectWorkflow<R, B>
where
    R: WorkflowRuntime + HumanIO<Error = WorkflowError> + 'static,
    B: KnowledgeBackend,
{
    pub async fn run(
        &self,
        runtime: &R,
        input: RunInput,
    ) -> Result<WorkflowRunResult, WorkflowError> {
        let discovery = run_live_discovery(
            runtime,
            &self.discovery_step,
            input.prompt,
            input.max_discovery_turns.min(self.max_discovery_turns),
        )
        .await?;

        let knowledge_plan = self
            .knowledge_planning_step
            .run(
                runtime,
                KnowledgePlanningInput {
                    discovery: discovery.clone(),
                },
            )
            .await
            .map_err(|error| {
                WorkflowError::Knowledge(format!("knowledge planning failed: {error}"))
            })?;

        let materialised_knowledge = self
            .knowledge_materialisation_step
            .run(runtime, knowledge_plan.clone())
            .await
            .map_err(|error| {
                WorkflowError::Knowledge(format!("knowledge materialisation failed: {error}"))
            })?;

        let solutions_session = build_stage_knowledge_session(
            runtime,
            WorkflowStageId::Solutions,
            &materialised_knowledge,
        );
        let solution_input = SolutionInput {
            discovery: discovery.clone(),
            knowledge: solutions_session,
        };

        let drafts = self
            .solution_generation_step
            .run(runtime, solution_input)
            .await
            .map_err(|error| {
                WorkflowError::Solution(format!("solution generation failed: {error}"))
            })?;

        let collected_solutions = self
            .solution_collect_step
            .run(runtime, drafts)
            .await
            .map_err(|error| {
                WorkflowError::Solution(format!("solution collection failed: {error}"))
            })?;

        let user_choice = self
            .solution_choice_step
            .run(runtime, collected_solutions.clone())
            .await
            .map_err(|error| WorkflowError::Solution(format!("solution choice failed: {error}")))?;

        let selected_solution = match user_choice {
            SolutionUserChoice::Selected(selected_solution) => selected_solution,
            SolutionUserChoice::Revise { feedback } => {
                return Ok(WorkflowRunResult::NeedsRevision {
                    collected_solutions,
                    feedback,
                });
            }
        };

        let architect_knowledge = build_stage_knowledge_session(
            runtime,
            WorkflowStageId::SoftwareArchitect,
            &materialised_knowledge,
        );
        let architect_plan = self
            .architect_step
            .run(
                runtime,
                ArchitectInput {
                    discovery: discovery.clone(),
                    selected_solution: selected_solution.clone(),
                    knowledge: architect_knowledge,
                },
            )
            .await
            .map_err(|error| {
                WorkflowError::Architect(format!("architect stage failed: {error}"))
            })?;

        Ok(WorkflowRunResult::ReadyForPlanning {
            planning: Box::new(PlanningBoundaryInput {
                discovery,
                knowledge_plan,
                materialised_knowledge,
                selected_solution,
                architect_plan,
            }),
            collected_solutions,
        })
    }
}

pub fn build_subject_workflow<R, D, K, S, C, A, B>(
    dependencies: SubjectWorkflowDependencies<D, K, S, C, A, B>,
) -> SubjectWorkflow<R, B>
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
        discovery_step: build_turn_step(dependencies.discovery_agent),
        knowledge_planning_step: build_knowledge_planning_step(dependencies.knowledge_planner),
        knowledge_materialisation_step: build_materialisation_step(
            dependencies.knowledge_store,
            dependencies.knowledge_backend.clone(),
        ),
        solution_generation_step: build_solution_generation_step(dependencies.solution_agent),
        solution_collect_step: build_collect_step(dependencies.collect_agent),
        solution_choice_step: build_choice_step::<R>(),
        architect_step: architect::build_architect_step(dependencies.architect_agent),
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

        let result = workflow
            .run(&runtime, RunInput::new("Rewrite MMAT"))
            .await
            .expect("workflow should succeed");

        match result {
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
                    goals: vec![],
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
