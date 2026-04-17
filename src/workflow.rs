use std::env;

use naaf_core::{NeverFinding, Step, Task, TaskExt, task_fn};
use naaf_llm::{
    CompletionRequest, Executor, ExecutorConfig, LlmAgent, Message, OpenAiClient, OpenAiError,
    QuestionTool, RegisterToolError, Tool, ToolRegistry,
};

use crate::{
    error::AppError,
    models::{
        ApprovalOutcome, DiscoveryBrief, ProjectPrompt, ReconciledProposal, SolutionBranch,
        SolutionProposal, ValidatedSolution,
    },
    parsing::decode_json_output,
    prompts::{
        approval_system_prompt, approval_user_prompt, discovery_system_prompt,
        discovery_user_prompt, reconcile_system_prompt, reconcile_user_prompt,
        solution_generation_system_prompt, solution_generation_user_prompt,
        solution_validation_system_prompt, solution_validation_user_prompt,
    },
    runtime::{AppRuntime, AppWebSearchTool, WebSearchConfig},
};

const DEFAULT_MODEL: &str = "gpt-4.1";
const EXECUTOR_TURNS: usize = 12;

type AppAgent = LlmAgent<OpenAiClient<AppRuntime>, AppRuntime, AppError>;
type LlmStageError = naaf_llm::AdapterError<AppError, OpenAiError, AppError, serde_json::Error>;

pub(crate) async fn run_mmat(
    runtime: &AppRuntime,
    prompt: String,
) -> Result<ApprovalOutcome, AppError> {
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

    let llm = build_agent(web_search)?;
    let discovery_step = Step::builder(build_discovery_task(&llm, &model, search_enabled))
        .with_findings::<NeverFinding>()
        .build();
    let pragmatic = build_solution_branch(&llm, &model, SolutionBranch::Pragmatic, search_enabled);
    let best_practice =
        build_solution_branch(&llm, &model, SolutionBranch::BestPractice, search_enabled);
    let alternative =
        build_solution_branch(&llm, &model, SolutionBranch::Alternative, search_enabled);
    let contrarian =
        build_solution_branch(&llm, &model, SolutionBranch::Contrarian, search_enabled);
    let ambitious = build_solution_branch(&llm, &model, SolutionBranch::Ambitious, search_enabled);
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
        .run(runtime, reconciled)
        .await
        .map_err(|error| AppError::Workflow(format!("approval stage failed: {error}")))?;

    log_approval_summary(runtime, &approval)?;
    Ok(approval)
}

fn build_agent(web_search: Option<WebSearchConfig>) -> Result<AppAgent, AppError> {
    let mut tools: ToolRegistry<AppRuntime, AppError> = ToolRegistry::new();
    tools = register_tool(tools, QuestionTool::<AppRuntime>::new())?;
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
    branch: SolutionBranch,
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
            decode_json_output::<SolutionProposal>,
        )
        .observed_as(solution_name),
    )
    .with_findings::<NeverFinding>()
    .build();

    let validate = Step::builder(
        llm.task(
            move |_runtime: &AppRuntime, proposal: SolutionProposal| {
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
