use std::env;

use naaf_core::{NeverFinding, Step};
use serde::{Deserialize, Serialize};

use crate::{
    artifacts::RunArtifact,
    error::AppError,
    models::{
        ApprovalRequest, ApprovedContract, ApprovedProposal, ContractApprovalRequest,
        ContractDraftInput, FinalReview, ImplementationDraft, ImplementationItemResult,
        ImplementationManagementRequest, ImplementationTaskInput, ImplementationWorklist,
        ProjectPrompt, StageFinding, ValidationFinding, WorkflowOutcome,
    },
    runtime::{AppRuntime, WebSearchConfig},
};

mod execution;
mod graph;
mod logging;
mod steps;
mod tasks;

type LlmStageError =
    naaf_llm::AdapterError<AppError, naaf_llm::OpenAiError, AppError, serde_json::Error>;

const DEFAULT_MODEL: &str = "qwen/qwen3.6-35b-a3b";
const IMPLEMENTATION_RETRY_LIMIT: usize = 3;
const MAX_DISCOVERY_CLARIFICATION_PASSES: usize = 2;
const MAX_FINAL_REVIEW_PASSES: usize = 3;
const WORKFLOW_MAX_CONCURRENCY: usize = 4;
const WORKTREE_DIR: &str = ".mmat-worktrees";

#[derive(Clone)]
pub(crate) struct ExecutionGraphSteps {
    managed_phase:
        Step<AppRuntime, ImplementationManagementRequest, ManagedPhase, NeverFinding, AppError>,
    implementation:
        Step<AppRuntime, ImplementationExecutionInput, ImplementationDraft, StageFinding, AppError>,
    phase_review: Step<AppRuntime, PhaseExecutionInput, PhaseReviewResult, NeverFinding, AppError>,
    outcome: Step<AppRuntime, WorkflowOutcome, WorkflowOutcome, NeverFinding, AppError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PhaseExecutionInput {
    phase: ManagedPhase,
    drafts: Vec<ImplementationDraft>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PhaseReviewResult {
    phase: ManagedPhase,
    completed_items: Vec<ImplementationItemResult>,
    review: FinalReview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhaseReviewAction {
    Complete,
    Remediate,
    Halt,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ManagedPhase {
    request: ImplementationManagementRequest,
    worklist: ImplementationWorklist,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationExecutionInput {
    task: ImplementationTaskInput,
    worktree_name: String,
}

impl From<ValidationFinding> for StageFinding {
    fn from(value: ValidationFinding) -> Self {
        Self {
            severity: value.severity,
            category: value.category,
            message: value.message,
        }
    }
}

pub(crate) async fn run_mmat(
    runtime: &AppRuntime,
    prompt: String,
) -> Result<WorkflowOutcome, AppError> {
    let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    let web_search = WebSearchConfig::from_env();
    let search_enabled = web_search.is_some();

    let base_url =
        env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:1234/v1".to_string());

    runtime.log_info(format!("Using model `{model}` via `{base_url}`."))?;
    if search_enabled {
        runtime.log_info("External web research is enabled for this run.")?;
    } else {
        runtime.log_warn(
            "External web research is disabled. Set MMAT_WEB_SEARCH_URL to enable the web_search tool.",
        )?;
    }

    let llm_interactive = steps::build_agent(runtime.project_root(), web_search.clone(), true)?;
    let llm_quiet = steps::build_agent(runtime.project_root(), web_search.clone(), false)?;
    let discovery_step = Step::builder(tasks::build_discovery_task(
        &llm_interactive,
        &model,
        search_enabled,
    ))
    .with_findings::<NeverFinding>()
    .build();
    let [conservative_branch, recommended_branch, ambitious_branch] =
        crate::models::SolutionBranch::default_set();
    let conservative =
        tasks::build_solution_branch(&llm_quiet, &model, conservative_branch, search_enabled);
    let recommended =
        tasks::build_solution_branch(&llm_quiet, &model, recommended_branch, search_enabled);
    let ambitious =
        tasks::build_solution_branch(&llm_quiet, &model, ambitious_branch, search_enabled);
    let solutions_workflow = conservative
        .join(recommended)
        .reconcile_task(tasks::collect_solution_pair_task("collect_default_pair"))
        .join(ambitious)
        .reconcile_task(tasks::push_solution_task("add_ambitious_solution"));
    let reconcile_step = Step::builder(tasks::build_reconcile_task(&llm_interactive, &model))
        .with_findings::<NeverFinding>()
        .build();
    let approval_step = Step::builder(tasks::build_approval_task(&llm_interactive, &model))
        .with_findings::<NeverFinding>()
        .build();
    let contract_step = Step::builder(tasks::build_contract_task(
        &llm_quiet,
        &model,
        search_enabled,
    ))
    .with_findings::<NeverFinding>()
    .build();
    let contract_approval_step = Step::builder(tasks::build_contract_approval_task(
        &llm_interactive,
        &model,
    ))
    .with_findings::<NeverFinding>()
    .build();

    let planning_step = Step::builder(tasks::build_planning_task(
        &llm_quiet,
        &model,
        search_enabled,
    ))
    .with_findings::<NeverFinding>()
    .build();
    let architect_review_step =
        Step::builder(tasks::build_architect_review_task(&llm_quiet, &model))
            .with_findings::<NeverFinding>()
            .build();
    let knowledge_step = Step::builder(tasks::build_knowledge_compilation_task(
        &llm_quiet,
        &model,
        search_enabled,
    ))
    .with_findings::<NeverFinding>()
    .build();
    let execution_steps = ExecutionGraphSteps {
        implementation: steps::build_implementation_step(&model, web_search.clone())?,
        managed_phase: steps::build_managed_phase_step(
            runtime.project_root(),
            &model,
            search_enabled,
            web_search.clone(),
        )?,
        phase_review: steps::build_phase_review_step(
            runtime.project_root(),
            &model,
            search_enabled,
            web_search.clone(),
        )?,
        outcome: graph::build_outcome_step(),
    };

    let mut prompt_context = prompt;
    let mut clarification_attempt = 0usize;

    loop {
        execution::write_run_summary(runtime, &prompt_context, "running", "discovery", None)?;
        let discovery = discovery_step
            .run(
                runtime,
                ProjectPrompt {
                    raw: prompt_context.clone(),
                    clarification_attempt,
                    clarification_limit: MAX_DISCOVERY_CLARIFICATION_PASSES,
                },
            )
            .await
            .map_err(|error| AppError::Workflow(format!("discovery stage failed: {error}")))?;
        runtime.persist_artifact(RunArtifact::IntentBrief, &discovery)?;
        logging::log_discovery_summary(runtime, &discovery)?;

        if !execution::discovery_ready_for_solution(&discovery)
            && clarification_attempt < MAX_DISCOVERY_CLARIFICATION_PASSES
        {
            execution::write_run_summary(
                runtime,
                &prompt_context,
                "awaiting_clarification",
                "discovery",
                Some("user clarification"),
            )?;
            let clarification =
                execution::prompt_for_discovery_clarification(runtime, &discovery).await?;
            prompt_context = execution::append_user_guidance(
                &prompt_context,
                "User clarification after discovery",
                &clarification,
            );
            clarification_attempt += 1;
            continue;
        }

        if !execution::discovery_ready_for_solution(&discovery) {
            runtime.log_warn(
                "Clarification budget exhausted. Proceeding with the recorded best-guess intent brief and defaults.",
            )?;
        }

        execution::write_run_summary(
            runtime,
            &prompt_context,
            "running",
            "knowledge_compilation",
            None,
        )?;
        let repository_knowledge = knowledge_step
            .run(runtime, discovery.clone())
            .await
            .map_err(|error| {
                AppError::Workflow(format!("knowledge compilation failed: {error}"))
            })?;
        runtime.persist_artifact(RunArtifact::KnowledgeArtifact, &repository_knowledge)?;
        logging::log_knowledge_summary(runtime, &repository_knowledge)?;

        execution::write_run_summary(
            runtime,
            &prompt_context,
            "running",
            "solution_generation",
            None,
        )?;
        let solutions = solutions_workflow
            .run(runtime, discovery.clone())
            .await
            .map_err(|error| AppError::Workflow(format!("solution generation failed: {error}")))?;
        logging::log_solution_summaries(runtime, &solutions)?;

        let reconciled = reconcile_step
            .run(runtime, solutions)
            .await
            .map_err(|error| AppError::Workflow(format!("reconcile stage failed: {error}")))?;
        runtime.persist_artifact(RunArtifact::ReconciledProposal, &reconciled)?;
        logging::log_reconciled_summary(runtime, &reconciled)?;

        runtime.log_info(
            "The next prompt will collect approval, revision notes, or any final constraints.",
        )?;

        execution::write_run_summary(
            runtime,
            &prompt_context,
            "awaiting_approval",
            "approval",
            None,
        )?;
        let approval_response = execution::prompt_for_approval(runtime, &reconciled).await?;
        let approval = approval_step
            .run(
                runtime,
                ApprovalRequest {
                    proposal: reconciled.clone(),
                    user_response: approval_response.clone(),
                },
            )
            .await
            .map_err(|error| AppError::Workflow(format!("approval stage failed: {error}")))?;
        runtime.persist_artifact(RunArtifact::ApprovalOutcome, &approval)?;
        logging::log_approval_summary(runtime, &approval)?;

        if !execution::approval_granted(&approval) {
            runtime.log_info(
                "Revision requested. Returning to discovery with the user's latest guidance.",
            )?;
            prompt_context = execution::append_user_guidance(
                &prompt_context,
                "User revision after proposal review",
                &approval_response,
            );
            execution::write_run_summary(runtime, &prompt_context, "revising", "discovery", None)?;
            continue;
        }

        let approved = ApprovedProposal {
            proposal: reconciled,
            approval: approval.clone(),
        };
        runtime.log_info("Proposal approved. Forming the project contract.")?;

        execution::write_run_summary(runtime, &prompt_context, "running", "contract", None)?;
        let contract = contract_step
            .run(
                runtime,
                ContractDraftInput {
                    intent: discovery.clone(),
                    approved: approved.clone(),
                },
            )
            .await
            .map_err(|error| AppError::Workflow(format!("contract stage failed: {error}")))?;
        runtime.persist_artifact(RunArtifact::ProjectContract, &contract)?;
        logging::log_contract_summary(runtime, &contract)?;

        execution::write_run_summary(
            runtime,
            &prompt_context,
            "awaiting_contract_approval",
            "contract",
            None,
        )?;
        let contract_response = execution::prompt_for_contract_approval(runtime, &contract).await?;
        let contract_approval = contract_approval_step
            .run(
                runtime,
                ContractApprovalRequest {
                    contract: contract.clone(),
                    user_response: contract_response.clone(),
                },
            )
            .await
            .map_err(|error| AppError::Workflow(format!("contract approval failed: {error}")))?;
        runtime.persist_artifact(RunArtifact::ContractApprovalOutcome, &contract_approval)?;
        logging::log_contract_approval_summary(runtime, &contract_approval)?;

        if !execution::approval_granted(&contract_approval) {
            runtime.log_info(
                "Contract revisions requested. Returning to discovery with the user's latest guidance.",
            )?;
            prompt_context = execution::append_user_guidance(
                &prompt_context,
                "User revision after contract review",
                &contract_response,
            );
            execution::write_run_summary(runtime, &prompt_context, "revising", "discovery", None)?;
            continue;
        }

        let approved_contract = ApprovedContract {
            approved,
            contract: contract.clone(),
            contract_approval: contract_approval.clone(),
        };
        runtime.log_info("Contract approved. Starting planning and execution.")?;

        execution::write_run_summary(runtime, &prompt_context, "running", "planning", None)?;
        let plan = planning_step
            .run(runtime, approved_contract.clone())
            .await
            .map_err(|error| AppError::Workflow(format!("planning stage failed: {error}")))?;
        runtime.persist_artifact(RunArtifact::ExecutionPlan, &plan)?;
        execution::persist_task_cards(runtime, &plan.task_cards)?;
        logging::log_planning_summary(runtime, &plan)?;

        execution::write_run_summary(
            runtime,
            &prompt_context,
            "running",
            "architect_review",
            None,
        )?;
        let architect_review = architect_review_step
            .run(runtime, plan.clone())
            .await
            .map_err(|error| AppError::Workflow(format!("architect review failed: {error}")))?;
        runtime.persist_artifact(RunArtifact::ArchitectReview, &architect_review)?;
        logging::log_stage_review(runtime, "Architect review", &architect_review)?;

        execution::write_run_summary(runtime, &prompt_context, "running", "implementation", None)?;
        let outcome = execution::run_dynamic_implementation_workflow(
            runtime,
            approved_contract,
            plan,
            architect_review,
            execution_steps.clone(),
        )
        .await?;

        if let Some(final_review) = &outcome.final_review {
            runtime.persist_artifact(RunArtifact::FinalReview, final_review)?;
        }

        let release_assessment =
            execution::run_release_assessment(runtime, &model, &outcome, web_search.clone())
                .await?;
        runtime.persist_artifact(RunArtifact::ReleaseAssessment, &release_assessment)?;
        logging::log_release_assessment_summary(runtime, &release_assessment)?;

        let mut final_outcome = outcome;
        final_outcome.release_assessment = Some(release_assessment);
        runtime.persist_artifact(RunArtifact::WorkflowOutcome, &final_outcome)?;
        execution::write_run_summary(
            runtime,
            &prompt_context,
            &final_outcome.status,
            "completed",
            Some(&final_outcome.next_step),
        )?;

        return Ok(final_outcome);
    }
}
