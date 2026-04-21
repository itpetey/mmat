use naaf_core::{NeverFinding, Step, Task, TaskExt, task_fn};
use naaf_llm::{CompletionRequest, Message};

use super::{AppError, AppRuntime, LlmStageError, steps::AppAgent};
use crate::{
    models::{
        ApprovalOutcome, ApprovalRequest, ApprovedContract, ContractApprovalRequest,
        ContractDraftInput, ExecutionPlan, IntentBrief, KnowledgeArtifact, ProjectContract,
        ProjectPrompt, ReconciledProposal, ValidatedSolution,
    },
    parsing::decode_json_output,
    prompts::{
        approval_system_prompt, approval_user_prompt, architect_review_system_prompt,
        architect_review_user_prompt, contract_approval_system_prompt,
        contract_approval_user_prompt, contract_system_prompt, contract_user_prompt,
        discovery_system_prompt, discovery_user_prompt, knowledge_compilation_system_prompt,
        knowledge_compilation_user_prompt, planning_system_prompt, planning_user_prompt,
        reconcile_system_prompt, reconcile_user_prompt, solution_generation_system_prompt,
        solution_generation_user_prompt, solution_validation_system_prompt,
        solution_validation_user_prompt,
    },
};

pub fn build_approval_task(
    llm: &AppAgent,
    model: &str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ApprovalRequest,
    Output = ApprovalOutcome,
    Error = LlmStageError,
> + use<> {
    llm.json_task(
        model.to_string(),
        approval_system_prompt(),
        |request: ApprovalRequest| approval_user_prompt(&request),
        decode_json_output::<ApprovalOutcome>,
        "approval".to_string(),
    )
}

pub fn build_architect_review_task(
    llm: &AppAgent,
    model: &str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ExecutionPlan,
    Output = crate::models::StageReview,
    Error = LlmStageError,
> + use<> {
    llm.json_task(
        model.to_string(),
        architect_review_system_prompt(),
        |plan: ExecutionPlan| architect_review_user_prompt(&plan),
        decode_json_output::<crate::models::StageReview>,
        "architect_review".to_string(),
    )
}

pub fn build_contract_approval_task(
    llm: &AppAgent,
    model: &str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ContractApprovalRequest,
    Output = ApprovalOutcome,
    Error = LlmStageError,
> + use<> {
    llm.json_task(
        model.to_string(),
        contract_approval_system_prompt(),
        |request: ContractApprovalRequest| contract_approval_user_prompt(&request),
        decode_json_output::<ApprovalOutcome>,
        "contract_approval".to_string(),
    )
}

pub fn build_contract_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ContractDraftInput,
    Output = ProjectContract,
    Error = LlmStageError,
> + use<> {
    let system_prompt = contract_system_prompt(web_search_enabled);
    llm.json_task(
        model.to_string(),
        system_prompt,
        |input: ContractDraftInput| contract_user_prompt(&input),
        decode_json_output::<ProjectContract>,
        "project_contract".to_string(),
    )
}

pub fn build_discovery_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<Runtime = AppRuntime, Input = ProjectPrompt, Output = IntentBrief, Error = LlmStageError>
+ use<> {
    let system_prompt = discovery_system_prompt(web_search_enabled);
    llm.json_task(
        model.to_string(),
        system_prompt,
        |input: ProjectPrompt| {
            Ok(discovery_user_prompt(
                &input.raw,
                input.clarification_attempt,
                input.clarification_limit,
            ))
        },
        decode_json_output::<IntentBrief>,
        "discovery".to_string(),
    )
}

pub fn build_planning_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<
    Runtime = AppRuntime,
    Input = ApprovedContract,
    Output = ExecutionPlan,
    Error = LlmStageError,
> + use<> {
    let system_prompt = planning_system_prompt(web_search_enabled);
    llm.json_task(
        model.to_string(),
        system_prompt,
        |approved: ApprovedContract| planning_user_prompt(&approved),
        decode_json_output::<ExecutionPlan>,
        "planning".to_string(),
    )
}

pub fn build_knowledge_compilation_task(
    llm: &AppAgent,
    model: &str,
    web_search_enabled: bool,
) -> impl Task<
    Runtime = AppRuntime,
    Input = IntentBrief,
    Output = KnowledgeArtifact,
    Error = LlmStageError,
> + use<> {
    let system_prompt = knowledge_compilation_system_prompt(web_search_enabled);
    llm.json_task(
        model.to_string(),
        system_prompt,
        |intent: IntentBrief| knowledge_compilation_user_prompt(&intent),
        decode_json_output::<KnowledgeArtifact>,
        "knowledge_compilation".to_string(),
    )
}

pub fn build_reconcile_task(
    llm: &AppAgent,
    model: &str,
) -> impl Task<
    Runtime = AppRuntime,
    Input = Vec<ValidatedSolution>,
    Output = ReconciledProposal,
    Error = LlmStageError,
> + use<> {
    llm.json_task(
        model.to_string(),
        reconcile_system_prompt(),
        |solutions: Vec<ValidatedSolution>| reconcile_user_prompt(&solutions),
        decode_json_output::<ReconciledProposal>,
        "reconcile".to_string(),
    )
}

pub fn build_solution_branch(
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

pub fn collect_solution_pair_task(
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

pub fn push_solution_task(
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
