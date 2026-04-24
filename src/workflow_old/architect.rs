use std::sync::Arc;

use futures::future::LocalBoxFuture;
use naaf_core::{Attempt, RetryPolicy, Step, TaskExt, check_fn, repair_fn, task_fn};
use serde::{Deserialize, Serialize};

use crate::workflow_old::{
    WorkflowError, discovery::DiscoveryOutcome, knowledge::StageKnowledgeSession,
    solutions::SelectedSolution,
};

pub trait ArchitectAgent<R>: Send + Sync + 'static {
    fn design<'a>(
        &'a self,
        runtime: &'a R,
        input: ArchitectInput,
        prompt: String,
    ) -> LocalBoxFuture<'a, Result<ArchitectPlan, WorkflowError>>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectInput {
    pub discovery: DiscoveryOutcome,
    pub selected_solution: SelectedSolution,
    pub knowledge: StageKnowledgeSession,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectPlan {
    pub summary: String,
    pub architecture_decisions: Vec<String>,
    pub implementation_guidance: Vec<String>,
    pub planning_notes: Vec<String>,
    pub risks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectTurnInput {
    pub request: ArchitectInput,
    pub turn: usize,
    pub findings: Vec<ArchitectFinding>,
    pub prior_plan: Option<ArchitectPlan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectTurn {
    pub input: ArchitectTurnInput,
    pub plan: ArchitectPlan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchitectFinding {
    MissingSummary,
    MissingArchitectureDecisions,
    MissingImplementationGuidance,
    MissingPlanningNotes,
    MissingRisks,
}

impl ArchitectInput {
    pub fn new(
        discovery: DiscoveryOutcome,
        selected_solution: SelectedSolution,
        knowledge: StageKnowledgeSession,
    ) -> Self {
        Self {
            discovery,
            selected_solution,
            knowledge,
        }
    }
}

impl ArchitectTurnInput {
    pub fn new(request: ArchitectInput) -> Self {
        Self {
            request,
            turn: 0,
            findings: Vec::new(),
            prior_plan: None,
        }
    }
}

impl ArchitectFinding {
    pub fn description(&self) -> &'static str {
        match self {
            Self::MissingSummary => "architect plan is missing a summary",
            Self::MissingArchitectureDecisions => {
                "architect plan is missing architecture decisions"
            }
            Self::MissingImplementationGuidance => {
                "architect plan is missing implementation guidance"
            }
            Self::MissingPlanningNotes => "architect plan is missing planning notes",
            Self::MissingRisks => "architect plan is missing risks",
        }
    }
}

pub fn build_architect_prompt(input: &ArchitectTurnInput) -> String {
    let request = &input.request;
    let mut lines = vec![
        request.knowledge.system_prompt.clone(),
        String::new(),
        format!("Architect turn: {}", input.turn + 1),
        "Refine the selected solution into an execution-ready architectural handoff.".to_string(),
        format!(
            "Problem statement: {}",
            request.discovery.state.problem_statement
        ),
        format!("Selected solution: {}", request.selected_solution.title),
        format!("Selection summary: {}", request.selected_solution.summary),
        format!(
            "Knowledge groups in scope: {}",
            request.knowledge.group_collections.join(", ")
        ),
    ];

    if !request.selected_solution.risks.is_empty() {
        lines.push(format!(
            "Selected-solution risks: {}",
            request.selected_solution.risks.join(" | ")
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

fn validate_architect_turn(turn: &ArchitectTurn) -> Vec<ArchitectFinding> {
    let mut findings = Vec::new();

    if turn.plan.summary.trim().is_empty() {
        findings.push(ArchitectFinding::MissingSummary);
    }

    if !has_non_empty_entries(&turn.plan.architecture_decisions) {
        findings.push(ArchitectFinding::MissingArchitectureDecisions);
    }

    if !has_non_empty_entries(&turn.plan.implementation_guidance) {
        findings.push(ArchitectFinding::MissingImplementationGuidance);
    }

    if !has_non_empty_entries(&turn.plan.planning_notes) {
        findings.push(ArchitectFinding::MissingPlanningNotes);
    }

    if !has_non_empty_entries(&turn.plan.risks) {
        findings.push(ArchitectFinding::MissingRisks);
    }

    findings
}

fn repair_architect_input<R>(
    _runtime: &R,
    attempts: Vec<Attempt<ArchitectTurnInput, ArchitectTurn, ArchitectFinding>>,
) -> LocalBoxFuture<'_, Result<ArchitectTurnInput, WorkflowError>>
where
    R: 'static,
{
    Box::pin(async move {
        let latest_attempt = attempts
            .last()
            .expect("architect repair requires an attempt");
        Ok(ArchitectTurnInput {
            request: latest_attempt.artefact.input.request.clone(),
            turn: latest_attempt.artefact.input.turn + 1,
            findings: latest_attempt.findings.clone(),
            prior_plan: Some(latest_attempt.artefact.plan.clone()),
        })
    })
}

pub fn build_architect_step<R: 'static, A>(
    agent: Arc<A>,
    retry_policy: RetryPolicy,
) -> Step<R, ArchitectTurnInput, ArchitectTurn, ArchitectFinding, WorkflowError>
where
    A: ArchitectAgent<R>,
{
    Step::builder(
        task_fn(move |runtime: &R, input: ArchitectTurnInput| {
            let agent = agent.clone();
            let prompt = build_architect_prompt(&input);
            Box::pin(async move {
                let plan = agent.design(runtime, input.request.clone(), prompt).await?;
                Ok(ArchitectTurn { input, plan })
            })
        })
        .observed_as("software_architect"),
    )
    .validate(check_fn(|_runtime: &R, turn: ArchitectTurn| {
        Box::pin(async move { Ok(validate_architect_turn(&turn)) })
    }))
    .repair_with(repair_fn(|runtime: &R, attempts| {
        repair_architect_input(runtime, attempts)
    }))
    .retry_policy(retry_policy)
    .build()
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
            solutions::SelectedSolution,
        },
    };

    use super::*;

    struct StubArchitectAgent {
        plans: Mutex<VecDeque<ArchitectPlan>>,
        prompts: Mutex<Vec<String>>,
    }

    impl StubArchitectAgent {
        fn new(plans: Vec<ArchitectPlan>) -> Self {
            Self {
                plans: Mutex::new(plans.into()),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().clone()
        }
    }

    impl ArchitectAgent<ScriptedRuntime> for StubArchitectAgent {
        fn design<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            _input: ArchitectInput,
            prompt: String,
        ) -> LocalBoxFuture<'a, Result<ArchitectPlan, WorkflowError>> {
            self.prompts.lock().push(prompt);
            let plan = self
                .plans
                .lock()
                .pop_front()
                .expect("stub architect plan should exist");
            Box::pin(async move { Ok(plan) })
        }
    }

    fn sample_architect_input() -> ArchitectInput {
        ArchitectInput::new(
            DiscoveryOutcome {
                state: DiscoveryState {
                    ready_for_solution: true,
                    problem_statement: "Rewrite MMAT".to_string(),
                    goals: vec!["Keep the workflow shape".to_string()],
                    constraints: vec![],
                    assumptions: vec![],
                    risks: vec![],
                    notes: vec![],
                    recommended_path: "Architect the selected branch".to_string(),
                    open_questions: vec![],
                },
                answers: vec![],
            },
            SelectedSolution {
                choice_label: "recommended".to_string(),
                branch_sources: vec![],
                title: "Recommended path".to_string(),
                summary: "Use subject-owned workflow modules".to_string(),
                architecture: vec!["workflow/discovery".to_string()],
                delivery_plan: vec!["Build discovery".to_string()],
                technologies: vec!["Rust".to_string()],
                rationale: "Best default".to_string(),
                risks: vec!["Rewrite complexity".to_string()],
                recommended_for_stage: WorkflowStageId::SoftwareArchitect,
            },
            StageKnowledgeSession {
                stage: WorkflowStageId::SoftwareArchitect,
                system_prompt: "Architect prompt".to_string(),
                group_collections: vec!["workspace-code-repo".to_string()],
            },
        )
    }

    #[tokio::test]
    async fn architect_step_produces_planning_ready_output() {
        let runtime = ScriptedRuntime::new(std::iter::empty::<String>());
        let agent = Arc::new(StubArchitectAgent::new(vec![ArchitectPlan {
            summary: "Architected summary".to_string(),
            architecture_decisions: vec!["Use subject-owned workflow modules".to_string()],
            implementation_guidance: vec!["Plan discovery first".to_string()],
            planning_notes: vec!["Keep stages scoped".to_string()],
            risks: vec!["Upstream NAAF work remains".to_string()],
        }]));
        let step = build_architect_step(agent, RetryPolicy::new(3));

        let plan = step
            .run(&runtime, ArchitectTurnInput::new(sample_architect_input()))
            .await
            .expect("architect step should succeed")
            .plan;

        assert!(plan.summary.contains("Architected"));
        assert_eq!(plan.architecture_decisions.len(), 1);
    }

    #[test]
    fn architect_prompt_includes_scoped_knowledge_prompt() {
        let prompt = build_architect_prompt(&ArchitectTurnInput::new(ArchitectInput::new(
            DiscoveryOutcome {
                state: DiscoveryState {
                    ready_for_solution: true,
                    problem_statement: "Rewrite MMAT".to_string(),
                    goals: vec![],
                    constraints: vec![],
                    assumptions: vec![],
                    risks: vec![],
                    notes: vec![],
                    recommended_path: "Architect it".to_string(),
                    open_questions: vec![],
                },
                answers: vec![],
            },
            SelectedSolution {
                choice_label: "recommended".to_string(),
                branch_sources: vec![],
                title: "Recommended path".to_string(),
                summary: "Use scoped knowledge".to_string(),
                architecture: vec![],
                delivery_plan: vec![],
                technologies: vec![],
                rationale: "Best default".to_string(),
                risks: vec![],
                recommended_for_stage: WorkflowStageId::SoftwareArchitect,
            },
            StageKnowledgeSession {
                stage: WorkflowStageId::SoftwareArchitect,
                system_prompt: "Architect scoped knowledge prompt".to_string(),
                group_collections: vec!["architect-group".to_string()],
            },
        )));

        assert!(prompt.contains("Architect scoped knowledge prompt"));
        assert!(prompt.contains("architect-group"));
    }

    #[tokio::test]
    async fn architect_step_retries_with_validation_findings() {
        let runtime = ScriptedRuntime::new(std::iter::empty::<String>());
        let agent = Arc::new(StubArchitectAgent::new(vec![
            ArchitectPlan {
                summary: String::new(),
                architecture_decisions: vec![],
                implementation_guidance: vec!["Plan discovery first".to_string()],
                planning_notes: vec![],
                risks: vec![],
            },
            ArchitectPlan {
                summary: "Architected summary".to_string(),
                architecture_decisions: vec!["Use subject-owned workflow modules".to_string()],
                implementation_guidance: vec!["Plan discovery first".to_string()],
                planning_notes: vec!["Keep stages scoped".to_string()],
                risks: vec!["Upstream NAAF work remains".to_string()],
            },
        ]));
        let step = build_architect_step(agent.clone(), RetryPolicy::new(3));

        let traced = step
            .run_traced(&runtime, ArchitectTurnInput::new(sample_architect_input()))
            .await
            .expect("architect step should recover");

        assert_eq!(traced.report().attempt_count(), 2);
        assert_eq!(
            traced.report().attempts()[0].findings,
            vec![
                ArchitectFinding::MissingSummary,
                ArchitectFinding::MissingArchitectureDecisions,
                ArchitectFinding::MissingPlanningNotes,
                ArchitectFinding::MissingRisks,
            ]
        );
        assert!(
            agent
                .prompts()
                .last()
                .expect("second architect prompt should exist")
                .contains("architect plan is missing a summary")
        );
    }
}
