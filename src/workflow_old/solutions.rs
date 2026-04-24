use std::sync::Arc;

use futures::future::{LocalBoxFuture, try_join3};
use naaf_core::{Attempt, RetryPolicy, Step, TaskExt, check_fn, repair_fn, task_fn};
use naaf_llm::{HumanIO, HumanQuestion};
use serde::{Deserialize, Serialize};

use crate::workflow_old::{
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionBranchInput {
    pub branch: SolutionBranch,
    pub solution: SolutionInput,
    pub turn: usize,
    pub findings: Vec<SolutionBranchFinding>,
    pub prior_draft: Option<SolutionDraft>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionBranchTurn {
    pub input: SolutionBranchInput,
    pub draft: SolutionDraft,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolutionBranchFinding {
    WrongBranch(SolutionBranch),
    MissingTitle,
    MissingSummary,
    MissingScope,
    MissingArchitecture,
    MissingDeliveryPlan,
    MissingTechnologies,
    MissingRationale,
    MissingRisks,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionCollectInput {
    pub drafts: Vec<SolutionDraft>,
    pub turn: usize,
    pub findings: Vec<SolutionCollectFinding>,
    pub prior_collection: Option<SolutionCollection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionCollectTurn {
    pub input: SolutionCollectInput,
    pub collection: SolutionCollection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolutionCollectFinding {
    EmptyDrafts,
    MissingDraftBranch(SolutionBranch),
    DuplicateDraftBranch(SolutionBranch),
    MissingRecommendationRationale,
    RecommendedBranchNotPresent(SolutionBranch),
    RecommendedHybridHasNoSources,
    RecommendedHybridUnknownBranch(SolutionBranch),
}

impl SolutionInput {
    pub fn new(discovery: DiscoveryOutcome, knowledge: StageKnowledgeSession) -> Self {
        Self {
            discovery,
            knowledge,
        }
    }
}

impl SolutionBranchInput {
    pub fn new(branch: SolutionBranch, solution: SolutionInput) -> Self {
        Self {
            branch,
            solution,
            turn: 0,
            findings: Vec::new(),
            prior_draft: None,
        }
    }
}

impl SolutionCollectInput {
    pub fn new(drafts: Vec<SolutionDraft>) -> Self {
        Self {
            drafts,
            turn: 0,
            findings: Vec::new(),
            prior_collection: None,
        }
    }
}

impl SolutionBranchFinding {
    pub fn description(&self) -> String {
        match self {
            Self::WrongBranch(branch) => {
                format!(
                    "draft returned the wrong branch; expected `{}`",
                    branch.slug()
                )
            }
            Self::MissingTitle => "draft is missing a title".to_string(),
            Self::MissingSummary => "draft is missing a summary".to_string(),
            Self::MissingScope => "draft is missing a scope".to_string(),
            Self::MissingArchitecture => "draft is missing architecture guidance".to_string(),
            Self::MissingDeliveryPlan => "draft is missing a delivery plan".to_string(),
            Self::MissingTechnologies => "draft is missing technologies".to_string(),
            Self::MissingRationale => "draft is missing rationale".to_string(),
            Self::MissingRisks => "draft is missing risks".to_string(),
        }
    }
}

impl SolutionCollectFinding {
    pub fn description(&self) -> String {
        match self {
            Self::EmptyDrafts => "solution collection received no drafts".to_string(),
            Self::MissingDraftBranch(branch) => {
                format!(
                    "solution collection is missing the `{}` branch",
                    branch.slug()
                )
            }
            Self::DuplicateDraftBranch(branch) => {
                format!(
                    "solution collection duplicated the `{}` branch",
                    branch.slug()
                )
            }
            Self::MissingRecommendationRationale => {
                "solution recommendation is missing rationale".to_string()
            }
            Self::RecommendedBranchNotPresent(branch) => format!(
                "recommended branch `{}` is not present in the draft set",
                branch.slug()
            ),
            Self::RecommendedHybridHasNoSources => {
                "recommended hybrid does not cite any source branches".to_string()
            }
            Self::RecommendedHybridUnknownBranch(branch) => format!(
                "recommended hybrid references unknown branch `{}`",
                branch.slug()
            ),
        }
    }
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

pub fn build_collect_prompt(input: &SolutionCollectInput) -> String {
    let mut lines = vec![
        "Collect the candidate solution branches, recommend one branch or a hybrid, and explain the rationale."
            .to_string(),
        format!("Collection turn: {}", input.turn + 1),
    ];
    lines.extend(input.drafts.iter().map(|draft| {
        format!(
            "- {}: {} :: {}",
            draft.branch.slug(),
            draft.title,
            draft.summary
        )
    }));

    if let Some(prior_collection) = &input.prior_collection {
        lines.push(String::new());
        lines.push("Prior collection output:".to_string());
        lines.push(format!(
            "Recommendation: {}",
            describe_recommendation(&prior_collection.recommendation)
        ));
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

    lines.join("\n")
}

fn has_non_empty_entries(values: &[String]) -> bool {
    values.iter().any(|value| !value.trim().is_empty())
}

fn validate_solution_branch_turn(turn: &SolutionBranchTurn) -> Vec<SolutionBranchFinding> {
    let mut findings = Vec::new();
    let draft = &turn.draft;

    if draft.branch != turn.input.branch {
        findings.push(SolutionBranchFinding::WrongBranch(turn.input.branch));
    }

    if draft.title.trim().is_empty() {
        findings.push(SolutionBranchFinding::MissingTitle);
    }

    if draft.summary.trim().is_empty() {
        findings.push(SolutionBranchFinding::MissingSummary);
    }

    if draft.scope.trim().is_empty() {
        findings.push(SolutionBranchFinding::MissingScope);
    }

    if !has_non_empty_entries(&draft.architecture) {
        findings.push(SolutionBranchFinding::MissingArchitecture);
    }

    if !has_non_empty_entries(&draft.delivery_plan) {
        findings.push(SolutionBranchFinding::MissingDeliveryPlan);
    }

    if !has_non_empty_entries(&draft.technologies) {
        findings.push(SolutionBranchFinding::MissingTechnologies);
    }

    if draft.rationale.trim().is_empty() {
        findings.push(SolutionBranchFinding::MissingRationale);
    }

    if !has_non_empty_entries(&draft.risks) {
        findings.push(SolutionBranchFinding::MissingRisks);
    }

    findings
}

fn repair_solution_branch_input<R>(
    _runtime: &R,
    attempts: Vec<Attempt<SolutionBranchInput, SolutionBranchTurn, SolutionBranchFinding>>,
) -> LocalBoxFuture<'_, Result<SolutionBranchInput, WorkflowError>>
where
    R: 'static,
{
    Box::pin(async move {
        let latest_attempt = attempts
            .last()
            .expect("solution branch repair requires an attempt");
        Ok(SolutionBranchInput {
            branch: latest_attempt.artefact.input.branch,
            solution: latest_attempt.artefact.input.solution.clone(),
            turn: latest_attempt.artefact.input.turn + 1,
            findings: latest_attempt.findings.clone(),
            prior_draft: Some(latest_attempt.artefact.draft.clone()),
        })
    })
}

fn validate_solution_collection_turn(turn: &SolutionCollectTurn) -> Vec<SolutionCollectFinding> {
    let mut findings = Vec::new();
    let drafts = &turn.collection.drafts;

    if drafts.is_empty() {
        findings.push(SolutionCollectFinding::EmptyDrafts);
    }

    for required in [
        SolutionBranch::Conservative,
        SolutionBranch::Recommended,
        SolutionBranch::Ambitious,
    ] {
        let count = drafts
            .iter()
            .filter(|draft| draft.branch == required)
            .count();
        if count == 0 {
            findings.push(SolutionCollectFinding::MissingDraftBranch(required));
        } else if count > 1 {
            findings.push(SolutionCollectFinding::DuplicateDraftBranch(required));
        }
    }

    if turn.collection.recommendation.rationale.trim().is_empty() {
        findings.push(SolutionCollectFinding::MissingRecommendationRationale);
    }

    if let Some(branch) = turn.collection.recommendation.recommended_branch
        && !drafts.iter().any(|draft| draft.branch == branch)
    {
        findings.push(SolutionCollectFinding::RecommendedBranchNotPresent(branch));
    }

    if let Some(hybrid) = &turn.collection.recommendation.recommended_hybrid {
        if hybrid.source_branches.is_empty() {
            findings.push(SolutionCollectFinding::RecommendedHybridHasNoSources);
        }

        for branch in &hybrid.source_branches {
            if !drafts.iter().any(|draft| draft.branch == *branch) {
                findings.push(SolutionCollectFinding::RecommendedHybridUnknownBranch(
                    *branch,
                ));
            }
        }
    }

    findings
}

fn repair_solution_collect_input<R>(
    _runtime: &R,
    attempts: Vec<Attempt<SolutionCollectInput, SolutionCollectTurn, SolutionCollectFinding>>,
) -> LocalBoxFuture<'_, Result<SolutionCollectInput, WorkflowError>>
where
    R: 'static,
{
    Box::pin(async move {
        let latest_attempt = attempts
            .last()
            .expect("solution collect repair requires an attempt");
        Ok(SolutionCollectInput {
            drafts: latest_attempt.artefact.input.drafts.clone(),
            turn: latest_attempt.artefact.input.turn + 1,
            findings: latest_attempt.findings.clone(),
            prior_collection: Some(latest_attempt.artefact.collection.clone()),
        })
    })
}

pub fn build_collect_step<R: 'static, A>(
    agent: Arc<A>,
    retry_policy: RetryPolicy,
) -> Step<R, SolutionCollectInput, SolutionCollectTurn, SolutionCollectFinding, WorkflowError>
where
    A: SolutionCollectAgent<R>,
{
    Step::builder(
        task_fn(move |runtime: &R, input: SolutionCollectInput| {
            let prompt = build_collect_prompt(&input);
            let agent = agent.clone();
            Box::pin(async move {
                let collection = agent.collect(runtime, input.drafts.clone(), prompt).await?;
                Ok(SolutionCollectTurn { input, collection })
            })
        })
        .observed_as("collect_solutions"),
    )
    .validate(check_fn(|_runtime: &R, turn: SolutionCollectTurn| {
        Box::pin(async move { Ok(validate_solution_collection_turn(&turn)) })
    }))
    .repair_with(repair_fn(|runtime: &R, attempts| {
        repair_solution_collect_input(runtime, attempts)
    }))
    .retry_policy(retry_policy)
    .build()
}

pub fn build_solution_branch_prompt(input: &SolutionBranchInput) -> String {
    let branch = input.branch;
    let solution = &input.solution;
    let mut lines = vec![
        solution.knowledge.system_prompt.clone(),
        String::new(),
        format!("Branch generation turn: {}", input.turn + 1),
        format!("Generate the `{}` solution branch.", branch.slug()),
        branch.instruction().to_string(),
        format!(
            "Problem statement: {}",
            solution.discovery.state.problem_statement
        ),
        format!(
            "Knowledge groups in scope: {}",
            solution.knowledge.group_collections.join(", ")
        ),
    ];

    if !solution.discovery.state.constraints.is_empty() {
        lines.push(format!(
            "Constraints: {}",
            solution.discovery.state.constraints.join(" | ")
        ));
    }

    if !solution.discovery.state.risks.is_empty() {
        lines.push(format!(
            "Known risks: {}",
            solution.discovery.state.risks.join(" | ")
        ));
    }

    if let Some(prior_draft) = &input.prior_draft {
        lines.push(String::new());
        lines.push("Prior branch draft:".to_string());
        lines.push(format!("Title: {}", prior_draft.title));
        lines.push(format!("Summary: {}", prior_draft.summary));
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

    lines.join("\n")
}

pub fn build_solution_branch_step<R: 'static, A>(
    agent: Arc<A>,
    retry_policy: RetryPolicy,
) -> Step<R, SolutionBranchInput, SolutionBranchTurn, SolutionBranchFinding, WorkflowError>
where
    A: SolutionBranchAgent<R>,
{
    Step::builder(
        task_fn(move |runtime: &R, input: SolutionBranchInput| {
            let prompt = build_solution_branch_prompt(&input);
            let agent = agent.clone();
            Box::pin(async move {
                let draft = agent
                    .generate(runtime, input.branch, input.solution.clone(), prompt)
                    .await?;
                Ok(SolutionBranchTurn { input, draft })
            })
        })
        .observed_as("solution_branch"),
    )
    .validate(check_fn(|_runtime: &R, turn: SolutionBranchTurn| {
        Box::pin(async move { Ok(validate_solution_branch_turn(&turn)) })
    }))
    .repair_with(repair_fn(|runtime: &R, attempts| {
        repair_solution_branch_input(runtime, attempts)
    }))
    .retry_policy(retry_policy)
    .build()
}

pub fn build_solution_generation_step<R: 'static, A>(
    agent: Arc<A>,
) -> Step<R, SolutionInput, Vec<SolutionDraft>, (), WorkflowError>
where
    A: SolutionBranchAgent<R>,
{
    let conservative = build_solution_branch_step(agent.clone(), RetryPolicy::new(3));
    let recommended = build_solution_branch_step(agent.clone(), RetryPolicy::new(3));
    let ambitious = build_solution_branch_step(agent, RetryPolicy::new(3));

    Step::builder(
        task_fn(move |runtime: &R, input: SolutionInput| {
            let conservative = conservative.clone();
            let recommended = recommended.clone();
            let ambitious = ambitious.clone();
            let conservative_input = input.clone();
            let recommended_input = input.clone();
            let ambitious_input = input;
            Box::pin(async move {
                let (conservative, recommended, ambitious) = try_join3(
                    async {
                        conservative
                            .run(
                                runtime,
                                SolutionBranchInput::new(
                                    SolutionBranch::Conservative,
                                    conservative_input,
                                ),
                            )
                            .await
                            .map_err(|error| {
                                WorkflowError::Solution(format!(
                                    "conservative solution generation failed: {error}"
                                ))
                            })
                    },
                    async {
                        recommended
                            .run(
                                runtime,
                                SolutionBranchInput::new(
                                    SolutionBranch::Recommended,
                                    recommended_input,
                                ),
                            )
                            .await
                            .map_err(|error| {
                                WorkflowError::Solution(format!(
                                    "recommended solution generation failed: {error}"
                                ))
                            })
                    },
                    async {
                        ambitious
                            .run(
                                runtime,
                                SolutionBranchInput::new(
                                    SolutionBranch::Ambitious,
                                    ambitious_input,
                                ),
                            )
                            .await
                            .map_err(|error| {
                                WorkflowError::Solution(format!(
                                    "ambitious solution generation failed: {error}"
                                ))
                            })
                    },
                )
                .await?;

                Ok(vec![conservative.draft, recommended.draft, ambitious.draft])
            })
        })
        .observed_as("generate_solution_set"),
    )
    .with_findings::<()>()
    .build()
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
        workflow_old::{
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

    struct RetryingSolutionAgent {
        drafts: Mutex<VecDeque<SolutionDraft>>,
        prompts: Mutex<Vec<String>>,
    }

    impl RetryingSolutionAgent {
        fn new(drafts: Vec<SolutionDraft>) -> Self {
            Self {
                drafts: Mutex::new(drafts.into()),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().clone()
        }
    }

    impl SolutionBranchAgent<ScriptedRuntime> for RetryingSolutionAgent {
        fn generate<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            _branch: SolutionBranch,
            _input: SolutionInput,
            prompt: String,
        ) -> LocalBoxFuture<'a, Result<SolutionDraft, WorkflowError>> {
            self.prompts.lock().push(prompt);
            let draft = self
                .drafts
                .lock()
                .pop_front()
                .expect("stub draft should exist");
            Box::pin(async move { Ok(draft) })
        }
    }

    struct StubCollectAgent {
        collections: Mutex<VecDeque<SolutionCollection>>,
        prompts: Mutex<Vec<String>>,
    }

    impl StubCollectAgent {
        fn new(collections: Vec<SolutionCollection>) -> Self {
            Self {
                collections: Mutex::new(collections.into()),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().clone()
        }
    }

    impl SolutionCollectAgent<ScriptedRuntime> for StubCollectAgent {
        fn collect<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            _drafts: Vec<SolutionDraft>,
            prompt: String,
        ) -> LocalBoxFuture<'a, Result<SolutionCollection, WorkflowError>> {
            self.prompts.lock().push(prompt);
            let collection = self
                .collections
                .lock()
                .pop_front()
                .expect("stub collection should exist");
            Box::pin(async move { Ok(collection) })
        }
    }

    fn sample_solution_input() -> SolutionInput {
        SolutionInput::new(
            DiscoveryOutcome {
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
            StageKnowledgeSession {
                stage: WorkflowStageId::Solutions,
                system_prompt: "Scoped knowledge prompt".to_string(),
                group_collections: vec!["workspace-code-repo".to_string()],
            },
        )
    }

    fn sample_draft(branch: SolutionBranch) -> SolutionDraft {
        SolutionDraft {
            branch,
            title: format!("{} design", branch.slug()),
            summary: format!("{} summary", branch.slug()),
            scope: format!("{} scope", branch.slug()),
            architecture: vec![format!("{} architecture", branch.slug())],
            delivery_plan: vec![format!("{} plan", branch.slug())],
            technologies: vec![format!("{} tech", branch.slug())],
            rationale: format!("{} rationale", branch.slug()),
            risks: vec![format!("{} risk", branch.slug())],
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
        let drafts = vec![
            sample_draft(SolutionBranch::Conservative),
            sample_draft(SolutionBranch::Recommended),
            sample_draft(SolutionBranch::Ambitious),
        ];
        let step = build_collect_step(
            Arc::new(StubCollectAgent::new(vec![SolutionCollection {
                drafts: drafts.clone(),
                recommendation: SolutionRecommendation {
                    recommended_branch: None,
                    recommended_hybrid: Some(HybridSolution {
                        title: "Hybrid path".to_string(),
                        summary: "Blend safety and leverage".to_string(),
                        source_branches: vec![
                            SolutionBranch::Conservative,
                            SolutionBranch::Ambitious,
                        ],
                        architecture: vec!["Hybrid architecture".to_string()],
                        delivery_plan: vec!["Hybrid plan".to_string()],
                        technologies: vec!["Rust".to_string()],
                        rationale: "Best of both".to_string(),
                        risks: vec!["Scope control".to_string()],
                    }),
                    rationale: "Use a hybrid".to_string(),
                },
            }])),
            RetryPolicy::new(3),
        );

        let collection = step
            .run(&runtime, SolutionCollectInput::new(drafts))
            .await
            .expect("collect step should succeed")
            .collection;

        assert!(collection.recommendation.recommended_hybrid.is_some());
    }

    #[tokio::test]
    async fn solution_branch_step_retries_with_validation_findings() {
        let runtime = ScriptedRuntime::new(std::iter::empty::<String>());
        let agent = Arc::new(RetryingSolutionAgent::new(vec![
            SolutionDraft {
                branch: SolutionBranch::Conservative,
                title: String::new(),
                summary: String::new(),
                scope: String::new(),
                architecture: vec![],
                delivery_plan: vec![],
                technologies: vec![],
                rationale: String::new(),
                risks: vec![],
            },
            sample_draft(SolutionBranch::Recommended),
        ]));
        let step = build_solution_branch_step(agent.clone(), RetryPolicy::new(3));

        let traced = step
            .run_traced(
                &runtime,
                SolutionBranchInput::new(SolutionBranch::Recommended, sample_solution_input()),
            )
            .await
            .expect("solution branch step should recover");

        assert_eq!(traced.report().attempt_count(), 2);
        assert_eq!(
            traced.report().attempts()[0].findings,
            vec![
                SolutionBranchFinding::WrongBranch(SolutionBranch::Recommended),
                SolutionBranchFinding::MissingTitle,
                SolutionBranchFinding::MissingSummary,
                SolutionBranchFinding::MissingScope,
                SolutionBranchFinding::MissingArchitecture,
                SolutionBranchFinding::MissingDeliveryPlan,
                SolutionBranchFinding::MissingTechnologies,
                SolutionBranchFinding::MissingRationale,
                SolutionBranchFinding::MissingRisks,
            ]
        );
        assert!(
            agent
                .prompts()
                .last()
                .expect("second solution prompt should exist")
                .contains("draft is missing a title")
        );
    }

    #[tokio::test]
    async fn collect_step_retries_with_validation_findings() {
        let runtime = ScriptedRuntime::new(std::iter::empty::<String>());
        let drafts = vec![
            sample_draft(SolutionBranch::Conservative),
            sample_draft(SolutionBranch::Recommended),
            sample_draft(SolutionBranch::Ambitious),
        ];
        let agent = Arc::new(StubCollectAgent::new(vec![
            SolutionCollection {
                drafts: vec![
                    sample_draft(SolutionBranch::Recommended),
                    sample_draft(SolutionBranch::Recommended),
                ],
                recommendation: SolutionRecommendation {
                    recommended_branch: Some(SolutionBranch::Recommended),
                    recommended_hybrid: None,
                    rationale: String::new(),
                },
            },
            SolutionCollection {
                drafts: drafts.clone(),
                recommendation: SolutionRecommendation {
                    recommended_branch: Some(SolutionBranch::Recommended),
                    recommended_hybrid: None,
                    rationale: "recommended is the best default".to_string(),
                },
            },
        ]));
        let step = build_collect_step(agent.clone(), RetryPolicy::new(3));

        let traced = step
            .run_traced(&runtime, SolutionCollectInput::new(drafts))
            .await
            .expect("collect step should recover");

        assert_eq!(traced.report().attempt_count(), 2);
        assert_eq!(
            traced.report().attempts()[0].findings,
            vec![
                SolutionCollectFinding::MissingDraftBranch(SolutionBranch::Conservative),
                SolutionCollectFinding::DuplicateDraftBranch(SolutionBranch::Recommended),
                SolutionCollectFinding::MissingDraftBranch(SolutionBranch::Ambitious),
                SolutionCollectFinding::MissingRecommendationRationale,
            ]
        );
        assert!(
            agent
                .prompts()
                .last()
                .expect("second collect prompt should exist")
                .contains("solution recommendation is missing rationale")
        );
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
        let prompt = build_solution_branch_prompt(&SolutionBranchInput::new(
            SolutionBranch::Recommended,
            sample_solution_input(),
        ));

        assert!(prompt.contains("Scoped knowledge prompt"));
        assert!(prompt.contains("workspace-code-repo"));
    }
}
