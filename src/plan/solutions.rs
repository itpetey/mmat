use std::fmt::{Debug, Display};

use futures::future;
use naaf_core::{Attempt, RetryPolicy, Step, check_fn, repair_fn, task_fn};
use naaf_llm::{HumanIO, HumanQuestion, LlmAgent, LlmClient, TaskError};
use serde::{Deserialize, Serialize};

use crate::plan::{
    WorkflowBuildError, WorkflowStageId, WorkflowTaskError, discovery::DiscoveryOutput,
    knowledge::StageKnowledgeSession, parser::decode_outcome,
};

pub(super) type SolutionBranchStep<C, R, E> =
    Step<R, SolutionBranchInput, SolutionDraft, SolutionBranchFinding, WorkflowTaskError<C, R, E>>;
type SolutionCollectStep<C, R, E> = Step<
    R,
    SolutionCollectInput,
    SolutionCollection,
    SolutionCollectFinding,
    WorkflowTaskError<C, R, E>,
>;

const MAX_BRANCH_ATTEMPTS: usize = 3;
const MAX_COLLECT_ATTEMPTS: usize = 3;
pub const BRANCH_SYSTEM_PROMPT: &str = "You are a solution branch planner for MMAT. Your job is to produce one distinct implementation direction that can be compared with alternatives.";
pub const COLLECT_SYSTEM_PROMPT: &str = "You are the solution collection stage for MMAT. Your job is to compare candidate branches, preserve the tradeoffs, and recommend either one branch or a coherent hybrid.";
pub const MODEL: &str = "openai/gpt-5.5";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SolutionBranchInput {
    branch: SolutionBranch,
    solution: SolutionInput,
    turn: usize,
    findings: Vec<SolutionBranchFinding>,
    prior_draft: Option<SolutionDraft>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SolutionCollectInput {
    drafts: Vec<SolutionDraft>,
    turn: usize,
    findings: Vec<SolutionCollectFinding>,
    prior_collection: Option<SolutionCollection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SolutionCollection {
    pub(super) drafts: Vec<SolutionDraft>,
    pub(super) recommendation: SolutionRecommendation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SolutionRecommendation {
    recommended_branch: Option<SolutionBranch>,
    recommended_hybrid: Option<HybridSolution>,
    rationale: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SolutionDraft {
    branch: SolutionBranch,
    title: String,
    summary: String,
    scope: String,
    architecture: Vec<String>,
    delivery_plan: Vec<String>,
    technologies: Vec<String>,
    rationale: String,
    risks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct HybridSolution {
    title: String,
    summary: String,
    source_branches: Vec<SolutionBranch>,
    architecture: Vec<String>,
    delivery_plan: Vec<String>,
    technologies: Vec<String>,
    rationale: String,
    risks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SelectedSolution {
    pub(super) choice_label: String,
    pub(super) branch_sources: Vec<SolutionBranch>,
    pub(super) title: String,
    pub(super) summary: String,
    pub(super) architecture: Vec<String>,
    pub(super) delivery_plan: Vec<String>,
    pub(super) technologies: Vec<String>,
    pub(super) rationale: String,
    pub(super) risks: Vec<String>,
    pub(super) recommended_for_stage: WorkflowStageId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum SolutionUserChoice {
    Selected(SelectedSolution),
    Revise { feedback: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum SolutionBranchFinding {
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
pub(super) enum SolutionCollectFinding {
    EmptyDrafts,
    MissingDraftBranch(SolutionBranch),
    DuplicateDraftBranch(SolutionBranch),
    MissingRecommendationRationale,
    RecommendedBranchNotPresent(SolutionBranch),
    RecommendedHybridHasNoSources,
    RecommendedHybridUnknownBranch(SolutionBranch),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SolutionInput {
    discovery: DiscoveryOutput,
    knowledge: StageKnowledgeSession,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) enum SolutionBranch {
    Conservative,
    Recommended,
    Ambitious,
}

impl SolutionBranchInput {
    pub(super) fn new(branch: SolutionBranch, solution: SolutionInput) -> Self {
        Self {
            branch,
            solution,
            turn: 0,
            findings: Vec::new(),
            prior_draft: None,
        }
    }
}

impl SolutionInput {
    pub(super) fn new(discovery: DiscoveryOutput, knowledge: StageKnowledgeSession) -> Self {
        Self {
            discovery,
            knowledge,
        }
    }
}

impl SolutionCollectInput {
    pub(super) fn new(drafts: Vec<SolutionDraft>) -> Self {
        Self {
            drafts,
            turn: 0,
            findings: Vec::new(),
            prior_collection: None,
        }
    }
}

impl SolutionBranch {
    fn slug(self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Recommended => "recommended",
            Self::Ambitious => "ambitious",
        }
    }

    fn instruction(self) -> &'static str {
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

impl Display for SolutionBranchFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongBranch(branch) => {
                write!(
                    f,
                    "draft returned the wrong branch; expected `{}`",
                    branch.slug()
                )
            }
            Self::MissingTitle => write!(f, "draft is missing a title"),
            Self::MissingSummary => write!(f, "draft is missing a summary"),
            Self::MissingScope => write!(f, "draft is missing a scope"),
            Self::MissingArchitecture => write!(f, "draft is missing architecture guidance"),
            Self::MissingDeliveryPlan => write!(f, "draft is missing a delivery plan"),
            Self::MissingTechnologies => write!(f, "draft is missing technologies"),
            Self::MissingRationale => write!(f, "draft is missing rationale"),
            Self::MissingRisks => write!(f, "draft is missing risks"),
        }
    }
}

impl Display for SolutionCollectFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDrafts => write!(f, "solution collection received no drafts"),
            Self::MissingDraftBranch(branch) => {
                write!(
                    f,
                    "solution collection is missing the `{}` branch",
                    branch.slug()
                )
            }
            Self::DuplicateDraftBranch(branch) => {
                write!(
                    f,
                    "solution collection duplicated the `{}` branch",
                    branch.slug()
                )
            }
            Self::MissingRecommendationRationale => {
                write!(f, "solution recommendation is missing rationale")
            }
            Self::RecommendedBranchNotPresent(branch) => {
                write!(
                    f,
                    "recommended branch `{}` is not present in the draft set",
                    branch.slug()
                )
            }
            Self::RecommendedHybridHasNoSources => {
                write!(f, "recommended hybrid does not cite any source branches")
            }
            Self::RecommendedHybridUnknownBranch(branch) => {
                write!(
                    f,
                    "recommended hybrid references unknown branch `{}`",
                    branch.slug()
                )
            }
        }
    }
}

pub(super) fn branch_step<C, R, E>(agent: &LlmAgent<C, R, E>) -> SolutionBranchStep<C, R, E>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    Step::builder(agent.json_task(
        MODEL.into(),
        BRANCH_SYSTEM_PROMPT.into(),
        |i| Ok::<_, WorkflowBuildError<R::Error>>(build_branch_prompt(i)),
        decode_outcome,
        "solution-branch".into(),
    ))
    .validate(check_fn(|r, i, o| {
        Box::pin(future::ok(validate_branch(r, i, o)))
    }))
    .repair_with(repair_fn(|_r, a| {
        Box::pin(async move { repair_branch(a).await.map_err(|error| match error {}) })
    }))
    .retry_policy(RetryPolicy::new(MAX_BRANCH_ATTEMPTS))
    .build_persistent()
}

pub(super) fn choice_step<C, R, E>()
-> Step<R, SolutionCollection, SolutionUserChoice, (), WorkflowTaskError<C, R, E>>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    Step::builder(task_fn(
        move |runtime: &R, collection: SolutionCollection| {
            Box::pin(async move {
                let reply = runtime
                    .ask(HumanQuestion {
                        question: build_choice_prompt(&collection),
                        choices: Some(choice_options(&collection)),
                    })
                    .await
                    .map_err(|error| TaskError::Build(WorkflowBuildError::Human(error)))?;

                if reply.content.trim().eq_ignore_ascii_case("revise") {
                    let feedback = runtime
                        .ask(HumanQuestion {
                            question:
                                "Describe the revisions you want to the presented solution set."
                                    .to_string(),
                            choices: None,
                        })
                        .await
                        .map_err(|error| TaskError::Build(WorkflowBuildError::Human(error)))?;
                    return Ok(SolutionUserChoice::Revise {
                        feedback: feedback.content,
                    });
                }

                parse_solution_choice(&reply.content, &collection).map_err(TaskError::Build)
            })
        },
    ))
    .with_findings::<()>()
    .build_persistent()
}

pub(super) fn collect_step<C, R, E>(agent: &LlmAgent<C, R, E>) -> SolutionCollectStep<C, R, E>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    Step::builder(agent.json_task(
        MODEL.into(),
        COLLECT_SYSTEM_PROMPT.into(),
        |i| Ok::<_, WorkflowBuildError<R::Error>>(build_collect_prompt(i)),
        decode_outcome,
        "solution-collect".into(),
    ))
    .validate(check_fn(|r, i, o| {
        Box::pin(future::ok(validate_collection(r, i, o)))
    }))
    .repair_with(repair_fn(|_r, a| {
        Box::pin(async move { repair_collection(a).await.map_err(|error| match error {}) })
    }))
    .retry_policy(RetryPolicy::new(MAX_COLLECT_ATTEMPTS))
    .build_persistent()
}

fn build_branch_prompt(input: SolutionBranchInput) -> String {
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
            solution.discovery.problem_statement
        ),
        format!(
            "Knowledge groups in scope: {}",
            solution.knowledge.group_collections.join(", ")
        ),
    ];

    if !solution.discovery.constraints.is_empty() {
        lines.push(format!(
            "Constraints: {}",
            solution.discovery.constraints.join(" | ")
        ));
    }

    if !solution.discovery.risks.is_empty() {
        lines.push(format!(
            "Known risks: {}",
            solution.discovery.risks.join(" | ")
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
        lines.extend(input.findings.iter().map(|finding| format!("- {finding}")));
    }

    lines.push(String::new());
    lines.push(
        "Return only one JSON object. Do not include markdown, prose, code fences, or hidden reasoning in the assistant content."
            .to_string(),
    );
    lines.push(
        "The JSON object must use this exact shape: {\"branch\":string,\"title\":string,\"summary\":string,\"scope\":string,\"architecture\":string[],\"delivery_plan\":string[],\"technologies\":string[],\"rationale\":string,\"risks\":string[]}."
            .to_string(),
    );
    lines.push(
        "Valid branch values are Conservative, Recommended, and Ambitious. The branch field must match the requested branch."
            .to_string(),
    );

    lines.join("\n")
}

fn build_choice_prompt(collection: &SolutionCollection) -> String {
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

fn build_collect_prompt(input: SolutionCollectInput) -> String {
    let mut lines = vec![
        format!("Collection turn: {}", input.turn + 1),
        "Collect the candidate solution branches, recommend one branch or a hybrid, and explain the rationale."
            .to_string(),
        String::new(),
        "Candidate branches:".to_string(),
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
        lines.extend(input.findings.iter().map(|finding| format!("- {finding}")));
    }

    lines.push(String::new());
    lines.push(
        "Return only one JSON object. Do not include markdown, prose, code fences, or hidden reasoning in the assistant content."
            .to_string(),
    );
    lines.push(
        "The JSON object must use this exact shape: {\"drafts\":[{\"branch\":string,\"title\":string,\"summary\":string,\"scope\":string,\"architecture\":string[],\"delivery_plan\":string[],\"technologies\":string[],\"rationale\":string,\"risks\":string[]}],\"recommendation\":{\"recommended_branch\":string|null,\"recommended_hybrid\":{\"title\":string,\"summary\":string,\"source_branches\":string[],\"architecture\":string[],\"delivery_plan\":string[],\"technologies\":string[],\"rationale\":string,\"risks\":string[]}|null,\"rationale\":string}}."
            .to_string(),
    );
    lines.push(
        "Return exactly one draft for each branch: Conservative, Recommended, and Ambitious."
            .to_string(),
    );

    lines.join("\n")
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

fn has_non_empty_entries(values: &[String]) -> bool {
    values.iter().any(|value| !value.trim().is_empty())
}

fn parse_solution_choice<H>(
    answer: &str,
    collection: &SolutionCollection,
) -> Result<SolutionUserChoice, WorkflowBuildError<H>> {
    let trimmed = answer.trim();
    let normalised = trimmed.to_ascii_lowercase();

    match normalised.as_str() {
        "conservative" | "recommended" | "ambitious" => {
            let draft = collection
                .drafts
                .iter()
                .find(|draft| draft.branch.slug() == normalised)
                .ok_or_else(|| {
                    WorkflowBuildError::InvalidChoice(format!(
                        "no `{normalised}` branch exists in the collected solutions"
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
                    WorkflowBuildError::InvalidChoice(
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
        _ => Err(WorkflowBuildError::InvalidChoice(format!(
            "unrecognised solution choice `{trimmed}`"
        ))),
    }
}

async fn repair_branch(
    attempts: Vec<Attempt<SolutionBranchInput, SolutionDraft, SolutionBranchFinding>>,
) -> Result<SolutionBranchInput, std::convert::Infallible> {
    let latest_attempt = attempts
        .last()
        .expect("solution branch repair requires an attempt");

    Ok(SolutionBranchInput {
        branch: latest_attempt.input.branch,
        solution: latest_attempt.input.solution.clone(),
        turn: latest_attempt.input.turn + 1,
        findings: latest_attempt.findings.clone(),
        prior_draft: Some(latest_attempt.output.clone()),
    })
}

async fn repair_collection(
    attempts: Vec<Attempt<SolutionCollectInput, SolutionCollection, SolutionCollectFinding>>,
) -> Result<SolutionCollectInput, std::convert::Infallible> {
    let latest_attempt = attempts
        .last()
        .expect("solution collect repair requires an attempt");

    Ok(SolutionCollectInput {
        drafts: latest_attempt.input.drafts.clone(),
        turn: latest_attempt.input.turn + 1,
        findings: latest_attempt.findings.clone(),
        prior_collection: Some(latest_attempt.output.clone()),
    })
}

fn validate_branch<R>(
    _runtime: &R,
    input: SolutionBranchInput,
    draft: SolutionDraft,
) -> Vec<SolutionBranchFinding> {
    let mut findings = Vec::new();

    if draft.branch != input.branch {
        findings.push(SolutionBranchFinding::WrongBranch(input.branch));
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

fn validate_collection<R>(
    _runtime: &R,
    _input: SolutionCollectInput,
    collection: SolutionCollection,
) -> Vec<SolutionCollectFinding> {
    let mut findings = Vec::new();
    let drafts = &collection.drafts;

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

    if collection.recommendation.rationale.trim().is_empty() {
        findings.push(SolutionCollectFinding::MissingRecommendationRationale);
    }

    if let Some(branch) = collection.recommendation.recommended_branch
        && !drafts.iter().any(|draft| draft.branch == branch)
    {
        findings.push(SolutionCollectFinding::RecommendedBranchNotPresent(branch));
    }

    if let Some(hybrid) = &collection.recommendation.recommended_hybrid {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_discovery() -> DiscoveryOutput {
        DiscoveryOutput {
            ready_for_solution: true,
            problem_statement: "Rewrite MMAT".to_string(),
            goals: vec!["Keep stages readable".to_string()],
            constraints: vec!["Use scoped knowledge".to_string()],
            assumptions: Vec::new(),
            risks: vec!["Architecture drift".to_string()],
            notes: Vec::new(),
            recommended_path: "Generate solution branches".to_string(),
            open_questions: Vec::new(),
        }
    }

    fn sample_session() -> StageKnowledgeSession {
        StageKnowledgeSession {
            stage: WorkflowStageId::Solutions,
            system_prompt: "Solutions prompt".to_string(),
            group_collections: vec!["workspace-code-repo".to_string()],
        }
    }

    fn sample_draft(branch: SolutionBranch) -> SolutionDraft {
        SolutionDraft {
            branch,
            title: format!("{} design", branch.slug()),
            summary: format!("{} summary", branch.slug()),
            scope: format!("{} scope", branch.slug()),
            architecture: vec![format!("{} architecture", branch.slug())],
            delivery_plan: vec![format!("{} plan", branch.slug())],
            technologies: vec!["Rust".to_string()],
            rationale: format!("{} rationale", branch.slug()),
            risks: vec![format!("{} risk", branch.slug())],
        }
    }

    fn sample_collection() -> SolutionCollection {
        SolutionCollection {
            drafts: vec![
                sample_draft(SolutionBranch::Conservative),
                sample_draft(SolutionBranch::Recommended),
                sample_draft(SolutionBranch::Ambitious),
            ],
            recommendation: SolutionRecommendation {
                recommended_branch: Some(SolutionBranch::Recommended),
                recommended_hybrid: None,
                rationale: "Recommended has the best risk balance".to_string(),
            },
        }
    }

    #[test]
    fn branch_prompt_includes_scoped_context_and_json_contract() {
        let prompt = build_branch_prompt(SolutionBranchInput::new(
            SolutionBranch::Recommended,
            SolutionInput::new(sample_discovery(), sample_session()),
        ));

        assert!(prompt.contains("Solutions prompt"));
        assert!(prompt.contains("Generate the `recommended` solution branch"));
        assert!(prompt.contains("workspace-code-repo"));
        assert!(prompt.contains("Return only one JSON object"));
        assert!(prompt.contains("\"delivery_plan\":string[]"));
    }

    #[test]
    fn branch_validation_reports_wrong_branch_and_empty_fields() {
        let mut draft = sample_draft(SolutionBranch::Conservative);
        draft.title = String::new();
        draft.architecture = Vec::new();

        let findings = validate_branch(
            &(),
            SolutionBranchInput::new(
                SolutionBranch::Recommended,
                SolutionInput::new(sample_discovery(), sample_session()),
            ),
            draft,
        );

        assert_eq!(
            findings,
            vec![
                SolutionBranchFinding::WrongBranch(SolutionBranch::Recommended),
                SolutionBranchFinding::MissingTitle,
                SolutionBranchFinding::MissingArchitecture,
            ]
        );
    }

    #[test]
    fn collection_validation_requires_one_of_each_branch() {
        let mut collection = sample_collection();
        collection.drafts = vec![
            sample_draft(SolutionBranch::Recommended),
            sample_draft(SolutionBranch::Recommended),
        ];
        collection.recommendation.rationale = String::new();

        let findings = validate_collection(&(), SolutionCollectInput::new(Vec::new()), collection);

        assert_eq!(
            findings,
            vec![
                SolutionCollectFinding::MissingDraftBranch(SolutionBranch::Conservative),
                SolutionCollectFinding::DuplicateDraftBranch(SolutionBranch::Recommended),
                SolutionCollectFinding::MissingDraftBranch(SolutionBranch::Ambitious),
                SolutionCollectFinding::MissingRecommendationRationale,
            ]
        );
    }

    #[test]
    fn solution_choice_selects_branch_or_revision_feedback() {
        let selected = parse_solution_choice::<()>("recommended", &sample_collection())
            .expect("branch choice should parse");
        assert!(matches!(
            selected,
            SolutionUserChoice::Selected(SelectedSolution {
                choice_label,
                ..
            }) if choice_label == "recommended"
        ));

        let revision = parse_solution_choice::<()>("revise: reduce scope", &sample_collection())
            .expect("revision choice should parse");
        assert_eq!(
            revision,
            SolutionUserChoice::Revise {
                feedback: "revise: reduce scope".to_string()
            }
        );
    }
}
