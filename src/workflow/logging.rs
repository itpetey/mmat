use super::{AppError, AppRuntime};
use crate::models::{
    ApprovalOutcome, ExecutionPlan, FinalReview, ImplementationItemResult, ImplementationWorklist,
    KnowledgeArtifact, ProjectContract, ReconciledProposal, ReleaseAssessment, StageReview,
    ValidatedSolution,
};

pub fn log_approval_summary(
    runtime: &AppRuntime,
    approval: &ApprovalOutcome,
) -> Result<(), AppError> {
    runtime.log_info(format!("Approval outcome: {}", approval.decision))?;
    runtime.log_info(approval.summary.clone())?;
    if !approval.final_details.is_empty() {
        runtime.log_info(format!(
            "Captured details: {}",
            approval.final_details.join(" | ")
        ))?;
    }
    Ok(())
}

pub fn log_contract_approval_summary(
    runtime: &AppRuntime,
    approval: &ApprovalOutcome,
) -> Result<(), AppError> {
    runtime.log_info(format!("Contract approval outcome: {}", approval.decision))?;
    runtime.log_info(approval.summary.clone())?;
    if !approval.final_details.is_empty() {
        runtime.log_info(format!(
            "Captured contract details: {}",
            approval.final_details.join(" | ")
        ))?;
    }
    Ok(())
}

pub fn log_contract_summary(
    runtime: &AppRuntime,
    contract: &ProjectContract,
) -> Result<(), AppError> {
    runtime.log_info("Project contract drafted.")?;
    runtime.log_info(format!(
        "Contract problem statement: {}",
        contract.problem_statement
    ))?;
    runtime.log_info(format!(
        "Contract acceptance criteria: {}",
        contract.acceptance_criteria.len()
    ))?;
    Ok(())
}

pub fn log_knowledge_summary(
    runtime: &AppRuntime,
    knowledge: &KnowledgeArtifact,
) -> Result<(), AppError> {
    runtime.log_info(format!(
        "Knowledge compilation complete: {} entries in '{}' channel.",
        knowledge.entries.len(),
        knowledge.channel
    ))?;
    Ok(())
}

pub fn log_discovery_summary(
    runtime: &AppRuntime,
    discovery: &crate::models::IntentBrief,
) -> Result<(), AppError> {
    runtime.log_info("Intent capture complete.")?;
    runtime.log_info(format!(
        "Ready for solution generation: {}",
        discovery.ready_for_solution
    ))?;
    runtime.log_info(format!("Recommended path: {}", discovery.recommended_path))?;
    if !discovery.default_assumptions.is_empty() {
        runtime.log_info(format!(
            "Default assumptions: {}",
            discovery.default_assumptions.join(" | ")
        ))?;
    }
    if !discovery.constraints.is_empty() {
        runtime.log_info(format!(
            "Constraints: {}",
            discovery.constraints.join(" | ")
        ))?;
    }
    if !discovery.clarification_questions.is_empty() {
        runtime.log_info(format!(
            "Clarification questions: {}",
            discovery.clarification_questions.join(" | ")
        ))?;
    }
    Ok(())
}

pub fn log_final_review_summary(
    runtime: &AppRuntime,
    review: &FinalReview,
) -> Result<(), AppError> {
    runtime.log_info(format!("Final review: {}", review.summary))?;
    if review.ready {
        runtime.log_info("Final review accepted the implementation.")?;
    } else {
        runtime.log_warn(format!(
            "Final review requested more work on {} item(s).",
            review.remediation_items.len()
        ))?;
    }
    Ok(())
}

pub fn log_implementation_result(
    runtime: &AppRuntime,
    result: &ImplementationItemResult,
) -> Result<(), AppError> {
    runtime.log_info(format!("Completed `{}`: {}", result.title, result.summary))?;
    if !result.changed_files.is_empty() {
        runtime.log_info(format!(
            "Changed files: {}",
            result.changed_files.join(" | ")
        ))?;
    }
    if !result.commands_run.is_empty() {
        runtime.log_info(format!(
            "Evidence: {}",
            result
                .commands_run
                .iter()
                .map(|command| format!("{}={}", command.command, command.outcome))
                .collect::<Vec<_>>()
                .join(" | ")
        ))?;
    }
    Ok(())
}

pub fn log_planning_summary(runtime: &AppRuntime, plan: &ExecutionPlan) -> Result<(), AppError> {
    runtime.log_info("Planning complete.")?;
    runtime.log_info(format!("Plan summary: {}", plan.summary))?;
    runtime.log_info(format!("Milestones: {}", plan.milestones.len()))?;
    runtime.log_info(format!("Task cards: {}", plan.task_cards.len()))?;
    Ok(())
}

pub fn log_reconciled_summary(
    runtime: &AppRuntime,
    proposal: &ReconciledProposal,
) -> Result<(), AppError> {
    runtime.log_info("Reconcile complete.")?;
    runtime.log_info(format!("Proposal: {}", proposal.title))?;
    runtime.log_info(proposal.executive_summary.clone())?;
    if !proposal.open_questions.is_empty() {
        runtime.log_info(format!(
            "Open questions: {}",
            proposal.open_questions.join(" | ")
        ))?;
    }
    Ok(())
}

pub fn log_solution_summaries(
    runtime: &AppRuntime,
    solutions: &[ValidatedSolution],
) -> Result<(), AppError> {
    runtime.log_info("Solution exploration complete.")?;
    for solution in solutions {
        runtime.log_info(format!(
            "{}: {} [{}; feasibility={}, risk={}]",
            solution.branch,
            solution.proposal.title,
            solution.recommendation,
            solution.feasibility,
            solution.delivery_risk,
        ))?;
    }
    Ok(())
}

pub fn log_stage_review(
    runtime: &AppRuntime,
    stage_name: &str,
    review: &StageReview,
) -> Result<(), AppError> {
    runtime.log_info(format!("{stage_name}: {}", review.summary))?;
    if review.findings.is_empty() {
        runtime.log_info(format!("{stage_name} found no blocking issues."))?;
    } else {
        runtime.log_warn(format!(
            "{stage_name} findings: {}",
            review
                .findings
                .iter()
                .map(|finding| format!(
                    "{}:{}:{}",
                    finding.severity, finding.category, finding.message
                ))
                .collect::<Vec<_>>()
                .join(" | ")
        ))?;
    }
    Ok(())
}

pub fn log_worklist_summary(
    runtime: &AppRuntime,
    worklist: &ImplementationWorklist,
) -> Result<(), AppError> {
    runtime.log_info(format!("Implementation management: {}", worklist.summary))?;
    runtime.log_info(format!(
        "Task cards in this phase: {}",
        worklist.items.len()
    ))?;
    Ok(())
}

pub fn log_release_assessment_summary(
    runtime: &AppRuntime,
    assessment: &ReleaseAssessment,
) -> Result<(), AppError> {
    runtime.log_info("Release assessment complete.")?;
    runtime.log_info(format!("Releasable: {}", assessment.releasable))?;
    runtime.log_info(format!("Summary: {}", assessment.summary))?;
    if !assessment.contract_items_incomplete.is_empty() {
        runtime.log_warn(format!(
            "Incomplete contract items: {}",
            assessment.contract_items_incomplete.join(" | ")
        ))?;
    }
    if !assessment.claimed_but_not_proven.is_empty() {
        runtime.log_warn(format!(
            "Claimed but not proven: {}",
            assessment.claimed_but_not_proven.join(" | ")
        ))?;
    }
    if !assessment.residual_risks.is_empty() {
        runtime.log_warn(format!(
            "Residual risks: {}",
            assessment.residual_risks.join(" | ")
        ))?;
    }
    Ok(())
}
