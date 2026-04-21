use naaf_core::TaskExt;
use naaf_core::{EdgeSpec, GraphPatch, NeverFinding, NodeId, NodeInput, NodeSpec, Step, StepNode};

use crate::{
    models::{ImplementationManagementRequest, ImplementationTaskInput},
    workflow::{
        AppError, AppRuntime, ExecutionGraphSteps, ImplementationExecutionInput, ManagedPhase,
        PhaseExecutionInput, PhaseReviewAction, PhaseReviewResult, WorkflowOutcome,
    },
};

pub fn build_outcome_patch(
    parent_id: NodeId,
    outcome_step: Step<AppRuntime, WorkflowOutcome, WorkflowOutcome, NeverFinding, AppError>,
) -> GraphPatch<AppRuntime, AppError> {
    let node_id = NodeId::new();
    GraphPatch::new()
        .with_node(
            NodeSpec::new(
                super::execution::outcome_node_name(),
                StepNode::without_findings(outcome_step, move |input: &NodeInput| {
                    let result = input.output_as::<PhaseReviewResult>(parent_id)?;
                    Ok(super::execution::workflow_outcome_from_phase_result(
                        &result,
                    ))
                }),
            )
            .with_id(node_id)
            .with_parent(parent_id),
        )
        .with_edge(EdgeSpec::new(parent_id, node_id))
}

pub fn build_outcome_step()
-> Step<AppRuntime, WorkflowOutcome, WorkflowOutcome, NeverFinding, AppError> {
    use naaf_core::task_fn;
    Step::builder(
        task_fn(|_runtime: &AppRuntime, outcome: WorkflowOutcome| {
            Box::pin(async move { Ok::<_, AppError>(outcome) })
        })
        .observed_as("workflow_outcome"),
    )
    .with_findings::<NeverFinding>()
    .build()
}

pub fn build_phase_patch(
    manager_id: NodeId,
    phase: &ManagedPhase,
    execution_steps: ExecutionGraphSteps,
) -> GraphPatch<AppRuntime, AppError> {
    let implementation_step = execution_steps.implementation.clone();
    let phase_review_step = execution_steps.phase_review.clone();
    let mut patch = GraphPatch::new();
    let mut item_ids = Vec::new();

    for item in &phase.worklist.items {
        let item_id = NodeId::new();
        item_ids.push((item_id, item.clone()));
        let safe_phase = super::execution::sanitise_worktree_name(&phase.request.phase);
        let safe_item_id = super::execution::sanitise_worktree_name(&item.id);
        patch = patch
            .with_node(
                NodeSpec::new(
                    super::execution::implementation_node_name(phase.request.pass_index, &item.id),
                    StepNode::new(implementation_step.clone(), {
                        let item = item.clone();
                        let safe_phase = safe_phase.clone();
                        let safe_item_id = safe_item_id.clone();
                        move |input: &NodeInput| {
                            let phase = input.output_as::<ManagedPhase>(manager_id)?;
                            Ok(ImplementationExecutionInput {
                                task: ImplementationTaskInput {
                                    approved: phase.request.approved.clone(),
                                    plan: phase.request.plan.clone(),
                                    work_item: item.clone(),
                                    completed_items: phase.request.completed_items.clone(),
                                    prior_feedback: Vec::new(),
                                },
                                worktree_name: format!("{safe_phase}-{safe_item_id}"),
                            })
                        }
                    }),
                )
                .with_id(item_id)
                .with_parent(manager_id),
            )
            .with_edge(EdgeSpec::new(manager_id, item_id));
    }

    let id_to_node: std::collections::HashMap<&str, NodeId> = item_ids
        .iter()
        .map(|(node_id, item)| (item.id.as_str(), *node_id))
        .collect();

    for (item_id, item) in &item_ids {
        for dep_id in &item.dependencies {
            if let Some(&dep_node_id) = id_to_node.get(dep_id.as_str()) {
                patch = patch.with_edge(EdgeSpec::new(dep_node_id, *item_id));
            }
        }
    }

    let review_id = NodeId::new();
    let review_dependencies = item_ids.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    patch = patch
        .with_node(
            NodeSpec::new(
                super::execution::review_node_name(phase.request.pass_index),
                StepNode::without_findings(phase_review_step.clone(), {
                    let review_dependencies = review_dependencies.clone();
                    move |input: &NodeInput| {
                        let phase = input.output_as::<ManagedPhase>(manager_id)?;
                        let drafts = review_dependencies
                            .iter()
                            .map(|item_id| input.output_as::<super::ImplementationDraft>(*item_id))
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(PhaseExecutionInput { phase, drafts })
                    }
                })
                .spawn_with({
                    let execution_steps = execution_steps.clone();
                    move |context, result| {
                        build_review_patch(context.node_id(), result, execution_steps.clone())
                    }
                }),
            )
            .with_id(review_id)
            .with_parent(manager_id),
        )
        .with_edge(EdgeSpec::new(manager_id, review_id));

    for (item_id, _) in item_ids {
        patch = patch.with_edge(EdgeSpec::new(item_id, review_id));
    }

    patch
}

pub fn build_review_patch(
    review_id: NodeId,
    result: &PhaseReviewResult,
    execution_steps: ExecutionGraphSteps,
) -> GraphPatch<AppRuntime, AppError> {
    match super::execution::phase_review_action(result.phase.request.pass_index, &result.review) {
        PhaseReviewAction::Remediate => {
            let node = remediation_management_node_spec(review_id, execution_steps);
            let node_id = node.id();
            GraphPatch::new()
                .with_node(node)
                .with_edge(EdgeSpec::new(review_id, node_id))
        }
        PhaseReviewAction::Complete | PhaseReviewAction::Halt => {
            build_outcome_patch(review_id, execution_steps.outcome)
        }
    }
}

pub fn root_management_node_spec(
    request: ImplementationManagementRequest,
    execution_steps: ExecutionGraphSteps,
) -> Result<NodeSpec<AppRuntime, AppError>, AppError> {
    NodeSpec::new(
        super::execution::management_node_name(request.pass_index),
        StepNode::without_findings(
            execution_steps.managed_phase.clone(),
            |input: &NodeInput| input.seed_as::<ImplementationManagementRequest>(),
        )
        .spawn_with({
            let execution_steps = execution_steps.clone();
            move |context, phase| {
                build_phase_patch(context.node_id(), phase, execution_steps.clone())
            }
        }),
    )
    .with_seed(request)
    .map_err(AppError::from)
}

fn remediation_management_node_name() -> String {
    "implementation_management_remediation".to_string()
}

fn remediation_management_node_spec(
    review_id: NodeId,
    execution_steps: ExecutionGraphSteps,
) -> NodeSpec<AppRuntime, AppError> {
    NodeSpec::new(
        remediation_management_node_name(),
        StepNode::without_findings(
            execution_steps.managed_phase.clone(),
            move |input: &NodeInput| {
                let result = input.output_as::<PhaseReviewResult>(review_id)?;
                Ok(super::execution::next_management_request(&result))
            },
        )
        .spawn_with({
            let execution_steps = execution_steps.clone();
            move |context, phase| {
                build_phase_patch(context.node_id(), phase, execution_steps.clone())
            }
        }),
    )
    .with_parent(review_id)
}

#[cfg(test)]
mod tests {
    use naaf_core::{NeverFinding, NodeId, Step, task_fn};

    use super::*;
    use crate::models::{
        ApprovalOutcome, ApprovedContract, ApprovedProposal, ExecutionMilestone, ExecutionPlan,
        ImplementationDelta, ImplementationManagementRequest, ProjectContract, ReconciledProposal,
        RemediationItem, StageReview, TaskCard,
    };
    use crate::workflow::ImplementationDraft;
    use crate::{error::AppError, runtime::AppRuntime};

    fn sample_management_request(pass_index: usize) -> ImplementationManagementRequest {
        ImplementationManagementRequest {
            pass_index,
            phase: super::super::execution::phase_label(pass_index),
            approved: ApprovedContract {
                approved: ApprovedProposal {
                    proposal: ReconciledProposal {
                        title: "Proposal".to_string(),
                        executive_summary: "summary".to_string(),
                        recommended_direction: "direction".to_string(),
                        why_this_plan: "why".to_string(),
                        adopted_ideas: Vec::new(),
                        deferred_ideas: Vec::new(),
                        scope: "scope".to_string(),
                        architecture: Vec::new(),
                        delivery_plan: Vec::new(),
                        technologies: Vec::new(),
                        major_risks: Vec::new(),
                        open_questions: Vec::new(),
                    },
                    approval: ApprovalOutcome {
                        decision: "approve".to_string(),
                        summary: "approved".to_string(),
                        final_details: Vec::new(),
                        next_step: "implement".to_string(),
                    },
                },
                contract: ProjectContract {
                    problem_statement: "Build the thing".to_string(),
                    user_goals: vec!["Ship the thing".to_string()],
                    non_goals: vec!["Rewrite everything".to_string()],
                    assumptions: vec!["Single repository".to_string()],
                    constraints: vec!["Use Rust".to_string()],
                    acceptance_criteria: vec!["Tests pass".to_string()],
                    definition_of_done: vec!["Workflow completes".to_string()],
                    approved_tech_choices: vec!["Rust".to_string()],
                    explicit_exclusions: vec!["Mobile app".to_string()],
                    demo_scenarios: vec!["Run the workflow".to_string()],
                },
                contract_approval: ApprovalOutcome {
                    decision: "approve".to_string(),
                    summary: "contract approved".to_string(),
                    final_details: Vec::new(),
                    next_step: "plan".to_string(),
                },
            },
            plan: ExecutionPlan {
                summary: "plan".to_string(),
                milestones: vec![ExecutionMilestone {
                    id: "m1".to_string(),
                    title: "Milestone".to_string(),
                    objective: "Ship it".to_string(),
                    task_card_ids: vec!["item-0".to_string()],
                }],
                task_cards: vec![TaskCard {
                    id: "item-0".to_string(),
                    source: "plan".to_string(),
                    milestone_id: Some("m1".to_string()),
                    title: "Task 0".to_string(),
                    objective: "Ship it".to_string(),
                    contract_refs: vec!["AC-1".to_string()],
                    acceptance_criteria: vec!["done".to_string()],
                    expected_files: vec!["src/lib.rs".to_string()],
                    verification_commands: vec!["cargo test".to_string()],
                    dependencies: Vec::new(),
                    rollback_notes: vec!["revert task".to_string()],
                }],
                risks: Vec::new(),
            },
            architect_review: StageReview {
                summary: "looks fine".to_string(),
                findings: Vec::new(),
            },
            completed_items: Vec::new(),
            remediation_items: Vec::new(),
        }
    }

    fn sample_phase(pass_index: usize, item_count: usize) -> ManagedPhase {
        ManagedPhase {
            request: sample_management_request(pass_index),
            worklist: crate::models::ImplementationWorklist {
                summary: "worklist".to_string(),
                items: (0..item_count)
                    .map(|index| TaskCard {
                        id: format!("item-{index}"),
                        source: "plan".to_string(),
                        milestone_id: Some("m1".to_string()),
                        title: format!("Item {index}"),
                        objective: "Do the thing".to_string(),
                        contract_refs: vec![format!("AC-{index}")],
                        acceptance_criteria: vec!["done".to_string()],
                        expected_files: vec!["src/workflow.rs".to_string()],
                        verification_commands: vec!["cargo test".to_string()],
                        dependencies: Vec::new(),
                        rollback_notes: vec!["undo item".to_string()],
                    })
                    .collect(),
            },
        }
    }

    fn dummy_execution_steps() -> ExecutionGraphSteps {
        ExecutionGraphSteps {
            managed_phase: dummy_management_step(),
            implementation: dummy_implementation_step(),
            phase_review: dummy_phase_review_step(),
            outcome: build_outcome_step(),
        }
    }

    fn dummy_management_step()
    -> Step<AppRuntime, ImplementationManagementRequest, ManagedPhase, NeverFinding, AppError> {
        Step::builder(task_fn(
            |_runtime: &AppRuntime, request: ImplementationManagementRequest| {
                Box::pin(async move {
                    Ok::<_, AppError>(ManagedPhase {
                        worklist: crate::models::ImplementationWorklist {
                            summary: "dummy".to_string(),
                            items: Vec::new(),
                        },
                        request,
                    })
                })
            },
        ))
        .with_findings::<NeverFinding>()
        .build()
    }

    fn dummy_implementation_step() -> Step<
        AppRuntime,
        super::super::ImplementationExecutionInput,
        ImplementationDraft,
        crate::models::StageFinding,
        AppError,
    > {
        Step::builder(task_fn(
            |_runtime: &AppRuntime, input: super::super::ImplementationExecutionInput| {
                Box::pin(async move {
                    Ok::<_, AppError>(ImplementationDraft {
                        input: input.task,
                        worktree_name: input.worktree_name,
                        delta: ImplementationDelta {
                            summary: "dummy".to_string(),
                            rationale: Vec::new(),
                            changes: Vec::new(),
                        },
                    })
                })
            },
        ))
        .with_findings::<crate::models::StageFinding>()
        .build()
    }

    fn dummy_phase_review_step() -> Step<
        AppRuntime,
        super::super::PhaseExecutionInput,
        PhaseReviewResult,
        NeverFinding,
        AppError,
    > {
        use crate::models::FinalReview;

        Step::builder(task_fn(
            |_runtime: &AppRuntime, input: super::super::PhaseExecutionInput| {
                Box::pin(async move {
                    Ok::<_, AppError>(PhaseReviewResult {
                        phase: input.phase,
                        completed_items: Vec::new(),
                        review: FinalReview {
                            summary: "dummy".to_string(),
                            ready: true,
                            strengths: Vec::new(),
                            findings: Vec::new(),
                            remediation_items: Vec::new(),
                            next_step: "done".to_string(),
                        },
                    })
                })
            },
        ))
        .with_findings::<NeverFinding>()
        .build()
    }

    #[test]
    fn build_phase_patch_spawns_item_nodes_and_one_review_node() {
        let phase = sample_phase(0, 2);
        let patch = build_phase_patch(NodeId::new(), &phase, dummy_execution_steps());

        assert_eq!(patch.nodes().len(), 3);
        assert_eq!(patch.edges().len(), 5);
        assert_eq!(
            patch
                .nodes()
                .iter()
                .filter(|node| node.name().starts_with("implement_item_"))
                .count(),
            2
        );
        assert!(
            patch
                .nodes()
                .iter()
                .any(|node| node.name().starts_with("phase_review_"))
        );
    }

    #[test]
    fn build_review_patch_spawns_next_management_phase_when_remediation_is_needed() {
        let result = PhaseReviewResult {
            phase: sample_phase(0, 1),
            completed_items: Vec::new(),
            review: crate::models::FinalReview {
                summary: "needs follow-up".to_string(),
                ready: false,
                strengths: Vec::new(),
                findings: Vec::new(),
                remediation_items: vec![RemediationItem {
                    id: "r1".to_string(),
                    title: "Fix issue".to_string(),
                    description: "patch it".to_string(),
                    acceptance_criteria: vec!["fixed".to_string()],
                    related_item_ids: vec!["item-0".to_string()],
                }],
                next_step: "repair".to_string(),
            },
        };

        let patch = build_review_patch(NodeId::new(), &result, dummy_execution_steps());

        assert_eq!(patch.nodes().len(), 1);
        assert_eq!(patch.edges().len(), 1);
        assert_eq!(
            patch.nodes()[0].name(),
            "implementation_management_remediation"
        );
    }

    #[test]
    fn build_outcome_patch_spawns_terminal_outcome_node() {
        let patch = build_outcome_patch(NodeId::new(), build_outcome_step());

        assert_eq!(patch.nodes().len(), 1);
        assert_eq!(patch.edges().len(), 1);
        assert_eq!(patch.nodes()[0].name(), "workflow_outcome");
    }
}
