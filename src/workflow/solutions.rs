use std::sync::Arc;

use futures::future::LocalBoxFuture;
use naaf_core::{Step, TaskExt, task_fn};
use naaf_llm::{HumanIO, HumanQuestion};
use serde::{Deserialize, Serialize};

use crate::workflow::{
    WorkflowError, WorkflowStageId, discovery::DiscoveryOutcome, knowledge::StageKnowledgeSession,
};

pub trait SolutionBranchAgent<R>: Send + Sync + 'static {
    fn generate<'a>(
        &'a self,
        runtime: &'a R,
        branch: SolutionBranch,
        input: SolutionInput,
        prompt: String,
    ) -> LocalBoxFuture<'a, Result<SolutionDraft, WorkflowError>>;
}

pub trait SolutionCollectAgent<R>: Send + Sync + 'static {
    fn collect<'a>(
        &'a self,
        runtime: &'a R,
        drafts: Vec<SolutionDraft>,
        prompt: String,
    ) -> LocalBoxFuture<'a, Result<SolutionCollection, WorkflowError>>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionRecommendation {
    pub recommended_branch: Option<SolutionBranch>,
    pub recommended_hybrid: Option<HybridSolution>,
    pub rationale: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionCollection {
    pub drafts: Vec<SolutionDraft>,
    pub recommendation: SolutionRecommendation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionDraft {
    pub branch: SolutionBranch,
    pub title: String,
    pub summary: String,
    pub scope: String,
    pub architecture: Vec<String>,
    pub delivery_plan: Vec<String>,
    pub technologies: Vec<String>,
    pub rationale: String,
    pub risks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HybridSolution {
    pub title: String,
    pub summary: String,
    pub source_branches: Vec<SolutionBranch>,
    pub architecture: Vec<String>,
    pub delivery_plan: Vec<String>,
    pub technologies: Vec<String>,
    pub rationale: String,
    pub risks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedSolution {
    pub choice_label: String,
    pub branch_sources: Vec<SolutionBranch>,
    pub title: String,
    pub summary: String,
    pub architecture: Vec<String>,
    pub delivery_plan: Vec<String>,
    pub technologies: Vec<String>,
    pub rationale: String,
    pub risks: Vec<String>,
    pub recommended_for_stage: WorkflowStageId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolutionUserChoice {
    Selected(SelectedSolution),
    Revise { feedback: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SolutionBranch {
    Conservative,
    Recommended,
    Ambitious,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionInput {
    pub discovery: DiscoveryOutcome,
    pub knowledge: StageKnowledgeSession,
}

impl SolutionBranch {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Recommended => "recommended",
            Self::Ambitious => "ambitious",
        }
    }

    pub fn instruction(self) -> &'static str {
        match self {
            Self::Conservative => {
                "Optimise for the lowest-risk, smallest viable change that still solves the problem."
            }
            Self::Recommended => {
                "Optimise for the professional default with maintainability and clarity."
            }
            Self::Ambitious => {
                "Optimise for leverage and upside while keeping delivery risk explicit."
            }
        }
    }
}

pub fn build_choice_prompt(collection: &SolutionCollection) -> String {
    let mut lines = vec![
        "Choose a solution branch, accept the recommended hybrid, or reply with revision feedback."
            .to_string(),
        String::new(),
        "Candidate branches:".to_string(),
    ];

    lines.extend(collection.drafts.iter().map(|draft| {
        format!(
            "- {}: {} :: {}",
            draft.branch.slug(),
            draft.title,
            draft.summary
        )
    }));

    lines.push(String::new());
    lines.push(format!(
        "Recommendation: {}",
        describe_recommendation(&collection.recommendation)
    ));
    lines.push(collection.recommendation.rationale.clone());
    lines.push(String::new());
    lines.push(
        "Reply with one of the choices or select `revise` and include feedback in a follow-up reply."
            .to_string(),
    );

    lines.join("\n")
}

pub fn build_choice_step<R>() -> Step<R, SolutionCollection, SolutionUserChoice, (), WorkflowError>
where
    R: HumanIO<Error = WorkflowError> + 'static,
{
    Step::builder(
        task_fn(move |runtime: &R, collection: SolutionCollection| {
            Box::pin(async move {
                let prompt = build_choice_prompt(&collection);
                let reply = runtime
                    .ask(HumanQuestion {
                        question: prompt,
                        choices: Some(choice_options(&collection)),
                    })
                    .await?;
                if reply.content.trim().eq_ignore_ascii_case("revise") {
                    let feedback = runtime
                        .ask(HumanQuestion {
                            question:
                                "Describe the revisions you want to the presented solution set."
                                    .to_string(),
                            choices: None,
                        })
                        .await?;
                    return Ok(SolutionUserChoice::Revise {
                        feedback: feedback.content,
                    });
                }
                parse_solution_choice(&reply.content, &collection)
            })
        })
        .observed_as("choose_solution"),
    )
    .with_findings::<()>()
    .build()
}

pub fn build_collect_prompt(drafts: &[SolutionDraft]) -> String {
    let mut lines = vec![
        "Collect the candidate solution branches, recommend one branch or a hybrid, and explain the rationale."
            .to_string(),
    ];
    lines.extend(drafts.iter().map(|draft| {
        format!(
            "- {}: {} :: {}",
            draft.branch.slug(),
            draft.title,
            draft.summary
        )
    }));
    lines.join("\n")
}

pub fn build_collect_step<R: 'static, A>(
    agent: Arc<A>,
) -> Step<R, Vec<SolutionDraft>, SolutionCollection, (), WorkflowError>
where
    A: SolutionCollectAgent<R>,
{
    Step::builder(
        task_fn(move |runtime: &R, drafts: Vec<SolutionDraft>| {
            let prompt = build_collect_prompt(&drafts);
            let agent = agent.clone();
            Box::pin(async move { agent.collect(runtime, drafts, prompt).await })
        })
        .observed_as("collect_solutions"),
    )
    .with_findings::<()>()
    .build()
}

pub fn build_solution_branch_prompt(branch: SolutionBranch, input: &SolutionInput) -> String {
    let mut lines = vec![
        input.knowledge.system_prompt.clone(),
        String::new(),
        format!("Generate the `{}` solution branch.", branch.slug()),
        branch.instruction().to_string(),
        format!(
            "Problem statement: {}",
            input.discovery.state.problem_statement
        ),
        format!(
            "Knowledge groups in scope: {}",
            input.knowledge.group_collections.join(", ")
        ),
    ];

    if !input.discovery.state.constraints.is_empty() {
        lines.push(format!(
            "Constraints: {}",
            input.discovery.state.constraints.join(" | ")
        ));
    }

    if !input.discovery.state.risks.is_empty() {
        lines.push(format!(
            "Known risks: {}",
            input.discovery.state.risks.join(" | ")
        ));
    }

    lines.join("\n")
}

pub fn build_solution_branch_step<R: 'static, A>(
    agent: Arc<A>,
    branch: SolutionBranch,
) -> Step<R, SolutionInput, SolutionDraft, (), WorkflowError>
where
    A: SolutionBranchAgent<R>,
{
    Step::builder(
        task_fn(move |runtime: &R, input: SolutionInput| {
            let prompt = build_solution_branch_prompt(branch, &input);
            let agent = agent.clone();
            Box::pin(async move { agent.generate(runtime, branch, input, prompt).await })
        })
        .observed_as(format!("{}_solution", branch.slug())),
    )
    .with_findings::<()>()
    .build()
}

pub fn build_solution_generation_step<R: 'static, A>(
    agent: Arc<A>,
) -> Step<R, SolutionInput, Vec<SolutionDraft>, (), WorkflowError>
where
    A: SolutionBranchAgent<R>,
{
    let conservative = build_solution_branch_step(agent.clone(), SolutionBranch::Conservative);
    let recommended = build_solution_branch_step(agent.clone(), SolutionBranch::Recommended);
    let ambitious = build_solution_branch_step(agent, SolutionBranch::Ambitious);

    conservative
        .join(recommended)
        .reconcile_task(
            task_fn(|_runtime: &R, input: (SolutionDraft, SolutionDraft)| {
                Box::pin(async move { Ok::<_, WorkflowError>(vec![input.0, input.1]) })
            })
            .observed_as("collect_solution_pair"),
        )
        .join(ambitious)
        .reconcile_task(
            task_fn(|_runtime: &R, input: (Vec<SolutionDraft>, SolutionDraft)| {
                Box::pin(async move {
                    let mut drafts = input.0;
                    drafts.push(input.1);
                    Ok::<_, WorkflowError>(drafts)
                })
            })
            .observed_as("collect_solution_set"),
        )
}

pub fn parse_solution_choice(
    answer: &str,
    collection: &SolutionCollection,
) -> Result<SolutionUserChoice, WorkflowError> {
    let trimmed = answer.trim();
    let normalised = trimmed.to_ascii_lowercase();

    match normalised.as_str() {
        "conservative" | "recommended" | "ambitious" => {
            let draft = collection
                .drafts
                .iter()
                .find(|draft| draft.branch.slug() == normalised)
                .ok_or_else(|| {
                    WorkflowError::InvalidChoice(format!(
                        "no `{}` branch exists in the collected solutions",
                        normalised
                    ))
                })?;

            Ok(SolutionUserChoice::Selected(SelectedSolution {
                choice_label: draft.branch.slug().to_string(),
                branch_sources: vec![draft.branch],
                title: draft.title.clone(),
                summary: draft.summary.clone(),
                architecture: draft.architecture.clone(),
                delivery_plan: draft.delivery_plan.clone(),
                technologies: draft.technologies.clone(),
                rationale: draft.rationale.clone(),
                risks: draft.risks.clone(),
                recommended_for_stage: WorkflowStageId::SoftwareArchitect,
            }))
        }
        "hybrid" => {
            let hybrid = collection
                .recommendation
                .recommended_hybrid
                .as_ref()
                .ok_or_else(|| {
                    WorkflowError::InvalidChoice(
                        "the collected solutions do not expose a hybrid recommendation".to_string(),
                    )
                })?;

            Ok(SolutionUserChoice::Selected(SelectedSolution {
                choice_label: "hybrid".to_string(),
                branch_sources: hybrid.source_branches.clone(),
                title: hybrid.title.clone(),
                summary: hybrid.summary.clone(),
                architecture: hybrid.architecture.clone(),
                delivery_plan: hybrid.delivery_plan.clone(),
                technologies: hybrid.technologies.clone(),
                rationale: hybrid.rationale.clone(),
                risks: hybrid.risks.clone(),
                recommended_for_stage: WorkflowStageId::SoftwareArchitect,
            }))
        }
        "revise" => Ok(SolutionUserChoice::Revise {
            feedback: "User requested revisions to the presented solution set.".to_string(),
        }),
        _ if normalised.starts_with("revise") || normalised.starts_with("reject") => {
            Ok(SolutionUserChoice::Revise {
                feedback: trimmed.to_string(),
            })
        }
        _ => Err(WorkflowError::InvalidChoice(format!(
            "unrecognised solution choice `{trimmed}`"
        ))),
    }
}

fn choice_options(collection: &SolutionCollection) -> Vec<String> {
    let mut options = collection
        .drafts
        .iter()
        .map(|draft| draft.branch.slug().to_string())
        .collect::<Vec<_>>();
    if collection.recommendation.recommended_hybrid.is_some() {
        options.push("hybrid".to_string());
    }
    options.push("revise".to_string());
    options
}

fn describe_recommendation(recommendation: &SolutionRecommendation) -> String {
    if let Some(hybrid) = &recommendation.recommended_hybrid {
        return format!(
            "hybrid from {}",
            hybrid
                .source_branches
                .iter()
                .map(|branch| branch.slug())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    recommendation
        .recommended_branch
        .map(|branch| branch.slug().to_string())
        .unwrap_or_else(|| "no single recommendation".to_string())
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Arc};

    use parking_lot::Mutex;

    use crate::{
        runtime::ScriptedRuntime,
        workflow::{
            WorkflowStageId,
            discovery::{DiscoveryOutcome, DiscoveryState},
            knowledge::StageKnowledgeSession,
        },
    };

    use super::*;

    #[derive(Default)]
    struct StubSolutionAgent;

    impl SolutionBranchAgent<ScriptedRuntime> for StubSolutionAgent {
        fn generate<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            branch: SolutionBranch,
            _input: SolutionInput,
            _prompt: String,
        ) -> LocalBoxFuture<'a, Result<SolutionDraft, WorkflowError>> {
            Box::pin(async move {
                Ok(SolutionDraft {
                    branch,
                    title: format!("{} design", branch.slug()),
                    summary: format!("{} summary", branch.slug()),
                    scope: format!("{} scope", branch.slug()),
                    architecture: vec![format!("{} architecture", branch.slug())],
                    delivery_plan: vec![format!("{} plan", branch.slug())],
                    technologies: vec![format!("{} tech", branch.slug())],
                    rationale: format!("{} rationale", branch.slug()),
                    risks: vec![format!("{} risk", branch.slug())],
                })
            })
        }
    }

    struct StubCollectAgent {
        collections: Mutex<VecDeque<SolutionCollection>>,
    }

    impl StubCollectAgent {
        fn new(collections: Vec<SolutionCollection>) -> Self {
            Self {
                collections: Mutex::new(collections.into()),
            }
        }
    }

    impl SolutionCollectAgent<ScriptedRuntime> for StubCollectAgent {
        fn collect<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            _drafts: Vec<SolutionDraft>,
            _prompt: String,
        ) -> LocalBoxFuture<'a, Result<SolutionCollection, WorkflowError>> {
            let collection = self
                .collections
                .lock()
                .pop_front()
                .expect("stub collection should exist");
            Box::pin(async move { Ok(collection) })
        }
    }

    fn sample_solution_input() -> SolutionInput {
        SolutionInput {
            discovery: DiscoveryOutcome {
                state: DiscoveryState {
                    ready_for_solution: true,
                    problem_statement: "Rewrite MMAT".to_string(),
                    goals: vec!["Use subject-oriented workflow modules".to_string()],
                    constraints: vec!["Use scoped knowledge".to_string()],
                    assumptions: vec![],
                    risks: vec!["Architecture drift".to_string()],
                    notes: vec![],
                    recommended_path: "Generate branches".to_string(),
                    open_questions: vec![],
                },
                answers: Vec::new(),
            },
            knowledge: StageKnowledgeSession {
                stage: WorkflowStageId::Solutions,
                system_prompt: "Scoped knowledge prompt".to_string(),
                group_collections: vec!["workspace-code-repo".to_string()],
            },
        }
    }

    #[tokio::test]
    async fn solution_generation_preserves_three_distinct_branches() {
        let runtime = ScriptedRuntime::new(std::iter::empty::<String>());
        let step = build_solution_generation_step(Arc::new(StubSolutionAgent));

        let drafts = step
            .run(&runtime, sample_solution_input())
            .await
            .expect("solution generation should succeed");

        assert_eq!(drafts.len(), 3);
        assert!(
            drafts
                .iter()
                .any(|draft| draft.branch == SolutionBranch::Conservative)
        );
        assert!(
            drafts
                .iter()
                .any(|draft| draft.branch == SolutionBranch::Recommended)
        );
        assert!(
            drafts
                .iter()
                .any(|draft| draft.branch == SolutionBranch::Ambitious)
        );
    }

    #[tokio::test]
    async fn collect_step_can_return_a_hybrid_recommendation() {
        let runtime = ScriptedRuntime::new(std::iter::empty::<String>());
        let step = build_collect_step(Arc::new(StubCollectAgent::new(vec![SolutionCollection {
            drafts: Vec::new(),
            recommendation: SolutionRecommendation {
                recommended_branch: None,
                recommended_hybrid: Some(HybridSolution {
                    title: "Hybrid path".to_string(),
                    summary: "Blend safety and leverage".to_string(),
                    source_branches: vec![SolutionBranch::Conservative, SolutionBranch::Ambitious],
                    architecture: vec!["Hybrid architecture".to_string()],
                    delivery_plan: vec!["Hybrid plan".to_string()],
                    technologies: vec!["Rust".to_string()],
                    rationale: "Best of both".to_string(),
                    risks: vec!["Scope control".to_string()],
                }),
                rationale: "Use a hybrid".to_string(),
            },
        }])));

        let collection = step
            .run(&runtime, Vec::new())
            .await
            .expect("collect step should succeed");

        assert!(collection.recommendation.recommended_hybrid.is_some());
    }

    #[test]
    fn parse_solution_choice_routes_revision_feedback() {
        let collection = SolutionCollection {
            drafts: Vec::new(),
            recommendation: SolutionRecommendation {
                recommended_branch: Some(SolutionBranch::Recommended),
                recommended_hybrid: None,
                rationale: "recommended is the best default".to_string(),
            },
        };

        let choice = parse_solution_choice("revise: keep it smaller", &collection)
            .expect("revision feedback should parse");

        assert_eq!(
            choice,
            SolutionUserChoice::Revise {
                feedback: "revise: keep it smaller".to_string()
            }
        );
    }

    #[tokio::test]
    async fn explicit_revise_choice_prompts_for_feedback() {
        let runtime = ScriptedRuntime::new(["revise", "Keep the first milestone smaller"]);
        let step = build_choice_step::<ScriptedRuntime>();
        let collection = SolutionCollection {
            drafts: vec![SolutionDraft {
                branch: SolutionBranch::Recommended,
                title: "Recommended path".to_string(),
                summary: "Balanced default".to_string(),
                scope: "Scoped rewrite".to_string(),
                architecture: vec!["workflow/discovery".to_string()],
                delivery_plan: vec!["Build discovery first".to_string()],
                technologies: vec!["Rust".to_string()],
                rationale: "Best default".to_string(),
                risks: vec!["Rewrite churn".to_string()],
            }],
            recommendation: SolutionRecommendation {
                recommended_branch: Some(SolutionBranch::Recommended),
                recommended_hybrid: None,
                rationale: "recommended is the best default".to_string(),
            },
        };

        let choice = step
            .run(&runtime, collection)
            .await
            .expect("choice step should succeed");

        assert_eq!(
            choice,
            SolutionUserChoice::Revise {
                feedback: "Keep the first milestone smaller".to_string()
            }
        );
        assert_eq!(runtime.asked_questions().len(), 2);
    }

    #[test]
    fn solution_prompt_includes_scoped_knowledge_prompt() {
        let prompt =
            build_solution_branch_prompt(SolutionBranch::Recommended, &sample_solution_input());

        assert!(prompt.contains("Scoped knowledge prompt"));
        assert!(prompt.contains("workspace-code-repo"));
    }
}
