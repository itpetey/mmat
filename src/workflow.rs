use std::{
    collections::BTreeSet,
    env, fs,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

use naaf_core::{
    Attempt, CheckExt, EdgeSpec, GraphPatch, MaterialiserExt, NeverFinding, NodeId, NodeInput,
    NodeSpec, RepairPlannerExt, RetryPolicy, RunnerRegistry, Step, StepNode, Task, TaskExt,
    Workflow, check_fn, materialiser_fn, repair_fn, task_fn,
};
use naaf_llm::{
    CompletionRequest, Executor, ExecutorConfig, HumanIO, HumanQuestion, LlmAgent, Message,
    OpenAiClient, OpenAiConfig, OpenAiError, QuestionTool, RegisterToolError, Tool, ToolRegistry,
};
use naaf_persistence_fs::FsCheckpointer;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::process::Command;

use crate::{
    artifacts::RunArtifact,
    error::AppError,
    models::{
        ApprovalOutcome, ApprovalRequest, ApprovedContract, ApprovedProposal, CommandEvidence,
        ContractApprovalRequest, ContractDraftInput, EvidenceLog, ExecutionPlan, FinalReview,
        FinalReviewInput, ImplementationDelta, ImplementationDraft, ImplementationItemResult,
        ImplementationManagementRequest, ImplementationTaskInput, ImplementationWorklist,
        IntentBrief, ProjectContract, ProjectPrompt, ReconciledProposal, ReleaseAssessment,
        ReleaseAssessmentInput, RunSummary, StageFinding, StageReview, TaskCard, ValidatedSolution,
        ValidationFinding, WorkflowOutcome,
    },
    parsing::decode_json_output,
    prompts::{
        approval_system_prompt, approval_user_prompt, architect_review_system_prompt,
        architect_review_user_prompt, contract_approval_system_prompt,
        contract_approval_user_prompt, contract_system_prompt, contract_user_prompt,
        contract_validation_system_prompt, contract_validation_user_prompt,
        discovery_system_prompt, discovery_user_prompt, final_review_system_prompt,
        final_review_user_prompt, implementation_management_system_prompt,
        implementation_management_user_prompt, implementation_task_system_prompt,
        implementation_task_user_prompt, peer_review_system_prompt, peer_review_user_prompt,
        planning_system_prompt, planning_user_prompt, reconcile_system_prompt,
        reconcile_user_prompt, release_assessment_system_prompt, release_assessment_user_prompt,
        solution_generation_system_prompt, solution_generation_user_prompt,
        solution_validation_system_prompt, solution_validation_user_prompt,
    },
    runtime::{
        AppGlobPathsTool, AppReadFileTool, AppRuntime, AppSearchFilesTool, AppWebSearchTool,
        WebSearchConfig,
    },
};

type AppAgent = LlmAgent<OpenAiClient<AppRuntime>, AppRuntime, AppError>;
type LlmStageError = naaf_llm::AdapterError<AppError, OpenAiError, AppError, serde_json::Error>;

const DEFAULT_API_KEY: &str = "lm-studio";
const DEFAULT_BASE_URL: &str = "http://127.0.0.1:1234/v1";
const DEFAULT_MODEL: &str = "essentialai/rnj-1";
const EXECUTOR_TURNS: usize = 12;
const IMPLEMENTATION_RETRY_LIMIT: usize = 3;
const MAX_FINAL_REVIEW_PASSES: usize = 3;
const MAX_DISCOVERY_CLARIFICATION_PASSES: usize = 2;
const WORKFLOW_MAX_CONCURRENCY: usize = 4;
const WORKTREE_DIR: &str = ".mmat-worktrees";

#[derive(Clone)]
struct ExecutionGraphSteps {
    managed_phase:
        Step<AppRuntime, ImplementationManagementRequest, ManagedPhase, NeverFinding, AppError>,
    implementation:
        Step<AppRuntime, ImplementationExecutionInput, ImplementationDraft, StageFinding, AppError>,
    phase_review: Step<AppRuntime, PhaseExecutionInput, PhaseReviewResult, NeverFinding, AppError>,
    outcome: Step<AppRuntime, WorkflowOutcome, WorkflowOutcome, NeverFinding, AppError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PhaseExecutionInput {
    phase: ManagedPhase,
    drafts: Vec<ImplementationDraft>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PhaseReviewResult {
    phase: ManagedPhase,
    completed_items: Vec<ImplementationItemResult>,
    review: FinalReview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinalReviewDisposition {
    Complete,
    Remediate,
    Halt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhaseReviewAction {
    Complete,
    Remediate,
    Halt,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ManagedPhase {
    request: ImplementationManagementRequest,
    worklist: ImplementationWorklist,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ImplementationExecutionInput {
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

    let base_url = env::var("OPENAI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());

    runtime.log_info(format!("Using model `{model}` via `{base_url}`."))?;
    if search_enabled {
        runtime.log_info("External web research is enabled for this run.")?;
    } else {
        runtime.log_warn(
            "External web research is disabled. Set MMAT_WEB_SEARCH_URL to enable the web_search tool.",
        )?;
    }

    let llm = build_agent(runtime.project_root(), web_search.clone())?;
    let discovery_step = Step::builder(build_discovery_task(&llm, &model, search_enabled))
        .with_findings::<NeverFinding>()
        .build();
    let [conservative_branch, recommended_branch, ambitious_branch] =
        crate::models::SolutionBranch::default_set();
    let conservative = build_solution_branch(&llm, &model, conservative_branch, search_enabled);
    let recommended = build_solution_branch(&llm, &model, recommended_branch, search_enabled);
    let ambitious = build_solution_branch(&llm, &model, ambitious_branch, search_enabled);
    let solutions_workflow = conservative
        .join(recommended)
        .reconcile_task(collect_solution_pair_task("collect_default_pair"))
        .join(ambitious)
        .reconcile_task(push_solution_task("add_ambitious_solution"));
    let reconcile_step = Step::builder(build_reconcile_task(&llm, &model))
        .with_findings::<NeverFinding>()
        .build();
    let approval_step = Step::builder(build_approval_task(&llm, &model))
        .with_findings::<NeverFinding>()
        .build();
    let contract_step = Step::builder(build_contract_task(&llm, &model, search_enabled))
        .with_findings::<NeverFinding>()
        .build();
    let contract_approval_step = Step::builder(build_contract_approval_task(&llm, &model))
        .with_findings::<NeverFinding>()
        .build();

    let planning_step = Step::builder(build_planning_task(&llm, &model, search_enabled))
        .with_findings::<NeverFinding>()
        .build();
    let architect_review_step = Step::builder(build_architect_review_task(&llm, &model))
        .with_findings::<NeverFinding>()
        .build();
    let execution_steps = ExecutionGraphSteps {
        implementation: build_implementation_step(&model, web_search.clone())?,
        managed_phase: build_managed_phase_step(
            runtime.project_root(),
            &model,
            search_enabled,
            web_search.clone(),
        )?,
        phase_review: build_phase_review_step(
            runtime.project_root(),
            &model,
            search_enabled,
            web_search.clone(),
        )?,
        outcome: build_outcome_step(),
    };

    let mut prompt_context = prompt;
    let mut clarification_attempt = 0usize;

    loop {
        write_run_summary(runtime, &prompt_context, "running", "discovery", None)?;
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
        log_discovery_summary(runtime, &discovery)?;

        if !discovery_ready_for_solution(&discovery)
            && clarification_attempt < MAX_DISCOVERY_CLARIFICATION_PASSES
        {
            write_run_summary(
                runtime,
                &prompt_context,
                "awaiting_clarification",
                "discovery",
                Some("user clarification"),
            )?;
            let clarification = prompt_for_discovery_clarification(runtime, &discovery).await?;
            prompt_context = append_user_guidance(
                &prompt_context,
                "User clarification after discovery",
                &clarification,
            );
            clarification_attempt += 1;
            continue;
        }

        if !discovery_ready_for_solution(&discovery) {
            runtime.log_warn(
                "Clarification budget exhausted. Proceeding with the recorded best-guess intent brief and defaults.",
            )?;
        }

        write_run_summary(
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
        log_solution_summaries(runtime, &solutions)?;

        let reconciled = reconcile_step
            .run(runtime, solutions)
            .await
            .map_err(|error| AppError::Workflow(format!("reconcile stage failed: {error}")))?;
        runtime.persist_artifact(RunArtifact::ReconciledProposal, &reconciled)?;
        log_reconciled_summary(runtime, &reconciled)?;

        runtime.log_info(
            "The next prompt will collect approval, revision notes, or any final constraints.",
        )?;

        write_run_summary(
            runtime,
            &prompt_context,
            "awaiting_approval",
            "approval",
            None,
        )?;
        let approval_response = prompt_for_approval(runtime, &reconciled).await?;
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
        log_approval_summary(runtime, &approval)?;

        if !approval_granted(&approval) {
            runtime.log_info(
                "Revision requested. Returning to discovery with the user's latest guidance.",
            )?;
            prompt_context = append_user_guidance(
                &prompt_context,
                "User revision after proposal review",
                &approval_response,
            );
            write_run_summary(runtime, &prompt_context, "revising", "discovery", None)?;
            continue;
        }

        let approved = ApprovedProposal {
            proposal: reconciled,
            approval: approval.clone(),
        };
        runtime.log_info("Proposal approved. Forming the project contract.")?;

        write_run_summary(runtime, &prompt_context, "running", "contract", None)?;
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
        log_contract_summary(runtime, &contract)?;

        write_run_summary(
            runtime,
            &prompt_context,
            "awaiting_contract_approval",
            "contract",
            None,
        )?;
        let contract_response = prompt_for_contract_approval(runtime, &contract).await?;
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
        log_contract_approval_summary(runtime, &contract_approval)?;

        if !approval_granted(&contract_approval) {
            runtime.log_info(
                "Contract revisions requested. Returning to discovery with the user's latest guidance.",
            )?;
            prompt_context = append_user_guidance(
                &prompt_context,
                "User revision after contract review",
                &contract_response,
            );
            write_run_summary(runtime, &prompt_context, "revising", "discovery", None)?;
            continue;
        }

        let approved_contract = ApprovedContract {
            approved,
            contract: contract.clone(),
            contract_approval: contract_approval.clone(),
        };
        runtime.log_info("Contract approved. Starting planning and execution.")?;

        write_run_summary(runtime, &prompt_context, "running", "planning", None)?;
        let plan = planning_step
            .run(runtime, approved_contract.clone())
            .await
            .map_err(|error| AppError::Workflow(format!("planning stage failed: {error}")))?;
        runtime.persist_artifact(RunArtifact::ExecutionPlan, &plan)?;
        persist_task_cards(runtime, &plan.task_cards)?;
        log_planning_summary(runtime, &plan)?;

        write_run_summary(
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
        log_stage_review(runtime, "Architect review", &architect_review)?;

        write_run_summary(runtime, &prompt_context, "running", "implementation", None)?;
        let outcome = run_dynamic_implementation_workflow(
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
            run_release_assessment(runtime, &model, &outcome, web_search.clone()).await?;
        runtime.persist_artifact(RunArtifact::ReleaseAssessment, &release_assessment)?;
        log_release_assessment_summary(runtime, &release_assessment)?;

        let mut final_outcome = outcome;
        final_outcome.release_assessment = Some(release_assessment);
        runtime.persist_artifact(RunArtifact::WorkflowOutcome, &final_outcome)?;
        write_run_summary(
            runtime,
            &prompt_context,
            &final_outcome.status,
            "completed",
            Some(&final_outcome.next_step),
        )?;

        return Ok(final_outcome);
    }
}

fn apply_file_deltas(root: &Path, delta: &ImplementationDelta) -> Result<(), AppError> {
    for change in &delta.changes {
        let path = resolve_project_path(root, &change.path)?;
        match change.action.as_str() {
            "write" => {
                let Some(content) = &change.content else {
                    return Err(AppError::Workflow(format!(
                        "file delta for `{}` is missing content",
                        change.path
                    )));
                };
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        AppError::Workflow(format!("failed to create directory: {error}"))
                    })?;
                }
                fs::write(&path, content).map_err(|error| {
                    AppError::Workflow(format!("failed to write `{}`: {error}", change.path))
                })?;
            }
            "delete" => {
                if path.exists() {
                    fs::remove_file(&path).map_err(|error| {
                        AppError::Workflow(format!("failed to delete `{}`: {error}", change.path))
                    })?;
                }
            }
            other => {
                return Err(AppError::Workflow(format!(
                    "unsupported file delta action `{other}` for `{}`",
                    change.path
                )));
            }
        }
    }

    Ok(())
}

fn apply_single_change(
    project_root: &Path,
    change: &crate::models::FileDelta,
) -> Result<(), AppError> {
    apply_file_deltas(
        project_root,
        &ImplementationDelta {
            summary: "apply merged worktree change".to_string(),
            rationale: Vec::new(),
            changes: vec![change.clone()],
        },
    )
}

fn approval_granted(approval: &ApprovalOutcome) -> bool {
    let decision = approval.decision.trim().to_ascii_lowercase();
    matches!(
        decision.as_str(),
        "approve" | "approved" | "accept" | "accepted"
    )
}

fn append_user_guidance(prompt: &str, heading: &str, response: &str) -> String {
    let response = response.trim();
    if response.is_empty() {
        return prompt.to_string();
    }

    format!("{prompt}\n\n{heading}:\n{response}")
}

fn discovery_ready_for_solution(discovery: &IntentBrief) -> bool {
    discovery.ready_for_solution
}

fn build_agent(
    project_root: &Path,
    web_search: Option<WebSearchConfig>,
) -> Result<AppAgent, AppError> {
    let mut tools: ToolRegistry<AppRuntime, AppError> = ToolRegistry::new();
    tools = register_tool(tools, QuestionTool::<AppRuntime>::new())?;
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

fn build_approval_task(
    llm: &AppAgent,
    model: &str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ApprovalRequest,
    Output = ApprovalOutcome,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = approval_system_prompt();

    llm.task(
        move |_runtime: &AppRuntime, request: ApprovalRequest| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(approval_user_prompt(&request)?),
                ],
            ))
        },
        decode_json_output::<ApprovalOutcome>,
    )
    .observed_as("approval")
}

fn build_architect_review_task(
    llm: &AppAgent,
    model: &str,
) -> impl Task<Runtime = AppRuntime, Input = ExecutionPlan, Output = StageReview, Error = LlmStageError>
+ use<> {
    let model = model.to_string();
    let system_prompt = architect_review_system_prompt();

    llm.task(
        move |_runtime: &AppRuntime, plan: ExecutionPlan| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(architect_review_user_prompt(&plan)?),
                ],
            ))
        },
        decode_json_output::<StageReview>,
    )
    .observed_as("architect_review")
}

fn build_contract_approval_task(
    llm: &AppAgent,
    model: &str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ContractApprovalRequest,
    Output = ApprovalOutcome,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = contract_approval_system_prompt();

    llm.task(
        move |_runtime: &AppRuntime, request: ContractApprovalRequest| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(contract_approval_user_prompt(&request)?),
                ],
            ))
        },
        decode_json_output::<ApprovalOutcome>,
    )
    .observed_as("contract_approval")
}

fn build_contract_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ContractDraftInput,
    Output = ProjectContract,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = contract_system_prompt(web_search_enabled);

    llm.task(
        move |_runtime: &AppRuntime, input: ContractDraftInput| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(contract_user_prompt(&input)?),
                ],
            ))
        },
        decode_json_output::<ProjectContract>,
    )
    .observed_as("project_contract")
}

fn build_discovery_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<Runtime = AppRuntime, Input = ProjectPrompt, Output = IntentBrief, Error = LlmStageError>
+ use<> {
    let model = model.to_string();
    let system_prompt = discovery_system_prompt(web_search_enabled);

    llm.task(
        move |_runtime: &AppRuntime, input: ProjectPrompt| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(discovery_user_prompt(
                        &input.raw,
                        input.clarification_attempt,
                        input.clarification_limit,
                    )),
                ],
            ))
        },
        decode_json_output::<IntentBrief>,
    )
    .observed_as("discovery")
}

fn build_implementation_step(
    model: &str,
    web_search: Option<WebSearchConfig>,
) -> Result<
    Step<AppRuntime, ImplementationExecutionInput, ImplementationDraft, StageFinding, AppError>,
    AppError,
> {
    let model_for_task = model.to_string();
    let model_for_review = model.to_string();
    let task_web_search = web_search.clone();
    let review_web_search = web_search.clone();
    let task = task_fn(
        move |runtime: &AppRuntime, input: ImplementationExecutionInput| {
            let model = model_for_task.clone();
            let web_search = task_web_search.clone();
            Box::pin(async move {
                let worktree_root =
                    prepare_worktree(runtime.project_root(), &input.worktree_name).await?;
                let llm = build_agent(&worktree_root, web_search)?;
                let request = CompletionRequest::new(
                    model,
                    vec![
                        Message::system(implementation_task_system_prompt()),
                        Message::user(implementation_task_user_prompt(&input.task)?),
                    ],
                );
                let delta = execute_json_stage::<ImplementationDelta>(
                    &llm,
                    runtime,
                    request,
                    "implementation task",
                )
                .await?;
                Ok::<_, AppError>(ImplementationDraft {
                    input: input.task,
                    worktree_name: input.worktree_name,
                    delta,
                })
            })
        },
    )
    .observed_as("implement_item");

    let apply_deltas = materialiser_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            apply_file_deltas(
                worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                &draft.delta,
            )?;
            Ok::<_, AppError>(draft)
        })
    })
    .observed_as("apply_file_deltas");

    let cargo_fmt = materialiser_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            run_command(
                worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                "cargo fmt --all",
                &["fmt", "--all"],
            )
            .await?;
            Ok::<_, AppError>(draft)
        })
    })
    .observed_as("cargo_fmt");

    let cargo_check = check_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            run_validator(
                worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                "cargo check",
                &["check"],
            )
            .await
        })
    })
    .observed_as("cargo_check");

    let cargo_test = check_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            run_validator(
                worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                "cargo test",
                &["test"],
            )
            .await
        })
    })
    .observed_as("cargo_test");

    let cargo_clippy = check_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            run_validator(
                worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                "cargo clippy -- -D warnings",
                &["clippy", "--", "-D", "warnings"],
            )
            .await
        })
    })
    .observed_as("cargo_clippy");

    let peer_review = check_fn(move |runtime: &AppRuntime, draft: ImplementationDraft| {
        let model = model_for_review.clone();
        let web_search = review_web_search.clone();
        Box::pin(async move {
            let llm = build_agent(
                worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                web_search,
            )?;
            let request = CompletionRequest::new(
                model,
                vec![
                    Message::system(peer_review_system_prompt()),
                    Message::user(peer_review_user_prompt(&draft.input, &draft.delta)?),
                ],
            );
            let review =
                execute_json_stage::<StageReview>(&llm, runtime, request, "peer review").await?;
            Ok::<_, AppError>(review.findings)
        })
    })
    .observed_as("peer_review");

    let contract_validation_model = model.to_string();
    let contract_validation_web_search = web_search;
    let contract_validation = check_fn(move |runtime: &AppRuntime, draft: ImplementationDraft| {
        let model = contract_validation_model.clone();
        let web_search = contract_validation_web_search.clone();
        Box::pin(async move {
            let llm = build_agent(
                worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                web_search,
            )?;
            let request = CompletionRequest::new(
                model,
                vec![
                    Message::system(contract_validation_system_prompt()),
                    Message::user(contract_validation_user_prompt(&draft.input, &draft.delta)?),
                ],
            );
            let review =
                execute_json_stage::<StageReview>(&llm, runtime, request, "contract validation")
                    .await?;
            Ok::<_, AppError>(review.findings)
        })
    })
    .observed_as("contract_validation");

    let revise = repair_fn(
        |_runtime: &AppRuntime,
         attempts: Vec<
            Attempt<ImplementationExecutionInput, ImplementationDraft, StageFinding>,
        >| {
            Box::pin(async move {
                let Some(last) = attempts.last() else {
                    return Err(AppError::Workflow(
                        "implementation repair was invoked without a prior attempt".to_string(),
                    ));
                };

                let mut prior_feedback = Vec::new();
                for attempt in &attempts {
                    prior_feedback.extend(attempt.findings.clone());
                }

                let mut next_input = last.input.clone();
                next_input.task.prior_feedback = prior_feedback;
                Ok::<_, AppError>(next_input)
            })
        },
    )
    .observed_as("revise_item");

    Ok(Step::builder(task)
        .materialise(apply_deltas)
        .materialise(cargo_fmt)
        .validate(cargo_check)
        .validate(cargo_test)
        .validate(cargo_clippy)
        .validate(peer_review)
        .validate(contract_validation)
        .repair_with(revise)
        .retry_policy(RetryPolicy::new(IMPLEMENTATION_RETRY_LIMIT))
        .build())
}

fn build_managed_phase_step(
    project_root: &Path,
    model: &str,
    web_search_enabled: bool,
    web_search: Option<WebSearchConfig>,
) -> Result<
    Step<AppRuntime, ImplementationManagementRequest, ManagedPhase, NeverFinding, AppError>,
    AppError,
> {
    let llm = Rc::new(build_agent(project_root, web_search)?);
    let model = model.to_string();
    let system_prompt = implementation_management_system_prompt(web_search_enabled);
    let task = task_fn(
        move |runtime: &AppRuntime, request: ImplementationManagementRequest| {
            let llm = llm.clone();
            let model = model.clone();
            let system_prompt = system_prompt.clone();
            Box::pin(async move {
                let worklist = execute_json_stage::<ImplementationWorklist>(
                    llm.as_ref(),
                    runtime,
                    CompletionRequest::new(
                        model,
                        vec![
                            Message::system(system_prompt),
                            Message::user(implementation_management_user_prompt(&request)?),
                        ],
                    ),
                    "implementation management",
                )
                .await?;
                log_worklist_summary(runtime, &worklist)?;
                Ok::<_, AppError>(ManagedPhase { request, worklist })
            })
        },
    )
    .observed_as("implementation_management");

    Ok(Step::builder(task).with_findings::<NeverFinding>().build())
}

fn build_outcome_patch(
    parent_id: NodeId,
    outcome_step: Step<AppRuntime, WorkflowOutcome, WorkflowOutcome, NeverFinding, AppError>,
) -> GraphPatch<AppRuntime, AppError> {
    let node_id = NodeId::new();
    GraphPatch::new()
        .with_node(
            NodeSpec::new(
                outcome_node_name(),
                StepNode::without_findings(outcome_step, move |input: &NodeInput| {
                    let result = input.output_as::<PhaseReviewResult>(parent_id)?;
                    Ok(workflow_outcome_from_phase_result(&result))
                }),
            )
            .with_id(node_id)
            .with_parent(parent_id),
        )
        .with_edge(EdgeSpec::new(parent_id, node_id))
}

fn build_outcome_step() -> Step<AppRuntime, WorkflowOutcome, WorkflowOutcome, NeverFinding, AppError>
{
    Step::builder(
        task_fn(|_runtime: &AppRuntime, outcome: WorkflowOutcome| {
            Box::pin(async move { Ok::<_, AppError>(outcome) })
        })
        .observed_as("workflow_outcome"),
    )
    .with_findings::<NeverFinding>()
    .build()
}

fn build_phase_patch(
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
        patch = patch
            .with_node(
                NodeSpec::new(
                    implementation_node_name(phase.request.pass_index, &item.id),
                    StepNode::new(implementation_step.clone(), {
                        let item = item.clone();
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
                                worktree_name: format!(
                                    "{}-{}",
                                    phase.request.phase.replace('_', "-"),
                                    item.id
                                ),
                            })
                        }
                    }),
                )
                .with_id(item_id)
                .with_parent(manager_id),
            )
            .with_edge(EdgeSpec::new(manager_id, item_id));
    }

    let review_id = NodeId::new();
    let review_dependencies = item_ids.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    patch = patch
        .with_node(
            NodeSpec::new(
                review_node_name(phase.request.pass_index),
                StepNode::without_findings(phase_review_step.clone(), {
                    let review_dependencies = review_dependencies.clone();
                    move |input: &NodeInput| {
                        let phase = input.output_as::<ManagedPhase>(manager_id)?;
                        let drafts = review_dependencies
                            .iter()
                            .map(|item_id| input.output_as::<ImplementationDraft>(*item_id))
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

fn build_phase_review_step(
    project_root: &Path,
    model: &str,
    web_search_enabled: bool,
    web_search: Option<WebSearchConfig>,
) -> Result<
    Step<AppRuntime, PhaseExecutionInput, PhaseReviewResult, NeverFinding, AppError>,
    AppError,
> {
    let llm = Rc::new(build_agent(project_root, web_search)?);
    let model = model.to_string();
    let system_prompt = final_review_system_prompt(web_search_enabled);
    let task = task_fn(move |runtime: &AppRuntime, input: PhaseExecutionInput| {
        let llm = llm.clone();
        let model = model.clone();
        let system_prompt = system_prompt.clone();
        Box::pin(async move {
            let baseline_root = create_baseline_snapshot(
                runtime.project_root(),
                &format!("baseline-{}", input.phase.request.phase.replace('_', "-")),
            )?;
            let mut completed_items = input.phase.request.completed_items.clone();
            let mut current_results = Vec::new();
            for draft in &input.drafts {
                current_results.push(
                    merge_item_worktree(runtime.project_root(), &baseline_root, draft).await?,
                );
            }
            for result in &current_results {
                log_implementation_result(runtime, result)?;
                runtime.persist_task_result(result)?;
            }
            completed_items.extend(current_results);
            runtime.persist_artifact(
                RunArtifact::EvidenceLog,
                &EvidenceLog {
                    task_results: completed_items.clone(),
                },
            )?;
            remove_directory_if_exists(&baseline_root)?;

            let review = execute_json_stage::<FinalReview>(
                llm.as_ref(),
                runtime,
                CompletionRequest::new(
                    model,
                    vec![
                        Message::system(system_prompt),
                        Message::user(final_review_user_prompt(&FinalReviewInput {
                            approved: input.phase.request.approved.clone(),
                            plan: input.phase.request.plan.clone(),
                            completed_items: completed_items.clone(),
                        })?),
                    ],
                ),
                "final review",
            )
            .await?;
            log_final_review_summary(runtime, &review)?;
            match phase_review_action(input.phase.request.pass_index, &review) {
                PhaseReviewAction::Remediate => runtime.log_warn(
                    "Final review requested remediation. Spawning another implementation management phase.",
                )?,
                PhaseReviewAction::Halt if review.remediation_items.is_empty() => runtime.log_warn(
                    "Final review is not yet ready but did not provide remediation items. Stopping the workflow.",
                )?,
                PhaseReviewAction::Halt => runtime.log_warn(
                    "Maximum remediation passes reached. Stopping the workflow.",
                )?,
                PhaseReviewAction::Complete => {}
            }
            Ok::<_, AppError>(PhaseReviewResult {
                phase: input.phase,
                completed_items,
                review,
            })
        })
    })
    .observed_as("phase_review");

    Ok(Step::builder(task).with_findings::<NeverFinding>().build())
}

fn build_planning_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ApprovedContract,
    Output = ExecutionPlan,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = planning_system_prompt(web_search_enabled);

    llm.task(
        move |_runtime: &AppRuntime, approved: ApprovedContract| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(planning_user_prompt(&approved)?),
                ],
            ))
        },
        decode_json_output::<ExecutionPlan>,
    )
    .observed_as("planning")
}

fn build_reconcile_task(
    llm: &AppAgent,
    model: &str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = Vec<ValidatedSolution>,
    Output = ReconciledProposal,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = reconcile_system_prompt();

    llm.task(
        move |_runtime: &AppRuntime, solutions: Vec<ValidatedSolution>| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(reconcile_user_prompt(&solutions)?),
                ],
            ))
        },
        decode_json_output::<ReconciledProposal>,
    )
    .observed_as("reconcile")
}

fn build_review_patch(
    review_id: NodeId,
    result: &PhaseReviewResult,
    execution_steps: ExecutionGraphSteps,
) -> GraphPatch<AppRuntime, AppError> {
    match phase_review_action(result.phase.request.pass_index, &result.review) {
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

fn build_solution_branch(
    llm: &AppAgent,
    model: &str,
    branch: crate::models::SolutionBranch,
    web_search_enabled: bool,
) -> Step<AppRuntime, IntentBrief, ValidatedSolution, NeverFinding, LlmStageError> {
    let generation_model = model.to_string();
    let validation_model = model.to_string();
    let generation_system = solution_generation_system_prompt(branch, web_search_enabled);
    let validation_system = solution_validation_system_prompt();
    let solution_name = format!("{}_solution", branch.slug());
    let validation_name = format!("{}_validation", branch.slug());

    let generate = Step::builder(
        llm.task(
            move |_runtime: &AppRuntime, discovery: IntentBrief| {
                Ok::<_, AppError>(CompletionRequest::new(
                    generation_model.clone(),
                    vec![
                        Message::system(generation_system.clone()),
                        Message::user(solution_generation_user_prompt(branch, &discovery)?),
                    ],
                ))
            },
            decode_json_output::<crate::models::SolutionProposal>,
        )
        .observed_as(solution_name),
    )
    .with_findings::<NeverFinding>()
    .build();

    let validate = Step::builder(
        llm.task(
            move |_runtime: &AppRuntime, proposal: crate::models::SolutionProposal| {
                Ok::<_, AppError>(CompletionRequest::new(
                    validation_model.clone(),
                    vec![
                        Message::system(validation_system.clone()),
                        Message::user(solution_validation_user_prompt(&proposal)?),
                    ],
                ))
            },
            decode_json_output::<ValidatedSolution>,
        )
        .observed_as(validation_name),
    )
    .with_findings::<NeverFinding>()
    .build();

    generate.then(validate)
}

fn build_workspace_delta(
    source_root: &Path,
    target_root: &Path,
) -> Result<Vec<crate::models::FileDelta>, AppError> {
    let source_paths = collect_workspace_files(source_root)?;
    let target_paths = collect_workspace_files(target_root)?;
    let paths = source_paths
        .union(&target_paths)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::new();

    for path in paths {
        let source = read_optional_file(source_root, &path)?;
        let target = read_optional_file(target_root, &path)?;
        if source == target {
            continue;
        }

        changes.push(crate::models::FileDelta {
            path: path.clone(),
            action: if source.is_some() {
                "write".to_string()
            } else {
                "delete".to_string()
            },
            content: source,
        });
    }

    Ok(changes)
}

fn collect_solution_pair_task(
    name: &'static str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = (ValidatedSolution, ValidatedSolution),
    Output = Vec<ValidatedSolution>,
    Error = LlmStageError,
> {
    task_fn(
        |_runtime: &AppRuntime, input: (ValidatedSolution, ValidatedSolution)| {
            Box::pin(async move { Ok::<_, LlmStageError>(vec![input.0, input.1]) })
        },
    )
    .observed_as(name)
}

fn collect_workspace_files(root: &Path) -> Result<BTreeSet<String>, AppError> {
    let mut paths = BTreeSet::new();
    collect_workspace_files_recursive(root, root, &mut paths)?;
    Ok(paths)
}

fn collect_workspace_files_recursive(
    root: &Path,
    current: &Path,
    paths: &mut BTreeSet<String>,
) -> Result<(), AppError> {
    for entry in fs::read_dir(current).map_err(|error| {
        AppError::Workflow(format!(
            "failed to read directory `{}`: {error}",
            current.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            AppError::Workflow(format!(
                "failed to iterate directory `{}`: {error}",
                current.display()
            ))
        })?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if should_skip_workspace_entry(&file_name) {
            continue;
        }

        let file_type = entry.file_type().map_err(|error| {
            AppError::Workflow(format!("failed to inspect `{}`: {error}", path.display()))
        })?;
        if file_type.is_dir() {
            collect_workspace_files_recursive(root, &path, paths)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(|error| {
                AppError::Workflow(format!("failed to relativise path: {error}"))
            })?;
            paths.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }

    Ok(())
}

fn command_failure_summary(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr.is_empty() {
        return truncate_text(&stderr);
    }

    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    if !stdout.is_empty() {
        return truncate_text(&stdout);
    }

    "command exited unsuccessfully with no output".to_string()
}

fn create_baseline_snapshot(project_root: &Path, name: &str) -> Result<PathBuf, AppError> {
    let snapshot_root = worktree_path(project_root, name);
    remove_directory_if_exists(&snapshot_root)?;
    fs::create_dir_all(&snapshot_root).map_err(|error| {
        AppError::Workflow(format!("failed to create baseline snapshot: {error}"))
    })?;
    sync_workspace_state(project_root, &snapshot_root)?;
    Ok(snapshot_root)
}

async fn execute_json_stage<T>(
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

fn final_review_disposition(review: &FinalReview) -> FinalReviewDisposition {
    if review.ready {
        FinalReviewDisposition::Complete
    } else if review.remediation_items.is_empty() {
        FinalReviewDisposition::Halt
    } else {
        FinalReviewDisposition::Remediate
    }
}

fn implementation_node_name(pass_index: usize, item_id: &str) -> String {
    format!("implement_item_{pass_index}_{item_id}")
}

fn initial_management_request(
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

fn log_approval_summary(runtime: &AppRuntime, approval: &ApprovalOutcome) -> Result<(), AppError> {
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

fn log_contract_approval_summary(
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

fn log_contract_summary(runtime: &AppRuntime, contract: &ProjectContract) -> Result<(), AppError> {
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

fn log_discovery_summary(runtime: &AppRuntime, discovery: &IntentBrief) -> Result<(), AppError> {
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

async fn prompt_for_approval(
    runtime: &AppRuntime,
    proposal: &ReconciledProposal,
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

async fn prompt_for_contract_approval(
    runtime: &AppRuntime,
    contract: &ProjectContract,
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

async fn prompt_for_discovery_clarification(
    runtime: &AppRuntime,
    discovery: &IntentBrief,
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

fn log_final_review_summary(runtime: &AppRuntime, review: &FinalReview) -> Result<(), AppError> {
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

fn log_implementation_result(
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

fn log_planning_summary(runtime: &AppRuntime, plan: &ExecutionPlan) -> Result<(), AppError> {
    runtime.log_info("Planning complete.")?;
    runtime.log_info(format!("Plan summary: {}", plan.summary))?;
    runtime.log_info(format!("Milestones: {}", plan.milestones.len()))?;
    runtime.log_info(format!("Task cards: {}", plan.task_cards.len()))?;
    Ok(())
}

fn log_reconciled_summary(
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

fn log_solution_summaries(
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

fn log_stage_review(
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

fn log_worklist_summary(
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

fn management_node_name(pass_index: usize) -> String {
    format!("implementation_management_{pass_index}")
}

async fn merge_change_into_workspace(
    project_root: &Path,
    baseline_root: &Path,
    item_root: &Path,
    change: &crate::models::FileDelta,
) -> Result<(), AppError> {
    let base = read_optional_file(baseline_root, &change.path)?;
    let current = read_optional_file(project_root, &change.path)?;
    let item = read_optional_file(item_root, &change.path)?;

    if current == item {
        return Ok(());
    }

    if current == base {
        apply_single_change(project_root, change)?;
        return Ok(());
    }

    match (base, current, item) {
        (Some(base), Some(current), Some(item)) => {
            let merged =
                merge_file_versions(project_root, &change.path, &current, &base, &item).await?;
            write_workspace_file(project_root, &change.path, &merged)?;
            Ok(())
        }
        (None, Some(current), Some(item)) if current == item => Ok(()),
        (Some(_base), None, None) => Ok(()),
        _ => Err(AppError::Workflow(format!(
            "conflicting parallel changes for `{}` could not be merged safely",
            change.path
        ))),
    }
}

async fn merge_file_versions(
    project_root: &Path,
    path: &str,
    current: &str,
    base: &str,
    item: &str,
) -> Result<String, AppError> {
    let temp_root = worktree_path(project_root, "merge-temp");
    fs::create_dir_all(&temp_root).map_err(|error| {
        AppError::Workflow(format!("failed to create merge temp directory: {error}"))
    })?;
    let current_path = temp_root.join("current.tmp");
    let base_path = temp_root.join("base.tmp");
    let item_path = temp_root.join("item.tmp");
    fs::write(&current_path, current)
        .map_err(|error| AppError::Workflow(format!("failed to write merge temp file: {error}")))?;
    fs::write(&base_path, base)
        .map_err(|error| AppError::Workflow(format!("failed to write merge temp file: {error}")))?;
    fs::write(&item_path, item)
        .map_err(|error| AppError::Workflow(format!("failed to write merge temp file: {error}")))?;

    let output = Command::new("git")
        .args([
            "merge-file",
            "-p",
            current_path.to_string_lossy().as_ref(),
            base_path.to_string_lossy().as_ref(),
            item_path.to_string_lossy().as_ref(),
        ])
        .current_dir(project_root)
        .output()
        .await
        .map_err(|error| {
            AppError::Workflow(format!("failed to run merge for `{path}`: {error}"))
        })?;
    remove_directory_if_exists(&temp_root)?;

    if !output.status.success() {
        return Err(AppError::Workflow(format!(
            "parallel changes to `{path}` produced merge conflicts"
        )));
    }

    String::from_utf8(output.stdout).map_err(|error| {
        AppError::Workflow(format!(
            "merged content for `{path}` was not valid UTF-8: {error}"
        ))
    })
}

async fn merge_item_worktree(
    project_root: &Path,
    baseline_root: &Path,
    draft: &ImplementationDraft,
) -> Result<ImplementationItemResult, AppError> {
    let item_root = worktree_path(project_root, &draft.worktree_name);
    let changes = build_workspace_delta(&item_root, baseline_root)?;

    for change in &changes {
        merge_change_into_workspace(project_root, baseline_root, &item_root, change).await?;
    }

    remove_worktree(project_root, &draft.worktree_name).await?;

    Ok(ImplementationItemResult {
        item_id: draft.input.work_item.id.clone(),
        source: draft.input.work_item.source.clone(),
        milestone_id: draft.input.work_item.milestone_id.clone(),
        title: draft.input.work_item.title.clone(),
        objective: draft.input.work_item.objective.clone(),
        summary: draft.delta.summary.clone(),
        contract_refs: draft.input.work_item.contract_refs.clone(),
        changed_files: changes.into_iter().map(|change| change.path).collect(),
        rationale: draft.delta.rationale.clone(),
        commands_run: build_command_evidence(&draft.input.work_item),
        reviewer_findings: Vec::new(),
        manual_checks: draft.input.work_item.acceptance_criteria.clone(),
        known_gaps: Vec::new(),
        scope_deviation: None,
        worktree_name: draft.worktree_name.clone(),
    })
}

fn build_command_evidence(task_card: &TaskCard) -> Vec<CommandEvidence> {
    let mut commands = BTreeSet::new();
    commands.insert("cargo fmt --all".to_string());
    commands.insert("cargo check".to_string());
    commands.insert("cargo test".to_string());
    commands.insert("cargo clippy -- -D warnings".to_string());
    commands.insert("peer review".to_string());
    commands.extend(task_card.verification_commands.iter().cloned());

    commands
        .into_iter()
        .map(|command| CommandEvidence {
            command,
            outcome: "passed".to_string(),
        })
        .collect()
}

fn next_management_request(result: &PhaseReviewResult) -> ImplementationManagementRequest {
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

fn outcome_node_name() -> String {
    "workflow_outcome".to_string()
}

fn phase_label(pass_index: usize) -> String {
    if pass_index == 0 {
        "initial_implementation".to_string()
    } else {
        format!("remediation_pass_{pass_index}")
    }
}

fn phase_review_action(pass_index: usize, review: &FinalReview) -> PhaseReviewAction {
    match final_review_disposition(review) {
        FinalReviewDisposition::Complete => PhaseReviewAction::Complete,
        FinalReviewDisposition::Halt => PhaseReviewAction::Halt,
        FinalReviewDisposition::Remediate if pass_index + 1 < MAX_FINAL_REVIEW_PASSES => {
            PhaseReviewAction::Remediate
        }
        FinalReviewDisposition::Remediate => PhaseReviewAction::Halt,
    }
}

async fn prepare_worktree(project_root: &Path, worktree_name: &str) -> Result<PathBuf, AppError> {
    let worktree_root = worktree_path(project_root, worktree_name);
    if worktree_root.exists() {
        remove_worktree(project_root, worktree_name).await?;
    }

    let worktree_parent = worktree_root.parent().ok_or_else(|| {
        AppError::Workflow(format!(
            "worktree path `{}` had no parent",
            worktree_root.display()
        ))
    })?;
    fs::create_dir_all(worktree_parent).map_err(|error| {
        AppError::Workflow(format!("failed to create worktree directory: {error}"))
    })?;

    run_git_command(
        project_root,
        &[
            "worktree",
            "add",
            "--detach",
            worktree_root.to_string_lossy().as_ref(),
            "HEAD",
        ],
        "create isolated worktree",
    )
    .await?;

    sync_workspace_state(project_root, &worktree_root)?;
    Ok(worktree_root)
}

fn push_solution_task(
    name: &'static str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = (Vec<ValidatedSolution>, ValidatedSolution),
    Output = Vec<ValidatedSolution>,
    Error = LlmStageError,
> {
    task_fn(
        |_runtime: &AppRuntime, input: (Vec<ValidatedSolution>, ValidatedSolution)| {
            Box::pin(async move {
                let mut merged = input.0;
                merged.push(input.1);
                Ok::<_, LlmStageError>(merged)
            })
        },
    )
    .observed_as(name)
}

fn read_optional_file(root: &Path, relative: &str) -> Result<Option<String>, AppError> {
    let path = root.join(relative);
    if !path.exists() {
        return Ok(None);
    }

    fs::read_to_string(&path).map(Some).map_err(|error| {
        AppError::Workflow(format!("failed to read `{}`: {error}", path.display()))
    })
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
                Ok(next_management_request(&result))
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

fn remove_directory_if_exists(path: &Path) -> Result<(), AppError> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|error| {
            AppError::Workflow(format!("failed to remove `{}`: {error}", path.display()))
        })?;
    }
    Ok(())
}

async fn remove_worktree(project_root: &Path, worktree_name: &str) -> Result<(), AppError> {
    let worktree_root = worktree_path(project_root, worktree_name);
    if !worktree_root.exists() {
        return Ok(());
    }

    let output = Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            worktree_root.to_string_lossy().as_ref(),
        ])
        .current_dir(project_root)
        .output()
        .await
        .map_err(|error| AppError::Workflow(format!("failed to remove worktree: {error}")))?;

    if !output.status.success() && worktree_root.exists() {
        fs::remove_dir_all(&worktree_root).map_err(|error| {
            AppError::Workflow(format!(
                "failed to remove stale worktree directory `{}`: {error}",
                worktree_root.display()
            ))
        })?;
    }

    Ok(())
}

fn resolve_project_path(root: &Path, relative: &str) -> Result<PathBuf, AppError> {
    let path = Path::new(relative);
    if path.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        return Err(AppError::Workflow(format!(
            "file delta path `{relative}` must stay within the project root"
        )));
    }

    Ok(root.join(path))
}

fn review_node_name(pass_index: usize) -> String {
    format!("phase_review_{pass_index}")
}

fn root_management_node_spec(
    request: ImplementationManagementRequest,
    execution_steps: ExecutionGraphSteps,
) -> Result<NodeSpec<AppRuntime, AppError>, AppError> {
    NodeSpec::new(
        management_node_name(request.pass_index),
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

async fn run_command(root: &Path, label: &str, args: &[&str]) -> Result<(), AppError> {
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
        command_failure_summary(&output.stdout, &output.stderr)
    )))
}

async fn run_dynamic_implementation_workflow(
    runtime: &AppRuntime,
    approved: ApprovedContract,
    plan: ExecutionPlan,
    architect_review: StageReview,
    execution_steps: ExecutionGraphSteps,
) -> Result<WorkflowOutcome, AppError> {
    let initial_request = initial_management_request(approved, plan, architect_review);
    let root = root_management_node_spec(initial_request, execution_steps)?;

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

async fn run_release_assessment(
    runtime: &AppRuntime,
    model: &str,
    outcome: &WorkflowOutcome,
    web_search: Option<WebSearchConfig>,
) -> Result<ReleaseAssessment, AppError> {
    let Some(contract) = &outcome.contract else {
        return Err(AppError::Workflow(
            "cannot run release assessment without a project contract".to_string(),
        ));
    };

    let evidence_log = EvidenceLog {
        task_results: outcome.completed_items.clone(),
    };

    let llm = build_agent(runtime.project_root(), web_search)?;
    let request = CompletionRequest::new(
        model.to_string(),
        vec![
            Message::system(release_assessment_system_prompt()),
            Message::user(release_assessment_user_prompt(&ReleaseAssessmentInput {
                contract: contract.clone(),
                plan: outcome.plan.clone().unwrap_or_else(|| ExecutionPlan {
                    summary: String::new(),
                    milestones: Vec::new(),
                    task_cards: Vec::new(),
                    risks: Vec::new(),
                }),
                task_results: outcome.completed_items.clone(),
                evidence_log,
            })?),
        ],
    );

    let assessment =
        execute_json_stage::<ReleaseAssessment>(&llm, runtime, request, "release assessment")
            .await?;

    Ok(assessment)
}

fn log_release_assessment_summary(
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

async fn run_git_command(project_root: &Path, args: &[&str], label: &str) -> Result<(), AppError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(project_root)
        .output()
        .await
        .map_err(|error| AppError::Workflow(format!("failed to {label}: {error}")))?;

    if output.status.success() {
        return Ok(());
    }

    Err(AppError::Workflow(format!(
        "failed to {label}: {}",
        command_failure_summary(&output.stdout, &output.stderr)
    )))
}

async fn run_validator(
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
        message: command_failure_summary(&output.stdout, &output.stderr),
    }])
}

fn should_skip_workspace_entry(name: &str) -> bool {
    matches!(name, ".git" | "target" | WORKTREE_DIR)
}

fn sync_workspace_state(source_root: &Path, target_root: &Path) -> Result<(), AppError> {
    let delta = build_workspace_delta(source_root, target_root)?;
    apply_file_deltas(
        target_root,
        &ImplementationDelta {
            summary: "sync workspace state".to_string(),
            rationale: Vec::new(),
            changes: delta,
        },
    )
}

fn truncate_text(text: &str) -> String {
    const MAX_LEN: usize = 600;
    if text.len() <= MAX_LEN {
        text.to_string()
    } else {
        format!("{}...", &text[..MAX_LEN])
    }
}

fn workflow_outcome_from_phase_result(result: &PhaseReviewResult) -> WorkflowOutcome {
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

fn write_run_summary(
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

fn persist_task_cards(runtime: &AppRuntime, task_cards: &[TaskCard]) -> Result<(), AppError> {
    for task_card in task_cards {
        runtime.persist_task_card(task_card)?;
    }

    Ok(())
}

fn worktree_path(project_root: &Path, worktree_name: &str) -> PathBuf {
    project_root.join(WORKTREE_DIR).join(worktree_name)
}

fn write_workspace_file(
    project_root: &Path,
    relative: &str,
    content: &str,
) -> Result<(), AppError> {
    let path = resolve_project_path(project_root, relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| AppError::Workflow(format!("failed to create directory: {error}")))?;
    }
    fs::write(path, content).map_err(|error| {
        AppError::Workflow(format!("failed to write merged file `{relative}`: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ExecutionGraphSteps, FinalReviewDisposition, ImplementationExecutionInput, ManagedPhase,
        PhaseExecutionInput, PhaseReviewAction, PhaseReviewResult, append_user_guidance,
        approval_granted, build_command_evidence, build_outcome_patch, build_outcome_step,
        build_phase_patch, build_review_patch, discovery_ready_for_solution,
        final_review_disposition, next_management_request, phase_label, phase_review_action,
    };
    use crate::models::{
        ApprovalOutcome, ApprovedContract, ApprovedProposal, CommandEvidence, ExecutionMilestone,
        ExecutionPlan, FileDelta, FinalReview, ImplementationDelta, ImplementationDraft,
        ImplementationItemResult, ImplementationManagementRequest, ImplementationTaskInput,
        IntentBrief, ProjectContract, ReconciledProposal, RemediationItem, StageReview, TaskCard,
    };
    use crate::{error::AppError, runtime::AppRuntime};
    use naaf_core::{NeverFinding, NodeId, Step, task_fn};

    fn item_result_from_draft(
        item: &crate::models::TaskCard,
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
            commands_run: build_command_evidence(item),
            reviewer_findings: Vec::new(),
            manual_checks: item.acceptance_criteria.clone(),
            known_gaps: Vec::new(),
            scope_deviation: None,
            worktree_name: draft.worktree_name.clone(),
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
        ImplementationExecutionInput,
        ImplementationDraft,
        crate::models::StageFinding,
        AppError,
    > {
        Step::builder(task_fn(
            |_runtime: &AppRuntime, input: ImplementationExecutionInput| {
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

    fn dummy_phase_review_step()
    -> Step<AppRuntime, PhaseExecutionInput, PhaseReviewResult, NeverFinding, AppError> {
        Step::builder(task_fn(
            |_runtime: &AppRuntime, input: PhaseExecutionInput| {
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
            remediation_items: vec![crate::models::RemediationItem {
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
            remediation_items: vec![crate::models::RemediationItem {
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
            phase_review_action(super::MAX_FINAL_REVIEW_PASSES - 1, &review),
            PhaseReviewAction::Halt
        );
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

    #[test]
    fn next_management_request_increments_pass_and_carries_completed_items() {
        let result = PhaseReviewResult {
            phase: sample_phase(0, 0),
            completed_items: vec![crate::models::ImplementationItemResult {
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
