use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

use naaf_core::{
    Attempt, CheckExt, MaterialiserExt, NeverFinding, RepairPlannerExt, RetryPolicy, Step,
    StepError, Task, TaskExt, check_fn, materialiser_fn, repair_fn, task_fn,
};
use naaf_llm::{
    CompletionRequest, Executor, ExecutorConfig, LlmAgent, Message, OpenAiClient, OpenAiError,
    QuestionTool, RegisterToolError, Tool, ToolRegistry,
};
use serde::de::DeserializeOwned;
use tokio::process::Command;

use crate::{
    error::AppError,
    models::{
        ApprovalOutcome, ApprovedProposal, DiscoveryBrief, FinalReview, FinalReviewInput,
        ImplementationDelta, ImplementationDraft, ImplementationItemResult,
        ImplementationManagementRequest, ImplementationPlan, ImplementationTaskInput,
        ImplementationWorklist, ManagedItem, ProjectPrompt, ReconciledProposal, StageFinding,
        StageReview, ValidatedSolution, ValidationFinding, WorkflowOutcome,
    },
    parsing::decode_json_output,
    prompts::{
        approval_system_prompt, approval_user_prompt, architect_review_system_prompt,
        architect_review_user_prompt, discovery_system_prompt, discovery_user_prompt,
        final_review_system_prompt, final_review_user_prompt,
        implementation_management_system_prompt, implementation_management_user_prompt,
        implementation_task_system_prompt, implementation_task_user_prompt,
        peer_review_system_prompt, peer_review_user_prompt, planning_system_prompt,
        planning_user_prompt, reconcile_system_prompt, reconcile_user_prompt,
        solution_generation_system_prompt, solution_generation_user_prompt,
        solution_validation_system_prompt, solution_validation_user_prompt,
    },
    runtime::{
        AppGlobPathsTool, AppReadFileTool, AppRuntime, AppSearchFilesTool, AppWebSearchTool,
        WebSearchConfig,
    },
};

const DEFAULT_MODEL: &str = "gpt-4.1";
const EXECUTOR_TURNS: usize = 12;
const IMPLEMENTATION_RETRY_LIMIT: usize = 3;
const MAX_FINAL_REVIEW_PASSES: usize = 3;

type AppAgent = LlmAgent<OpenAiClient<AppRuntime>, AppRuntime, AppError>;
type LlmStageError = naaf_llm::AdapterError<AppError, OpenAiError, AppError, serde_json::Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinalReviewDisposition {
    Complete,
    Remediate,
    Halt,
}

pub(crate) async fn run_mmat(
    runtime: &AppRuntime,
    prompt: String,
) -> Result<WorkflowOutcome, AppError> {
    let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    let web_search = WebSearchConfig::from_env();
    let search_enabled = web_search.is_some();

    runtime.log_info(format!("Using model `{model}`."))?;
    if search_enabled {
        runtime.log_info("External web research is enabled for this run.")?;
    } else {
        runtime.log_warn(
            "External web research is disabled. Set MMAT_WEB_SEARCH_URL to enable the web_search tool.",
        )?;
    }

    let llm = build_agent(runtime.project_root(), web_search)?;
    let discovery_step = Step::builder(build_discovery_task(&llm, &model, search_enabled))
        .with_findings::<NeverFinding>()
        .build();
    let pragmatic = build_solution_branch(
        &llm,
        &model,
        crate::models::SolutionBranch::Pragmatic,
        search_enabled,
    );
    let best_practice = build_solution_branch(
        &llm,
        &model,
        crate::models::SolutionBranch::BestPractice,
        search_enabled,
    );
    let alternative = build_solution_branch(
        &llm,
        &model,
        crate::models::SolutionBranch::Alternative,
        search_enabled,
    );
    let contrarian = build_solution_branch(
        &llm,
        &model,
        crate::models::SolutionBranch::Contrarian,
        search_enabled,
    );
    let ambitious = build_solution_branch(
        &llm,
        &model,
        crate::models::SolutionBranch::Ambitious,
        search_enabled,
    );
    let solutions_workflow = pragmatic
        .join(best_practice)
        .reconcile_task(collect_solution_pair_task("collect_established_pair"))
        .join(
            alternative
                .join(contrarian)
                .reconcile_task(collect_solution_pair_task("collect_challenger_pair")),
        )
        .reconcile_task(merge_solution_lists_task("merge_solution_groups"))
        .join(ambitious)
        .reconcile_task(push_solution_task("add_ambitious_solution"));
    let reconcile_step = Step::builder(build_reconcile_task(&llm, &model))
        .with_findings::<NeverFinding>()
        .build();
    let approval_step = Step::builder(build_approval_task(&llm, &model))
        .with_findings::<NeverFinding>()
        .build();

    let discovery = discovery_step
        .run(runtime, ProjectPrompt { raw: prompt })
        .await
        .map_err(|error| AppError::Workflow(format!("discovery stage failed: {error}")))?;
    log_discovery_summary(runtime, &discovery)?;

    let solutions = solutions_workflow
        .run(runtime, discovery.clone())
        .await
        .map_err(|error| AppError::Workflow(format!("solution generation failed: {error}")))?;
    log_solution_summaries(runtime, &solutions)?;

    let reconciled = reconcile_step
        .run(runtime, solutions)
        .await
        .map_err(|error| AppError::Workflow(format!("reconcile stage failed: {error}")))?;
    log_reconciled_summary(runtime, &reconciled)?;

    runtime.log_info(
        "The next prompt will collect approval, revision notes, or any final constraints.",
    )?;

    let approval = approval_step
        .run(runtime, reconciled.clone())
        .await
        .map_err(|error| AppError::Workflow(format!("approval stage failed: {error}")))?;
    log_approval_summary(runtime, &approval)?;

    if !approval_granted(&approval) {
        return Ok(WorkflowOutcome {
            status: "awaiting_user_revision".to_string(),
            approval: approval.clone(),
            plan: None,
            architect_review: None,
            completed_items: Vec::new(),
            final_review: None,
            next_step: approval.next_step.clone(),
        });
    }

    let approved = ApprovedProposal {
        proposal: reconciled,
        approval: approval.clone(),
    };
    runtime.log_info("Approval granted. Starting planning and execution.")?;

    let planning_step = Step::builder(build_planning_task(&llm, &model, search_enabled))
        .with_findings::<NeverFinding>()
        .build();
    let architect_review_step = Step::builder(build_architect_review_task(&llm, &model))
        .with_findings::<NeverFinding>()
        .build();
    let management_step = Step::builder(build_implementation_management_task(
        &llm,
        &model,
        search_enabled,
    ))
    .with_findings::<NeverFinding>()
    .build();
    let final_review_step = Step::builder(build_final_review_task(&llm, &model, search_enabled))
        .with_findings::<NeverFinding>()
        .build();
    let implementation_step = build_implementation_step(runtime.project_root(), &model)?;

    let plan = planning_step
        .run(runtime, approved.clone())
        .await
        .map_err(|error| AppError::Workflow(format!("planning stage failed: {error}")))?;
    log_planning_summary(runtime, &plan)?;

    let architect_review = architect_review_step
        .run(runtime, plan.clone())
        .await
        .map_err(|error| AppError::Workflow(format!("architect review failed: {error}")))?;
    log_stage_review(runtime, "Architect review", &architect_review)?;

    let mut completed_items = Vec::new();
    let mut remediation_items = Vec::new();
    let mut last_review: Option<FinalReview> = None;

    for pass in 0..MAX_FINAL_REVIEW_PASSES {
        let phase = if pass == 0 {
            "initial_implementation".to_string()
        } else {
            format!("remediation_pass_{}", pass)
        };

        let management_request = ImplementationManagementRequest {
            phase,
            approved: approved.clone(),
            plan: plan.clone(),
            architect_review: architect_review.clone(),
            completed_items: completed_items.clone(),
            remediation_items: remediation_items.clone(),
        };

        let worklist = management_step
            .run(runtime, management_request)
            .await
            .map_err(|error| {
                AppError::Workflow(format!("implementation management failed: {error}"))
            })?;
        log_worklist_summary(runtime, &worklist)?;

        for item in worklist.items {
            runtime.log_info(format!("Implementing item `{}`.", item.title))?;
            let draft = implementation_step
                .run(
                    runtime,
                    ImplementationTaskInput {
                        approved: approved.clone(),
                        plan: plan.clone(),
                        work_item: item.clone(),
                        completed_items: completed_items.clone(),
                        prior_feedback: Vec::new(),
                    },
                )
                .await
                .map_err(|error| map_step_error("implementation subtask", error))?;
            let result = item_result_from_draft(&item, &draft);
            log_implementation_result(runtime, &result)?;
            completed_items.push(result);
        }

        let review = final_review_step
            .run(
                runtime,
                FinalReviewInput {
                    approved: approved.clone(),
                    plan: plan.clone(),
                    completed_items: completed_items.clone(),
                },
            )
            .await
            .map_err(|error| AppError::Workflow(format!("final review failed: {error}")))?;
        log_final_review_summary(runtime, &review)?;
        last_review = Some(review.clone());

        match final_review_disposition(&review) {
            FinalReviewDisposition::Complete => {
                return Ok(WorkflowOutcome {
                    status: "completed".to_string(),
                    approval,
                    plan: Some(plan),
                    architect_review: Some(architect_review),
                    completed_items,
                    final_review: Some(review.clone()),
                    next_step: review.next_step,
                });
            }
            FinalReviewDisposition::Remediate => {
                remediation_items = review.remediation_items.clone();
                runtime.log_warn(
                    "Final review requested remediation. Starting another implementation management pass.",
                )?;
            }
            FinalReviewDisposition::Halt => {
                runtime.log_warn(
                    "Final review is not yet ready but did not provide remediation items. Stopping the workflow.",
                )?;
                break;
            }
        }
    }

    let next_step = last_review
        .as_ref()
        .map(|review| review.next_step.clone())
        .unwrap_or_else(|| approval.next_step.clone());

    Ok(WorkflowOutcome {
        status: "needs_more_work".to_string(),
        approval,
        plan: Some(plan),
        architect_review: Some(architect_review),
        completed_items,
        final_review: last_review,
        next_step,
    })
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

    let client = OpenAiClient::from_env()?;
    let executor =
        Executor::with_tools(client, tools).with_config(ExecutorConfig::new(EXECUTOR_TURNS));
    Ok(LlmAgent::with_executor(executor))
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

fn build_discovery_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ProjectPrompt,
    Output = DiscoveryBrief,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = discovery_system_prompt(web_search_enabled);

    llm.task(
        move |_runtime: &AppRuntime, input: ProjectPrompt| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(discovery_user_prompt(&input.raw)),
                ],
            ))
        },
        decode_json_output::<DiscoveryBrief>,
    )
    .observed_as("discovery")
}

fn build_solution_branch(
    llm: &AppAgent,
    model: &str,
    branch: crate::models::SolutionBranch,
    web_search_enabled: bool,
) -> Step<AppRuntime, DiscoveryBrief, ValidatedSolution, NeverFinding, LlmStageError> {
    let generation_model = model.to_string();
    let validation_model = model.to_string();
    let generation_system = solution_generation_system_prompt(branch, web_search_enabled);
    let validation_system = solution_validation_system_prompt();
    let solution_name = format!("{}_solution", branch.slug());
    let validation_name = format!("{}_validation", branch.slug());

    let generate = Step::builder(
        llm.task(
            move |_runtime: &AppRuntime, discovery: DiscoveryBrief| {
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

fn build_approval_task(
    llm: &AppAgent,
    model: &str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ReconciledProposal,
    Output = ApprovalOutcome,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = approval_system_prompt();

    llm.task(
        move |_runtime: &AppRuntime, proposal: ReconciledProposal| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(approval_user_prompt(&proposal)?),
                ],
            ))
        },
        decode_json_output::<ApprovalOutcome>,
    )
    .observed_as("approval")
}

fn build_planning_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ApprovedProposal,
    Output = ImplementationPlan,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = planning_system_prompt(web_search_enabled);

    llm.task(
        move |_runtime: &AppRuntime, approved: ApprovedProposal| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(planning_user_prompt(&approved)?),
                ],
            ))
        },
        decode_json_output::<ImplementationPlan>,
    )
    .observed_as("planning")
}

fn build_architect_review_task(
    llm: &AppAgent,
    model: &str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ImplementationPlan,
    Output = StageReview,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = architect_review_system_prompt();

    llm.task(
        move |_runtime: &AppRuntime, plan: ImplementationPlan| {
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

fn build_implementation_management_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ImplementationManagementRequest,
    Output = ImplementationWorklist,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = implementation_management_system_prompt(web_search_enabled);

    llm.task(
        move |_runtime: &AppRuntime, request: ImplementationManagementRequest| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(implementation_management_user_prompt(&request)?),
                ],
            ))
        },
        decode_json_output::<ImplementationWorklist>,
    )
    .observed_as("implementation_management")
}

fn build_final_review_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<
    Runtime = AppRuntime,
    Input = FinalReviewInput,
    Output = FinalReview,
    Error = LlmStageError,
> + use<> {
    let model = model.to_string();
    let system_prompt = final_review_system_prompt(web_search_enabled);

    llm.task(
        move |_runtime: &AppRuntime, input: FinalReviewInput| {
            Ok::<_, AppError>(CompletionRequest::new(
                model.clone(),
                vec![
                    Message::system(system_prompt.clone()),
                    Message::user(final_review_user_prompt(&input)?),
                ],
            ))
        },
        decode_json_output::<FinalReview>,
    )
    .observed_as("final_review")
}

fn build_implementation_step(
    project_root: &Path,
    model: &str,
) -> Result<
    Step<AppRuntime, ImplementationTaskInput, ImplementationDraft, StageFinding, AppError>,
    AppError,
> {
    let llm_for_task = Rc::new(build_agent(project_root, None)?);
    let model_for_task = model.to_string();
    let task = task_fn(
        move |runtime: &AppRuntime, input: ImplementationTaskInput| {
            let llm = llm_for_task.clone();
            let model = model_for_task.clone();
            Box::pin(async move {
                let request = CompletionRequest::new(
                    model,
                    vec![
                        Message::system(implementation_task_system_prompt()),
                        Message::user(implementation_task_user_prompt(&input)?),
                    ],
                );
                let delta = execute_json_stage::<ImplementationDelta>(
                    llm.as_ref(),
                    runtime,
                    request,
                    "implementation task",
                )
                .await?;
                Ok::<_, AppError>(ImplementationDraft { input, delta })
            })
        },
    )
    .observed_as("implement_item");

    let apply_deltas = materialiser_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            apply_file_deltas(runtime.project_root(), &draft.delta)?;
            Ok::<_, AppError>(draft)
        })
    })
    .observed_as("apply_file_deltas");

    let cargo_fmt = materialiser_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            run_command(runtime.project_root(), "cargo fmt --all", &["fmt", "--all"]).await?;
            Ok::<_, AppError>(draft)
        })
    })
    .observed_as("cargo_fmt");

    let cargo_check = check_fn(|runtime: &AppRuntime, _draft: ImplementationDraft| {
        Box::pin(
            async move { run_validator(runtime.project_root(), "cargo check", &["check"]).await },
        )
    })
    .observed_as("cargo_check");

    let cargo_test = check_fn(|runtime: &AppRuntime, _draft: ImplementationDraft| {
        Box::pin(
            async move { run_validator(runtime.project_root(), "cargo test", &["test"]).await },
        )
    })
    .observed_as("cargo_test");

    let cargo_clippy = check_fn(|runtime: &AppRuntime, _draft: ImplementationDraft| {
        Box::pin(async move {
            run_validator(
                runtime.project_root(),
                "cargo clippy -- -D warnings",
                &["clippy", "--", "-D", "warnings"],
            )
            .await
        })
    })
    .observed_as("cargo_clippy");

    let llm_for_review = Rc::new(build_agent(project_root, None)?);
    let model_for_review = model.to_string();
    let peer_review = check_fn(move |runtime: &AppRuntime, draft: ImplementationDraft| {
        let llm = llm_for_review.clone();
        let model = model_for_review.clone();
        Box::pin(async move {
            let request = CompletionRequest::new(
                model,
                vec![
                    Message::system(peer_review_system_prompt()),
                    Message::user(peer_review_user_prompt(&draft.input, &draft.delta)?),
                ],
            );
            let review =
                execute_json_stage::<StageReview>(llm.as_ref(), runtime, request, "peer review")
                    .await?;
            Ok::<_, AppError>(review.findings)
        })
    })
    .observed_as("peer_review");

    let revise = repair_fn(
        |_runtime: &AppRuntime,
         attempts: Vec<Attempt<ImplementationTaskInput, ImplementationDraft, StageFinding>>| {
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
            next_input.prior_feedback = prior_feedback;
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
        .repair_with(revise)
        .retry_policy(RetryPolicy::new(IMPLEMENTATION_RETRY_LIMIT))
        .build())
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

fn merge_solution_lists_task(
    name: &'static str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = (Vec<ValidatedSolution>, Vec<ValidatedSolution>),
    Output = Vec<ValidatedSolution>,
    Error = LlmStageError,
> {
    task_fn(
        |_runtime: &AppRuntime, input: (Vec<ValidatedSolution>, Vec<ValidatedSolution>)| {
            Box::pin(async move {
                let mut merged = input.0;
                merged.extend(input.1);
                Ok::<_, LlmStageError>(merged)
            })
        },
    )
    .observed_as(name)
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

fn truncate_text(text: &str) -> String {
    const MAX_LEN: usize = 600;
    if text.len() <= MAX_LEN {
        text.to_string()
    } else {
        format!("{}...", &text[..MAX_LEN])
    }
}

fn approval_granted(approval: &ApprovalOutcome) -> bool {
    let decision = approval.decision.trim().to_ascii_lowercase();
    matches!(
        decision.as_str(),
        "approve" | "approved" | "accept" | "accepted"
    )
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

fn item_result_from_draft(
    item: &ManagedItem,
    draft: &ImplementationDraft,
) -> ImplementationItemResult {
    ImplementationItemResult {
        item_id: item.id.clone(),
        title: item.title.clone(),
        summary: draft.delta.summary.clone(),
        changed_files: draft
            .delta
            .changes
            .iter()
            .map(|change| change.path.clone())
            .collect(),
        rationale: draft.delta.rationale.clone(),
    }
}

fn map_step_error(stage_name: &str, error: StepError<StageFinding, AppError>) -> AppError {
    match error {
        StepError::System { stage, error } => {
            AppError::Workflow(format!("{stage_name} {stage} failed: {error}"))
        }
        StepError::Rejected(report) => AppError::Workflow(format!(
            "{stage_name} was rejected after {} attempt(s)",
            report.attempt_count()
        )),
    }
}

fn log_discovery_summary(runtime: &AppRuntime, discovery: &DiscoveryBrief) -> Result<(), AppError> {
    runtime.log_info("Discovery complete.")?;
    runtime.log_info(format!("Recommended path: {}", discovery.recommended_path))?;
    if !discovery.constraints.is_empty() {
        runtime.log_info(format!(
            "Constraints: {}",
            discovery.constraints.join(" | ")
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

fn log_planning_summary(runtime: &AppRuntime, plan: &ImplementationPlan) -> Result<(), AppError> {
    runtime.log_info("Planning complete.")?;
    runtime.log_info(format!("Plan summary: {}", plan.summary))?;
    runtime.log_info(format!("Milestones: {}", plan.milestones.len()))?;
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
        "Work items in this phase: {}",
        worklist.items.len()
    ))?;
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
    Ok(())
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

impl From<ValidationFinding> for StageFinding {
    fn from(value: ValidationFinding) -> Self {
        Self {
            severity: value.severity,
            category: value.category,
            message: value.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FinalReviewDisposition, approval_granted, final_review_disposition, item_result_from_draft,
    };
    use crate::models::{
        ApprovalOutcome, FileDelta, FinalReview, ImplementationDelta, ImplementationDraft,
        ImplementationTaskInput, ManagedItem,
    };

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
    fn item_result_from_draft_carries_changed_files_and_rationale() {
        let item = ManagedItem {
            id: "item-1".to_string(),
            source: "plan".to_string(),
            milestone_id: Some("m1".to_string()),
            title: "Add stage".to_string(),
            objective: "Implement the feature".to_string(),
            acceptance_criteria: vec!["works".to_string()],
            dependencies: Vec::new(),
        };
        let draft = ImplementationDraft {
            input: serde_json::from_value::<ImplementationTaskInput>(serde_json::json!({
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
                "plan": {
                    "summary": "plan",
                    "milestones": [],
                    "risks": []
                },
                "work_item": {
                    "id": "item-1",
                    "source": "plan",
                    "milestone_id": "m1",
                    "title": "Add stage",
                    "objective": "Implement the feature",
                    "acceptance_criteria": ["works"],
                    "dependencies": []
                },
                "completed_items": [],
                "prior_feedback": []
            }))
            .expect("implementation task input should parse"),
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
        assert_eq!(
            result.changed_files,
            vec!["src/workflow.rs", "src/prompts.rs"]
        );
        assert_eq!(result.rationale, vec!["kept it small"]);
    }
}
