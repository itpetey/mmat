use std::fmt::{Debug, Display};

use futures::future;
use naaf_core::{Attempt, RetryPolicy, Step, check_fn, repair_fn};
use naaf_llm::{HumanIO, LlmAgent, LlmClient};
use serde::{Deserialize, Serialize};

use crate::workflow::{
    WorkflowBuildError, WorkflowTaskError, discovery::DiscoveryOutput,
    knowledge::StageKnowledgeSession, parser::decode_outcome, solutions::SelectedSolution,
};

pub(super) type ArchitectStep<C, R, E> =
    Step<R, ArchitectInput, ArchitectPlan, ArchitectFinding, WorkflowTaskError<C, R, E>>;

const MAX_ARCHITECT_ATTEMPTS: usize = 3;
pub const MODEL: &str = "qwen/qwen3.6-27b";
pub const SYSTEM_PROMPT: &str = "You are the software architect stage for MMAT. Your job is to refine the selected solution into an execution-ready architectural handoff.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ArchitectInput {
    discovery: DiscoveryOutput,
    selected_solution: SelectedSolution,
    knowledge: StageKnowledgeSession,
    turn: usize,
    findings: Vec<ArchitectFinding>,
    prior_plan: Option<ArchitectPlan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ArchitectPlan {
    summary: String,
    architecture_decisions: Vec<String>,
    implementation_guidance: Vec<String>,
    planning_notes: Vec<String>,
    risks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum ArchitectFinding {
    Summary,
    ArchitectureDecisions,
    ImplementationGuidance,
    PlanningNotes,
    Risks,
}

impl ArchitectInput {
    pub(super) fn new(
        discovery: DiscoveryOutput,
        selected_solution: SelectedSolution,
        knowledge: StageKnowledgeSession,
    ) -> Self {
        Self {
            discovery,
            selected_solution,
            knowledge,
            turn: 0,
            findings: Vec::new(),
            prior_plan: None,
        }
    }
}

impl Display for ArchitectFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Summary => write!(f, "architect plan is missing a summary"),
            Self::ArchitectureDecisions => {
                write!(f, "architect plan is missing architecture decisions")
            }
            Self::ImplementationGuidance => {
                write!(f, "architect plan is missing implementation guidance")
            }
            Self::PlanningNotes => write!(f, "architect plan is missing planning notes"),
            Self::Risks => write!(f, "architect plan is missing risks"),
        }
    }
}

pub(super) fn step<C, R, E>(agent: &LlmAgent<C, R, E>) -> ArchitectStep<C, R, E>
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
        "software-architect".into(),
    ))
    .validate(check_fn(|r, i, o| Box::pin(future::ok(validate(r, i, o)))))
    .repair_with(repair_fn(|_r, a| {
        Box::pin(async move { repair(a).await.map_err(|error| match error {}) })
    }))
    .retry_policy(RetryPolicy::new(MAX_ARCHITECT_ATTEMPTS))
    .build_persistent()
}

fn build_prompt(input: ArchitectInput) -> String {
    let mut lines = vec![
        input.knowledge.system_prompt.clone(),
        String::new(),
        format!("Architect turn: {}", input.turn + 1),
        "Refine the selected solution into an execution-ready architectural handoff.".to_string(),
        format!("Problem statement: {}", input.discovery.problem_statement),
        format!("Selected solution: {}", input.selected_solution.title),
        format!("Selection summary: {}", input.selected_solution.summary),
        format!(
            "Knowledge groups in scope: {}",
            input.knowledge.group_collections.join(", ")
        ),
    ];

    if !input.selected_solution.risks.is_empty() {
        lines.push(format!(
            "Selected-solution risks: {}",
            input.selected_solution.risks.join(" | ")
        ));
    }

    if let Some(prior_plan) = &input.prior_plan {
        lines.push(String::new());
        lines.push("Prior architect handoff:".to_string());
        lines.push(format!("Summary: {}", prior_plan.summary));
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
        "The JSON object must use this exact shape: {\"summary\":string,\"architecture_decisions\":string[],\"implementation_guidance\":string[],\"planning_notes\":string[],\"risks\":string[]}."
            .to_string(),
    );

    lines.join("\n")
}

async fn repair(
    attempts: Vec<Attempt<ArchitectInput, ArchitectPlan, ArchitectFinding>>,
) -> Result<ArchitectInput, std::convert::Infallible> {
    let latest_attempt = attempts
        .last()
        .expect("architect repair requires an attempt");

    Ok(ArchitectInput {
        discovery: latest_attempt.input.discovery.clone(),
        selected_solution: latest_attempt.input.selected_solution.clone(),
        knowledge: latest_attempt.input.knowledge.clone(),
        turn: latest_attempt.input.turn + 1,
        findings: latest_attempt.findings.clone(),
        prior_plan: Some(latest_attempt.output.clone()),
    })
}

fn validate<R>(_runtime: &R, _input: ArchitectInput, plan: ArchitectPlan) -> Vec<ArchitectFinding> {
    let mut findings = Vec::new();

    if plan.summary.trim().is_empty() {
        findings.push(ArchitectFinding::Summary);
    }

    if !has_non_empty_entries(&plan.architecture_decisions) {
        findings.push(ArchitectFinding::ArchitectureDecisions);
    }

    if !has_non_empty_entries(&plan.implementation_guidance) {
        findings.push(ArchitectFinding::ImplementationGuidance);
    }

    if !has_non_empty_entries(&plan.planning_notes) {
        findings.push(ArchitectFinding::PlanningNotes);
    }

    if !has_non_empty_entries(&plan.risks) {
        findings.push(ArchitectFinding::Risks);
    }

    findings
}

fn has_non_empty_entries(values: &[String]) -> bool {
    values.iter().any(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{WorkflowStageId, solutions::SolutionBranch};

    fn sample_input() -> ArchitectInput {
        ArchitectInput::new(
            DiscoveryOutput {
                ready_for_solution: true,
                problem_statement: "Rewrite MMAT".to_string(),
                goals: vec!["Keep stages readable".to_string()],
                constraints: vec!["Use scoped knowledge".to_string()],
                assumptions: Vec::new(),
                risks: Vec::new(),
                notes: Vec::new(),
                recommended_path: "Architect selected branch".to_string(),
                open_questions: Vec::new(),
            },
            SelectedSolution {
                choice_label: "recommended".to_string(),
                branch_sources: vec![SolutionBranch::Recommended],
                title: "Recommended design".to_string(),
                summary: "Use native workflow modules".to_string(),
                architecture: vec!["Keep stage ownership local".to_string()],
                delivery_plan: vec!["Port remaining stages".to_string()],
                technologies: vec!["Rust".to_string()],
                rationale: "Best default".to_string(),
                risks: vec!["Model output drift".to_string()],
                recommended_for_stage: WorkflowStageId::SoftwareArchitect,
            },
            StageKnowledgeSession {
                stage: WorkflowStageId::SoftwareArchitect,
                system_prompt: "Architect prompt".to_string(),
                group_collections: vec!["workspace-code-repo".to_string()],
            },
        )
    }

    #[test]
    fn architect_prompt_includes_selected_solution_and_json_contract() {
        let prompt = build_prompt(sample_input());

        assert!(prompt.contains("Architect prompt"));
        assert!(prompt.contains("Selected solution: Recommended design"));
        assert!(prompt.contains("workspace-code-repo"));
        assert!(prompt.contains("Return only one JSON object"));
        assert!(prompt.contains("\"architecture_decisions\":string[]"));
    }

    #[test]
    fn architect_validation_reports_missing_handoff_sections() {
        let findings = validate(
            &(),
            sample_input(),
            ArchitectPlan {
                summary: String::new(),
                architecture_decisions: Vec::new(),
                implementation_guidance: vec!["Build in small stages".to_string()],
                planning_notes: Vec::new(),
                risks: Vec::new(),
            },
        );

        assert_eq!(
            findings,
            vec![
                ArchitectFinding::Summary,
                ArchitectFinding::ArchitectureDecisions,
                ArchitectFinding::PlanningNotes,
                ArchitectFinding::Risks,
            ]
        );
    }
}
