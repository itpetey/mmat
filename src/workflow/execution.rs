use std::{env, path::Path};

use naaf_core::{GraphPatch, RunnerRegistry, Workflow};
use naaf_llm::{
    CompletionRequest, Executor, ExecutorConfig, HumanIO, HumanQuestion, LlmAgent, Message,
    OpenAiClient, OpenAiConfig, QuestionTool, RegisterToolError, Tool, ToolRegistry,
};
use naaf_persistence_fs::FsCheckpointer;
use naaf_workspace::{
    FileDelta as NaafFileDelta, FileDeltaSet as NaafFileDeltaSet,
    apply_file_deltas as naaf_apply_file_deltas,
    build_workspace_delta as naaf_build_workspace_delta,
    command_failure_summary as naaf_command_failure_summary,
    merge_change_into_workspace as naaf_merge_change_into_workspace,
    remove_worktree as naaf_remove_worktree,
};
use serde::de::DeserializeOwned;
use tokio::process::Command;

use crate::{
    models::{
        ApprovedContract, CommandEvidence, EvidenceLog, ExecutionPlan, FinalReview,
        ImplementationDelta, ImplementationManagementRequest, ReleaseAssessment,
        ReleaseAssessmentInput, RunSummary, StageFinding, StageReview, TaskCard,
    },
    parsing::decode_json_output,
    workflow::{
        AppError, AppRuntime, ImplementationDraft, ImplementationItemResult,
        MAX_FINAL_REVIEW_PASSES, PhaseReviewAction, PhaseReviewResult, WORKFLOW_MAX_CONCURRENCY,
        WORKTREE_DIR, WorkflowOutcome,
    },
};

pub type AppAgent = LlmAgent<OpenAiClient<AppRuntime>, AppRuntime, AppError>;

const DEFAULT_API_KEY: &str = "lm-studio";
const DEFAULT_BASE_URL: &str = "http://127.0.0.1:1234/v1";
pub const EXECUTOR_TURNS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalReviewDisposition {
    Complete,
    Remediate,
    Halt,
}

pub fn append_user_guidance(prompt: &str, heading: &str, response: &str) -> String {
    let response = response.trim();
    if response.is_empty() {
        return prompt.to_string();
    }

    format!("{prompt}\n\n{heading}:\n{response}")
}

pub fn apply_file_deltas(root: &Path, delta: &ImplementationDelta) -> Result<(), AppError> {
    let naaf_delta = NaafFileDeltaSet {
        summary: delta.summary.clone(),
        rationale: delta.rationale.clone(),
        changes: delta
            .changes
            .iter()
            .map(|c| NaafFileDelta {
                path: c.path.clone(),
                action: c.action.clone(),
                content: c.content.clone(),
            })
            .collect(),
    };
    naaf_apply_file_deltas(root, &naaf_delta)
        .map_err(|error| AppError::Workspace(error.to_string()))
}

pub fn sanitise_worktree_name(name: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    let hash = hasher.finish();

    let mut out = String::with_capacity(name.len() + 9);
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out = "unknown".to_string();
    }
    out.push_str(&format!("-{hash:x}"));
    out
}

pub fn approval_granted(approval: &crate::models::ApprovalOutcome) -> bool {
    let decision = approval.decision.trim().to_ascii_lowercase();
    matches!(
        decision.as_str(),
        "approve" | "approved" | "accept" | "accepted"
    )
}

pub fn build_agent(
    project_root: &Path,
    web_search: Option<super::WebSearchConfig>,
    allow_question: bool,
) -> Result<AppAgent, AppError> {
    use crate::runtime::{AppGlobPathsTool, AppReadFileTool, AppSearchFilesTool, AppWebSearchTool};

    let mut tools: ToolRegistry<AppRuntime, AppError> = ToolRegistry::new();
    if allow_question {
        tools = register_tool(tools, QuestionTool::<AppRuntime>::new())?;
    }
    tools = register_tool(tools, AppReadFileTool::new(project_root.to_path_buf()))?;
    tools = register_tool(tools, AppGlobPathsTool::new(project_root.to_path_buf()))?;
    tools = register_tool(tools, AppSearchFilesTool::new(project_root.to_path_buf()))?;
    if let Some(config) = web_search.as_ref() {
        tools = register_tool(tools, AppWebSearchTool::new(config))?;
    }

    let api_key = env::var("OPENAI_API_KEY").unwrap_or_else(|_| DEFAULT_API_KEY.to_string());
    let base_url = env::var("OPENAI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    let mut config = OpenAiConfig::new(api_key).with_base_url(base_url);
    if let Ok(org) = env::var("OPENAI_ORG_ID") {
        config = config.with_organisation(org);
    }

    let client = OpenAiClient::new(config);
    let executor =
        Executor::with_tools(client, tools).with_config(ExecutorConfig::new(EXECUTOR_TURNS));
    Ok(LlmAgent::with_executor(executor))
}

pub fn discovery_ready_for_solution(discovery: &crate::models::IntentBrief) -> bool {
    discovery.ready_for_solution
}

pub async fn execute_json_stage<T>(
    llm: &AppAgent,
    runtime: &AppRuntime,
    request: CompletionRequest,
    stage: &str,
) -> Result<T, AppError>
where
    T: DeserializeOwned,
{
    let outcome = llm
        .executor()
        .execute(runtime, request)
        .await
        .map_err(|error| AppError::Workflow(format!("{stage} execution failed: {error}")))?;
    decode_json_output(outcome).map_err(AppError::from)
}

pub fn final_review_disposition(review: &FinalReview) -> FinalReviewDisposition {
    if review.ready {
        FinalReviewDisposition::Complete
    } else if review.remediation_items.is_empty() {
        FinalReviewDisposition::Halt
    } else {
        FinalReviewDisposition::Remediate
    }
}

pub fn implementation_node_name(pass_index: usize, item_id: &str) -> String {
    format!("implement_item_{pass_index}_{item_id}")
}

pub fn initial_management_request(
    approved: ApprovedContract,
    plan: ExecutionPlan,
    architect_review: StageReview,
) -> ImplementationManagementRequest {
    ImplementationManagementRequest {
        pass_index: 0,
        phase: phase_label(0),
        approved,
        plan,
        architect_review,
        completed_items: Vec::new(),
        remediation_items: Vec::new(),
    }
}

pub fn management_node_name(pass_index: usize) -> String {
    format!("implementation_management_{pass_index}")
}

pub async fn merge_item_worktree(
    project_root: &Path,
    baseline_root: &Path,
    draft: &ImplementationDraft,
) -> Result<(), AppError> {
    let item_root = mmat_worktree_path(project_root, &draft.worktree_name);
    let changes = naaf_build_workspace_delta(&item_root, baseline_root)
        .map_err(|error| AppError::Workspace(error.to_string()))?;

    for change in &changes {
        let naaf_change = NaafFileDelta {
            path: change.path.clone(),
            action: change.action.clone(),
            content: change.content.clone(),
        };
        naaf_merge_change_into_workspace(project_root, baseline_root, &item_root, &naaf_change)
            .await
            .map_err(|error| AppError::Workspace(error.to_string()))?;
    }

    naaf_remove_worktree(project_root, &draft.worktree_name)
        .await
        .map_err(|error| AppError::Workspace(error.to_string()))?;

    Ok(())
}

pub fn build_item_results(
    drafts: &[ImplementationDraft],
    project_root: &Path,
    baseline_root: &Path,
    commands_run: Vec<CommandEvidence>,
) -> Result<Vec<ImplementationItemResult>, AppError> {
    let final_changes = naaf_build_workspace_delta(project_root, baseline_root)
        .map_err(|error| AppError::Workspace(error.to_string()))?;

    let mut results = Vec::new();
    for draft in drafts {
        let item_changed: Vec<String> = final_changes
            .iter()
            .filter(|c| draft.delta.changes.iter().any(|dc| dc.path == c.path))
            .map(|c| c.path.clone())
            .collect();

        results.push(ImplementationItemResult {
            item_id: draft.input.work_item.id.clone(),
            source: draft.input.work_item.source.clone(),
            milestone_id: draft.input.work_item.milestone_id.clone(),
            title: draft.input.work_item.title.clone(),
            objective: draft.input.work_item.objective.clone(),
            summary: draft.delta.summary.clone(),
            contract_refs: draft.input.work_item.contract_refs.clone(),
            changed_files: item_changed,
            rationale: draft.delta.rationale.clone(),
            commands_run: commands_run.clone(),
            reviewer_findings: Vec::new(),
            manual_checks: draft.input.work_item.acceptance_criteria.clone(),
            known_gaps: Vec::new(),
            scope_deviation: None,
            worktree_name: draft.worktree_name.clone(),
        });
    }
    Ok(results)
}

pub async fn run_phase_verification_commands(
    project_root: &Path,
    task_cards: &[TaskCard],
) -> Vec<CommandEvidence> {
    let mut commands = std::collections::BTreeSet::new();
    commands.insert("cargo fmt --all".to_string());
    commands.insert("cargo check".to_string());
    commands.insert("cargo test".to_string());
    commands.insert("cargo clippy -- -D warnings".to_string());
    commands.insert("peer review".to_string());
    for task_card in task_cards {
        for cmd in &task_card.verification_commands {
            if is_allowed_verification_command(cmd) {
                commands.insert(cmd.clone());
            }
        }
    }

    let mut evidence = Vec::new();
    for command in commands {
        let outcome = if command == "peer review" {
            "passed".to_string()
        } else if let Some(cargo_args) = command.strip_prefix("cargo ") {
            if run_command(
                project_root,
                &command,
                &cargo_args.split_whitespace().collect::<Vec<_>>(),
            )
            .await
            .is_ok()
            {
                "passed".to_string()
            } else {
                "failed".to_string()
            }
        } else {
            "skipped".to_string()
        };
        evidence.push(CommandEvidence { command, outcome });
    }
    evidence
}

fn is_allowed_verification_command(command: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "cargo fmt --all",
        "cargo fmt --check",
        "cargo check",
        "cargo check --all-features",
        "cargo check --no-default-features",
        "cargo test",
        "cargo test --all-features",
        "cargo test --no-default-features",
        "cargo clippy -- -D warnings",
        "cargo clippy --all-targets -- -D warnings",
        "cargo build",
        "cargo build --release",
        "cargo doc",
        "cargo doc --no-deps",
        "cargo bench",
    ];
    ALLOWED.contains(&command)
}

pub fn next_management_request(result: &PhaseReviewResult) -> ImplementationManagementRequest {
    let next_pass = result.phase.request.pass_index + 1;
    ImplementationManagementRequest {
        pass_index: next_pass,
        phase: phase_label(next_pass),
        approved: result.phase.request.approved.clone(),
        plan: result.phase.request.plan.clone(),
        architect_review: result.phase.request.architect_review.clone(),
        completed_items: result.completed_items.clone(),
        remediation_items: result.review.remediation_items.clone(),
    }
}

pub fn outcome_node_name() -> String {
    "workflow_outcome".to_string()
}

pub fn persist_task_cards(runtime: &AppRuntime, task_cards: &[TaskCard]) -> Result<(), AppError> {
    for task_card in task_cards {
        runtime.persist_task_card(task_card)?;
    }
    Ok(())
}

pub fn phase_label(pass_index: usize) -> String {
    if pass_index == 0 {
        "initial_implementation".to_string()
    } else {
        format!("remediation_pass_{pass_index}")
    }
}

pub fn phase_review_action(pass_index: usize, review: &FinalReview) -> PhaseReviewAction {
    match final_review_disposition(review) {
        FinalReviewDisposition::Complete => PhaseReviewAction::Complete,
        FinalReviewDisposition::Halt => PhaseReviewAction::Halt,
        FinalReviewDisposition::Remediate if pass_index + 1 < MAX_FINAL_REVIEW_PASSES => {
            PhaseReviewAction::Remediate
        }
        FinalReviewDisposition::Remediate => PhaseReviewAction::Halt,
    }
}

pub async fn prompt_for_approval(
    runtime: &AppRuntime,
    proposal: &crate::models::ReconciledProposal,
) -> Result<String, AppError> {
    let mut prompt = vec![
        "Please review the proposal before implementation starts.".to_string(),
        String::new(),
        format!("Title: {}", proposal.title),
    ];

    if !proposal.executive_summary.trim().is_empty() {
        prompt.push(format!("Summary: {}", proposal.executive_summary.trim()));
    }

    prompt.push(String::new());
    prompt.push(
        "Reply with `approve` to continue, or describe the revisions or constraints you want."
            .to_string(),
    );

    if !proposal.open_questions.is_empty() {
        prompt.push(String::new());
        prompt.push("Open questions still worth considering:".to_string());
        prompt.extend(
            proposal
                .open_questions
                .iter()
                .map(|question| format!("- {question}")),
        );
    }

    Ok(runtime
        .ask(HumanQuestion {
            question: prompt.join("\n"),
            choices: None,
        })
        .await?
        .content)
}

pub async fn prompt_for_contract_approval(
    runtime: &AppRuntime,
    contract: &crate::models::ProjectContract,
) -> Result<String, AppError> {
    let mut prompt = vec![
        "Please review the project contract before planning starts.".to_string(),
        String::new(),
        format!("Problem statement: {}", contract.problem_statement),
    ];

    if !contract.user_goals.is_empty() {
        prompt.push(format!("User goals: {}", contract.user_goals.join(" | ")));
    }

    if !contract.acceptance_criteria.is_empty() {
        prompt.push(format!(
            "Acceptance criteria: {}",
            contract.acceptance_criteria.join(" | ")
        ));
    }

    prompt.push(String::new());
    prompt.push(
        "Reply with `approve` to freeze the contract, or describe the revisions or constraints you want."
            .to_string(),
    );

    Ok(runtime
        .ask(HumanQuestion {
            question: prompt.join("\n"),
            choices: None,
        })
        .await?
        .content)
}

pub async fn prompt_for_discovery_clarification(
    runtime: &AppRuntime,
    discovery: &crate::models::IntentBrief,
) -> Result<String, AppError> {
    let mut prompt = vec![
        "Intent capture still has unresolved ambiguities before solution generation.".to_string(),
    ];

    if !discovery.problem_statement.trim().is_empty() {
        prompt.push(format!(
            "Current understanding: {}",
            discovery.problem_statement.trim()
        ));
    }

    if !discovery.default_assumptions.is_empty() {
        prompt.push(format!(
            "Current defaults if unanswered: {}",
            discovery.default_assumptions.join(" | ")
        ));
    }

    if !discovery.clarification_questions.is_empty() {
        prompt.push("Please answer these points in one reply:".to_string());
        for question in &discovery.clarification_questions {
            prompt.push(format!("- {question}"));
        }
    } else {
        prompt.push(
            "Please provide the missing problem statement, intended outcome, and any constraints in one reply."
                .to_string(),
        );
    }

    Ok(runtime
        .ask(HumanQuestion {
            question: prompt.join("\n"),
            choices: None,
        })
        .await?
        .content)
}

pub fn review_node_name(pass_index: usize) -> String {
    format!("phase_review_{pass_index}")
}

pub async fn run_command(root: &Path, label: &str, args: &[&str]) -> Result<(), AppError> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(root)
        .output()
        .await
        .map_err(|error| AppError::Workflow(format!("{label} failed to start: {error}")))?;

    if output.status.success() {
        return Ok(());
    }

    Err(AppError::Workflow(format!(
        "{label} failed: {}",
        naaf_command_failure_summary(&output.stdout, &output.stderr)
    )))
}

pub async fn run_dynamic_implementation_workflow(
    runtime: &AppRuntime,
    approved: ApprovedContract,
    plan: ExecutionPlan,
    architect_review: StageReview,
    execution_steps: super::ExecutionGraphSteps,
) -> Result<WorkflowOutcome, AppError> {
    let initial_request = initial_management_request(approved, plan, architect_review);
    let root = super::graph::root_management_node_spec(initial_request, execution_steps)?;

    let checkpointer = FsCheckpointer::new(runtime.run_root().join("naaf-checkpoints"));
    let registry: RunnerRegistry<AppRuntime, AppError> = RunnerRegistry::new();

    let report = Workflow::new()
        .with_max_concurrency(WORKFLOW_MAX_CONCURRENCY)
        .with_checkpointer(checkpointer)
        .with_registry(registry)
        .with_patch(GraphPatch::new().with_node(root))
        .map_err(|error| AppError::Workflow(format!("failed to build execution graph: {error}")))?
        .run(runtime)
        .await
        .map_err(|error| {
            AppError::Workflow(format!("dynamic execution workflow failed: {error}"))
        })?;

    let outcome = report
        .nodes()
        .values()
        .filter(|node| node.name().starts_with("workflow_outcome"))
        .max_by_key(|node| node.name())
        .ok_or_else(|| {
            AppError::Workflow("workflow completed without a terminal outcome node".to_string())
        })?;

    let outcome: WorkflowOutcome = serde_json::from_value(outcome.output().clone())?;
    Ok(outcome)
}

pub async fn run_release_assessment(
    runtime: &AppRuntime,
    model: &str,
    outcome: &WorkflowOutcome,
    web_search: Option<super::WebSearchConfig>,
) -> Result<ReleaseAssessment, AppError> {
    let Some(contract) = &outcome.contract else {
        return Err(AppError::Workflow(
            "cannot run release assessment without a project contract".to_string(),
        ));
    };

    let evidence_log = EvidenceLog {
        task_results: outcome.completed_items.clone(),
    };

    let llm = build_agent(runtime.project_root(), web_search, false)?;
    let request = CompletionRequest::new(
        model.to_string(),
        vec![
            Message::system(crate::prompts::release_assessment_system_prompt()),
            Message::user(crate::prompts::release_assessment_user_prompt(
                &ReleaseAssessmentInput {
                    contract: contract.clone(),
                    plan: outcome.plan.clone().unwrap_or_else(|| ExecutionPlan {
                        summary: String::new(),
                        milestones: Vec::new(),
                        task_cards: Vec::new(),
                        risks: Vec::new(),
                    }),
                    task_results: outcome.completed_items.clone(),
                    evidence_log,
                },
            )?),
        ],
    );

    let assessment =
        execute_json_stage::<ReleaseAssessment>(&llm, runtime, request, "release assessment")
            .await?;

    Ok(assessment)
}

pub async fn run_validator(
    root: &Path,
    label: &str,
    args: &[&str],
) -> Result<Vec<StageFinding>, AppError> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(root)
        .output()
        .await
        .map_err(|error| AppError::Workflow(format!("{label} failed to start: {error}")))?;

    if output.status.success() {
        return Ok(Vec::new());
    }

    Ok(vec![StageFinding {
        severity: "error".to_string(),
        category: label.to_string(),
        message: naaf_command_failure_summary(&output.stdout, &output.stderr),
    }])
}

pub fn workflow_outcome_from_phase_result(result: &PhaseReviewResult) -> WorkflowOutcome {
    WorkflowOutcome {
        status: if result.review.ready {
            "completed".to_string()
        } else {
            "needs_more_work".to_string()
        },
        approval: result.phase.request.approved.approved.approval.clone(),
        contract: Some(result.phase.request.approved.contract.clone()),
        contract_approval: Some(result.phase.request.approved.contract_approval.clone()),
        plan: Some(result.phase.request.plan.clone()),
        architect_review: Some(result.phase.request.architect_review.clone()),
        completed_items: result.completed_items.clone(),
        final_review: Some(result.review.clone()),
        release_assessment: None,
        next_step: result.review.next_step.clone(),
    }
}

pub fn write_run_summary(
    runtime: &AppRuntime,
    prompt: &str,
    status: &str,
    current_stage: &str,
    next_step: Option<&str>,
) -> Result<(), AppError> {
    runtime.persist_run_summary(&RunSummary {
        run_id: runtime.run_id().to_string(),
        project_root: runtime.project_root().display().to_string(),
        run_root: runtime.run_root().display().to_string(),
        prompt: prompt.to_string(),
        status: status.to_string(),
        current_stage: current_stage.to_string(),
        next_step: next_step.map(str::to_string),
    })
}

fn mmat_worktree_path(project_root: &Path, worktree_name: &str) -> std::path::PathBuf {
    project_root.join(WORKTREE_DIR).join(worktree_name)
}

fn register_tool<T>(
    tools: ToolRegistry<AppRuntime, AppError>,
    tool: T,
) -> Result<ToolRegistry<AppRuntime, AppError>, AppError>
where
    T: Tool<Runtime = AppRuntime, Error = AppError> + 'static,
{
    tools
        .with_tool(tool)
        .map_err(|error: RegisterToolError| AppError::Config(error.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::models::{
        ApprovalOutcome, CommandEvidence, FileDelta, ImplementationDelta, ImplementationDraft,
        ImplementationItemResult, ImplementationTaskInput, IntentBrief, RemediationItem, TaskCard,
    };

    use super::*;

    fn item_result_from_draft(
        item: &TaskCard,
        draft: &ImplementationDraft,
    ) -> ImplementationItemResult {
        ImplementationItemResult {
            item_id: item.id.clone(),
            source: item.source.clone(),
            milestone_id: item.milestone_id.clone(),
            title: item.title.clone(),
            objective: item.objective.clone(),
            summary: draft.delta.summary.clone(),
            contract_refs: item.contract_refs.clone(),
            changed_files: draft
                .delta
                .changes
                .iter()
                .map(|change| change.path.clone())
                .collect(),
            rationale: draft.delta.rationale.clone(),
            commands_run: item
                .verification_commands
                .iter()
                .map(|command| CommandEvidence {
                    command: command.clone(),
                    outcome: "passed".to_string(),
                })
                .collect(),
            reviewer_findings: Vec::new(),
            manual_checks: item.acceptance_criteria.clone(),
            known_gaps: Vec::new(),
            scope_deviation: None,
            worktree_name: draft.worktree_name.clone(),
        }
    }

    #[test]
    fn approval_granted_accepts_expected_decisions() {
        for decision in ["approve", "Approved", " accept ", "ACCEPTED"] {
            assert!(approval_granted(&ApprovalOutcome {
                decision: decision.to_string(),
                summary: "ok".to_string(),
                final_details: Vec::new(),
                next_step: "next".to_string(),
            }));
        }
    }

    #[test]
    fn approval_granted_rejects_non_approval_decisions() {
        for decision in ["revise", "reject", "needs changes"] {
            assert!(!approval_granted(&ApprovalOutcome {
                decision: decision.to_string(),
                summary: "not yet".to_string(),
                final_details: Vec::new(),
                next_step: "next".to_string(),
            }));
        }
    }

    #[test]
    fn discovery_ready_for_solution_uses_model_flag() {
        let ready = IntentBrief {
            ready_for_solution: true,
            problem_statement: "Build a task tracker".to_string(),
            user_goals: vec!["Track tasks".to_string()],
            non_goals: vec!["Collaboration".to_string()],
            assumptions: Vec::new(),
            default_assumptions: vec!["Single-user web app".to_string()],
            constraints: Vec::new(),
            ambiguities: Vec::new(),
            risks: Vec::new(),
            acceptance_criteria: vec!["Users can add tasks".to_string()],
            clarification_summary: Vec::new(),
            research_notes: Vec::new(),
            recommended_path: "Generate solutions".to_string(),
            clarification_questions: Vec::new(),
        };

        let waiting = IntentBrief {
            ready_for_solution: false,
            recommended_path: "Ask for clarification".to_string(),
            clarification_questions: vec!["What are we building?".to_string()],
            ..ready.clone()
        };

        assert!(discovery_ready_for_solution(&ready));
        assert!(!discovery_ready_for_solution(&waiting));
    }

    #[test]
    fn append_user_guidance_ignores_blank_responses() {
        assert_eq!(
            append_user_guidance("Prompt", "User clarification", "   "),
            "Prompt"
        );
    }

    #[test]
    fn append_user_guidance_adds_titled_follow_up() {
        assert_eq!(
            append_user_guidance("Prompt", "User clarification", "Add Python"),
            "Prompt\n\nUser clarification:\nAdd Python"
        );
    }

    #[test]
    fn final_review_disposition_prefers_completion() {
        let review = FinalReview {
            summary: "ready".to_string(),
            ready: true,
            strengths: vec!["done".to_string()],
            findings: Vec::new(),
            remediation_items: vec![RemediationItem {
                id: "r1".to_string(),
                title: "unused".to_string(),
                description: "ignored when ready".to_string(),
                acceptance_criteria: vec!["x".to_string()],
                related_item_ids: vec!["item-1".to_string()],
            }],
            next_step: "ship it".to_string(),
        };

        assert_eq!(
            final_review_disposition(&review),
            FinalReviewDisposition::Complete
        );
    }

    #[test]
    fn final_review_disposition_requests_remediation_when_needed() {
        let review = FinalReview {
            summary: "needs fixes".to_string(),
            ready: false,
            strengths: Vec::new(),
            findings: Vec::new(),
            remediation_items: vec![RemediationItem {
                id: "r1".to_string(),
                title: "fix issue".to_string(),
                description: "repair the work".to_string(),
                acceptance_criteria: vec!["passes".to_string()],
                related_item_ids: vec!["item-1".to_string()],
            }],
            next_step: "repair".to_string(),
        };

        assert_eq!(
            final_review_disposition(&review),
            FinalReviewDisposition::Remediate
        );
    }

    #[test]
    fn final_review_disposition_halts_without_remediation_items() {
        let review = FinalReview {
            summary: "blocked".to_string(),
            ready: false,
            strengths: Vec::new(),
            findings: Vec::new(),
            remediation_items: Vec::new(),
            next_step: "manual intervention".to_string(),
        };

        assert_eq!(
            final_review_disposition(&review),
            FinalReviewDisposition::Halt
        );
    }

    #[test]
    fn phase_review_action_halts_when_pass_limit_is_reached() {
        use super::super::MAX_FINAL_REVIEW_PASSES;

        let review = FinalReview {
            summary: "still needs fixes".to_string(),
            ready: false,
            strengths: Vec::new(),
            findings: Vec::new(),
            remediation_items: vec![RemediationItem {
                id: "r1".to_string(),
                title: "Fix it".to_string(),
                description: "one more issue".to_string(),
                acceptance_criteria: vec!["pass".to_string()],
                related_item_ids: vec!["item-1".to_string()],
            }],
            next_step: "repair".to_string(),
        };

        assert_eq!(
            phase_review_action(MAX_FINAL_REVIEW_PASSES - 1, &review),
            PhaseReviewAction::Halt
        );
    }

    #[test]
    fn next_management_request_increments_pass_and_carries_completed_items() {
        use super::super::{ManagedPhase, PhaseReviewResult};
        use crate::models::{
            ApprovalOutcome, ApprovedContract, ApprovedProposal, ExecutionMilestone, ExecutionPlan,
            ImplementationManagementRequest, ProjectContract, ReconciledProposal, StageReview,
        };

        fn sample_management_request(pass_index: usize) -> ImplementationManagementRequest {
            ImplementationManagementRequest {
                pass_index,
                phase: phase_label(pass_index),
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

        let result = PhaseReviewResult {
            phase: ManagedPhase {
                request: sample_management_request(0),
                worklist: crate::models::ImplementationWorklist {
                    summary: "worklist".to_string(),
                    items: Vec::new(),
                },
            },
            completed_items: vec![ImplementationItemResult {
                item_id: "item-1".to_string(),
                source: "plan".to_string(),
                milestone_id: Some("m1".to_string()),
                title: "done".to_string(),
                objective: "Ship it".to_string(),
                summary: "implemented".to_string(),
                contract_refs: vec!["AC-1".to_string()],
                changed_files: vec!["src/workflow.rs".to_string()],
                rationale: vec!["minimal".to_string()],
                commands_run: vec![CommandEvidence {
                    command: "cargo test".to_string(),
                    outcome: "passed".to_string(),
                }],
                reviewer_findings: Vec::new(),
                manual_checks: vec!["fixed".to_string()],
                known_gaps: Vec::new(),
                scope_deviation: None,
                worktree_name: "initial-implementation-item-1".to_string(),
            }],
            review: FinalReview {
                summary: "needs follow-up".to_string(),
                ready: false,
                strengths: Vec::new(),
                findings: Vec::new(),
                remediation_items: vec![RemediationItem {
                    id: "r1".to_string(),
                    title: "Fix issue".to_string(),
                    description: "patch it".to_string(),
                    acceptance_criteria: vec!["fixed".to_string()],
                    related_item_ids: vec!["item-1".to_string()],
                }],
                next_step: "repair".to_string(),
            },
        };

        let next = next_management_request(&result);

        assert_eq!(next.pass_index, 1);
        assert_eq!(next.phase, "remediation_pass_1");
        assert_eq!(next.completed_items.len(), 1);
        assert_eq!(next.remediation_items.len(), 1);
    }

    #[test]
    fn item_result_from_draft_carries_changed_files_and_rationale() {
        let item = TaskCard {
            id: "item-1".to_string(),
            source: "plan".to_string(),
            milestone_id: Some("m1".to_string()),
            title: "Add stage".to_string(),
            objective: "Implement the feature".to_string(),
            contract_refs: vec!["AC-1".to_string()],
            acceptance_criteria: vec!["works".to_string()],
            expected_files: vec!["src/workflow.rs".to_string()],
            verification_commands: vec!["cargo test".to_string()],
            dependencies: Vec::new(),
            rollback_notes: vec!["revert the stage".to_string()],
        };
        let draft = ImplementationDraft {
            input: serde_json::from_value::<ImplementationTaskInput>(serde_json::json!({
                "approved": {
                    "approved": {
                        "proposal": {
                            "title": "Plan",
                            "executive_summary": "summary",
                            "recommended_direction": "direction",
                            "why_this_plan": "why",
                            "adopted_ideas": [],
                            "deferred_ideas": [],
                            "scope": "scope",
                            "architecture": [],
                            "delivery_plan": [],
                            "technologies": [],
                            "major_risks": [],
                            "open_questions": []
                        },
                        "approval": {
                            "decision": "approve",
                            "summary": "ok",
                            "final_details": [],
                            "next_step": "build"
                        }
                    },
                    "contract": {
                        "problem_statement": "Implement the plan",
                        "user_goals": ["Add stage"],
                        "non_goals": ["Rewrite everything"],
                        "assumptions": ["Rust project"],
                        "constraints": ["Stay within the repo"],
                        "acceptance_criteria": ["works"],
                        "definition_of_done": ["tests pass"],
                        "approved_tech_choices": ["rust"],
                        "explicit_exclusions": ["new service"],
                        "demo_scenarios": ["run the workflow"]
                    },
                    "contract_approval": {
                        "decision": "approve",
                        "summary": "contract approved",
                        "final_details": [],
                        "next_step": "plan"
                    }
                },
                "plan": {
                    "summary": "plan",
                    "milestones": [],
                    "task_cards": [],
                    "risks": []
                },
                "work_item": {
                    "id": "item-1",
                    "source": "plan",
                    "milestone_id": "m1",
                    "title": "Add stage",
                    "objective": "Implement the feature",
                    "contract_refs": ["AC-1"],
                    "acceptance_criteria": ["works"],
                    "expected_files": ["src/workflow.rs", "src/prompts.rs"],
                    "verification_commands": ["cargo test"],
                    "dependencies": [],
                    "rollback_notes": ["revert the stage"]
                },
                "completed_items": [],
                "prior_feedback": []
            }))
            .expect("implementation task input should parse"),
            worktree_name: "initial-implementation-item-1".to_string(),
            delta: ImplementationDelta {
                summary: "implemented".to_string(),
                rationale: vec!["kept it small".to_string()],
                changes: vec![
                    FileDelta {
                        path: "src/workflow.rs".to_string(),
                        action: "write".to_string(),
                        content: Some("content".to_string()),
                    },
                    FileDelta {
                        path: "src/prompts.rs".to_string(),
                        action: "write".to_string(),
                        content: Some("content".to_string()),
                    },
                ],
            },
        };

        let result = item_result_from_draft(&item, &draft);

        assert_eq!(result.item_id, "item-1");
        assert_eq!(result.source, "plan");
        assert_eq!(result.milestone_id.as_deref(), Some("m1"));
        assert_eq!(result.objective, "Implement the feature");
        assert_eq!(result.contract_refs, vec!["AC-1"]);
        assert_eq!(
            result.changed_files,
            vec!["src/workflow.rs", "src/prompts.rs"]
        );
        assert_eq!(result.rationale, vec!["kept it small"]);
        assert_eq!(result.manual_checks, vec!["works"]);
        assert_eq!(result.commands_run[0].outcome, "passed");
        assert_eq!(result.worktree_name, "initial-implementation-item-1");
    }
}
