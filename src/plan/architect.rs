use std::fmt::{Debug, Display};

use futures::future;
use naaf_core::{Attempt, RetryPolicy, Step, check_fn, repair_fn, task_fn};
use naaf_knowledge::KnowledgeSearchTool;
use naaf_llm::{AdaptorError, CompletionRequest, Executor, HumanIO, LlmAgent, LlmClient, Message};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::plan::{
    WorkflowBuildError, WorkflowTaskError, discovery::DiscoveryOutput,
    knowledge::StageKnowledgeSession, parser::decode_outcome, solutions::SelectedSolution,
};

pub(super) type ArchitectStep<C, R, E> =
    Step<R, ArchitectInput, ArchitectPlan, ArchitectFinding, WorkflowTaskError<C, R, E>>;

const MAX_ARCHITECT_ATTEMPTS: usize = 3;
pub const MODEL: &str = "openai/gpt-5.5";
pub const SYSTEM_PROMPT: &str = "You are the software architect stage for MMAT. Your job is to refine the selected solution into an execution-ready architectural handoff. If useful, you may request references to existing knowledge groups that have been materialised using `@knowledge_search` tool.";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ArchitectInput {
    discovery: DiscoveryOutput,
    selected_solution: SelectedSolution,
    knowledge: StageKnowledgeSession,
    #[serde(default)]
    materialised: Vec<naaf_knowledge::KnowledgeGroup>,
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
        materialised: Vec<naaf_knowledge::KnowledgeGroup>,
    ) -> Self {
        Self {
            discovery,
            selected_solution,
            knowledge,
            materialised,
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

#[allow(dead_code)]
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

pub(super) fn step_with_knowledge_tools<C, R, E>(
    agent: &LlmAgent<C, R, E>,
    knowledge_backend: std::sync::Arc<crate::plan::knowledge::QdrantKnowledgeBackend<R>>,
) -> ArchitectStep<C, R, E>
where
    C: LlmClient<Runtime = R> + Clone + 'static,
    C::Error: Debug + Display + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    let system_prompt = format!(
        "{}\n\nYou have access to a `knowledge_search` tool to query existing repository knowledge.",
        SYSTEM_PROMPT
    );

    let kb = knowledge_backend.clone();
    let client = (*agent.executor().client()).clone();

    let task = task_fn(move |_runtime: &R, input: ArchitectInput| {
        let kb = kb.clone();
        let system_prompt = system_prompt.clone();
        let client = client.clone();
        Box::pin(async move {
            let user_content = build_prompt(input.clone());
            let request = CompletionRequest::new(
                MODEL.to_string(),
                vec![Message::system(system_prompt), Message::user(user_content)],
            )
            .with_metadata(json!({
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "architect_output",
                        "strict": false,
                        "schema": {
                            "type": "object",
                            "additionalProperties": true
                        }
                    }
                }
            }));

            let mut tools = naaf_llm::ToolRegistry::<R, naaf_knowledge::KnowledgeError>::new();
            for group in &input.materialised {
                let qdrant_agent_result = kb.agent_for_group(group);
                let qdrant_agent = match qdrant_agent_result {
                    Ok(a) => a,
                    Err(e) => {
                        return Err(AdaptorError::Build(WorkflowBuildError::Knowledge(
                            crate::plan::knowledge::KnowledgeError::Knowledge(format!(
                                "failed to get agent for group {}: {e}",
                                group.collection
                            )),
                        )));
                    }
                };
                let embedder = qdrant_agent.clone_embedder();
                let search_tool = KnowledgeSearchTool::new(embedder, 5, 0.7)
                    .with_group(group.clone(), qdrant_agent.client().clone());
                if let Err(e) = tools.register(search_tool) {
                    tracing::warn!(
                        error = %e,
                        collection = %group.collection,
                        "failed to register knowledge search tool"
                    );
                }
            }

            let executor: Executor<C, R, naaf_knowledge::KnowledgeError> =
                Executor::with_tools(client, tools);

            let outcome = executor.execute(_runtime, request).await.map_err(|e| {
                AdaptorError::Build(WorkflowBuildError::Workflow(format!(
                    "executor failed: {e}"
                )))
            })?;

            decode_outcome(outcome).map_err(AdaptorError::Decode)
        })
    });

    Step::builder(task)
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

fn has_non_empty_entries(values: &[String]) -> bool {
    values.iter().any(|value| !value.trim().is_empty())
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
        materialised: latest_attempt.input.materialised.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{WorkflowStageId, solutions::SolutionBranch};

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
                summary: "Use native plan modules".to_string(),
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
            Vec::new(),
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
    fn architect_system_prompt_mentions_knowledge_search() {
        assert!(SYSTEM_PROMPT.contains("@knowledge_search"));
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
