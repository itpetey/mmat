use std::sync::Arc;

use futures::future::LocalBoxFuture;
use naaf_core::{Step, TaskExt, task_fn};
use serde::{Deserialize, Serialize};

use crate::workflow::{
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

pub fn build_architect_prompt(input: &ArchitectInput) -> String {
    let mut lines = vec![
        input.knowledge.system_prompt.clone(),
        String::new(),
        "Refine the selected solution into an execution-ready architectural handoff.".to_string(),
        format!(
            "Problem statement: {}",
            input.discovery.state.problem_statement
        ),
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

    lines.join("\n")
}

pub fn build_architect_step<R: 'static, A>(
    agent: Arc<A>,
) -> Step<R, ArchitectInput, ArchitectPlan, (), WorkflowError>
where
    A: ArchitectAgent<R>,
{
    Step::builder(
        task_fn(move |runtime: &R, input: ArchitectInput| {
            let agent = agent.clone();
            let prompt = build_architect_prompt(&input);
            Box::pin(async move { agent.design(runtime, input, prompt).await })
        })
        .observed_as("software_architect"),
    )
    .with_findings::<()>()
    .build()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        runtime::ScriptedRuntime,
        workflow::{
            WorkflowStageId,
            discovery::{DiscoveryOutcome, DiscoveryState},
            knowledge::StageKnowledgeSession,
            solutions::SelectedSolution,
        },
    };

    use super::*;

    #[derive(Default)]
    struct StubArchitectAgent;

    impl ArchitectAgent<ScriptedRuntime> for StubArchitectAgent {
        fn design<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            _input: ArchitectInput,
            _prompt: String,
        ) -> LocalBoxFuture<'a, Result<ArchitectPlan, WorkflowError>> {
            Box::pin(async move {
                Ok(ArchitectPlan {
                    summary: "Architected summary".to_string(),
                    architecture_decisions: vec!["Use subject-owned workflow modules".to_string()],
                    implementation_guidance: vec!["Plan discovery first".to_string()],
                    planning_notes: vec!["Keep stages scoped".to_string()],
                    risks: vec!["Upstream NAAF work remains".to_string()],
                })
            })
        }
    }

    #[tokio::test]
    async fn architect_step_produces_planning_ready_output() {
        let runtime = ScriptedRuntime::new(std::iter::empty::<String>());
        let step = build_architect_step(Arc::new(StubArchitectAgent));

        let plan = step
            .run(
                &runtime,
                ArchitectInput {
                    discovery: DiscoveryOutcome {
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
                    selected_solution: SelectedSolution {
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
                    knowledge: StageKnowledgeSession {
                        stage: WorkflowStageId::SoftwareArchitect,
                        system_prompt: "Architect prompt".to_string(),
                        group_collections: vec!["workspace-code-repo".to_string()],
                    },
                },
            )
            .await
            .expect("architect step should succeed");

        assert!(plan.summary.contains("Architected"));
        assert_eq!(plan.architecture_decisions.len(), 1);
    }

    #[test]
    fn architect_prompt_includes_scoped_knowledge_prompt() {
        let prompt = build_architect_prompt(&ArchitectInput {
            discovery: DiscoveryOutcome {
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
            selected_solution: SelectedSolution {
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
            knowledge: StageKnowledgeSession {
                stage: WorkflowStageId::SoftwareArchitect,
                system_prompt: "Architect scoped knowledge prompt".to_string(),
                group_collections: vec!["architect-group".to_string()],
            },
        });

        assert!(prompt.contains("Architect scoped knowledge prompt"));
        assert!(prompt.contains("architect-group"));
    }
}
