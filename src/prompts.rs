use crate::{
    error::AppError,
    models::{
        ApprovalRequest, ApprovedContract, ContractApprovalRequest, ContractDraftInput,
        ExecutionPlan, FinalReviewInput, ImplementationDelta, ImplementationManagementRequest,
        ImplementationTaskInput, IntentBrief, ReleaseAssessmentInput, SolutionBranch,
        SolutionProposal, ValidatedSolution,
    },
    parsing::to_pretty_json,
};

pub(crate) fn approval_system_prompt() -> String {
    "You are the approval stage. The user has already reviewed the proposal and responded. Decide whether the response grants approval or requests revisions, capture any final constraints, and return raw JSON only with this shape: {\n  \"decision\": string,\n  \"summary\": string,\n  \"final_details\": string[],\n  \"next_step\": string\n}".to_string()
}

pub(crate) fn approval_user_prompt(request: &ApprovalRequest) -> Result<String, AppError> {
    Ok(format!(
        "Proposal under review:\n{}\n\nUser response:\n{}\n\nInterpret the user's response and return JSON only.",
        to_pretty_json(&request.proposal)?,
        request.user_response,
    ))
}

pub(crate) fn architect_review_system_prompt() -> String {
    "You are the planning validator acting as a senior software architect. Critique the execution plan for feasibility, sequencing, architecture, risk management, and missing work. Return raw JSON only with this shape: {\n  \"summary\": string,\n  \"findings\": [{\n    \"severity\": string,\n    \"category\": string,\n    \"message\": string\n  }]\n}. Use an empty findings array only when the plan is ready to execute.".to_string()
}

pub(crate) fn architect_review_user_prompt(plan: &ExecutionPlan) -> Result<String, AppError> {
    Ok(format!(
        "Review this execution plan as the senior architect and return JSON only:\n{}",
        to_pretty_json(plan)?,
    ))
}

pub(crate) fn contract_approval_system_prompt() -> String {
    "You are the contract approval stage. The user has reviewed the generated project contract and responded. Decide whether the response approves the contract or requests revisions, capture any final constraints, and return raw JSON only with this shape: {\n  \"decision\": string,\n  \"summary\": string,\n  \"final_details\": string[],\n  \"next_step\": string\n}".to_string()
}

pub(crate) fn contract_approval_user_prompt(
    request: &ContractApprovalRequest,
) -> Result<String, AppError> {
    Ok(format!(
        "Project contract under review:\n{}\n\nUser response:\n{}\n\nInterpret the user's response and return JSON only.",
        to_pretty_json(&request.contract)?,
        request.user_response,
    ))
}

pub(crate) fn contract_system_prompt(web_search_enabled: bool) -> String {
    format!(
        "You are the contract formation stage. Convert the approved direction into an explicit project contract that later stages must follow. Preserve the approved intent, make non-goals and exclusions explicit, and define acceptance criteria and definition of done in concrete terms. {} Return raw JSON only with this shape: {{\n  \"problem_statement\": string,\n  \"user_goals\": string[],\n  \"non_goals\": string[],\n  \"assumptions\": string[],\n  \"constraints\": string[],\n  \"acceptance_criteria\": string[],\n  \"definition_of_done\": string[],\n  \"approved_tech_choices\": string[],\n  \"explicit_exclusions\": string[],\n  \"demo_scenarios\": string[]\n}}",
        tool_guidance(web_search_enabled, false)
    )
}

pub(crate) fn contract_user_prompt(input: &ContractDraftInput) -> Result<String, AppError> {
    Ok(format!(
        "Intent brief and approved proposal:\n{}\n\nWrite the project contract now. Return JSON only.",
        to_pretty_json(input)?,
    ))
}

pub(crate) fn discovery_system_prompt(web_search_enabled: bool) -> String {
    format!(
        "You are the intent stage in a NAAF workflow for complex, unstructured work. Turn the user's prompt into a best-guess intent brief that preserves momentum while making uncertainty explicit. Ask only the highest-value clarification questions. When details are missing, record explicit default assumptions so downstream stages can still proceed. Set `ready_for_solution` to true when the brief is specific enough for solution design or when the remaining ambiguity can be handled safely through recorded defaults. {} Return raw JSON only with this shape: {{\n  \"ready_for_solution\": boolean,\n  \"problem_statement\": string,\n  \"user_goals\": string[],\n  \"non_goals\": string[],\n  \"assumptions\": string[],\n  \"default_assumptions\": string[],\n  \"constraints\": string[],\n  \"ambiguities\": string[],\n  \"risks\": string[],\n  \"acceptance_criteria\": string[],\n  \"clarification_summary\": string[],\n  \"research_notes\": string[],\n  \"recommended_path\": string,\n  \"clarification_questions\": string[]\n}}",
        tool_guidance(web_search_enabled, true)
    )
}

pub(crate) fn discovery_user_prompt(
    prompt: &str,
    clarification_attempt: usize,
    clarification_limit: usize,
) -> String {
    format!(
        "User prompt:\n{prompt}\n\nThis is clarification attempt {} of {}. Produce the best possible intent brief from the information available, do any useful research available to you, and finish with JSON only. If ambiguity remains, prioritise explicit defaults and the most important clarification questions.",
        clarification_attempt + 1,
        clarification_limit + 1,
    )
}

pub(crate) fn final_review_system_prompt(web_search_enabled: bool) -> String {
    [
        "You are the final integration review stage. Assess the finished solution for completion, adherence to the approved specification, code quality, code structure, and any remaining risks. ".to_string(),
        tool_guidance(web_search_enabled, false),
        " Return raw JSON only with this shape: {\n  \"summary\": string,\n  \"ready\": boolean,\n  \"strengths\": string[],\n  \"findings\": [{\n    \"severity\": string,\n    \"category\": string,\n    \"message\": string\n  }],\n  \"remediation_items\": [{\n    \"id\": string,\n    \"title\": string,\n    \"description\": string,\n    \"acceptance_criteria\": string[],\n    \"related_item_ids\": string[]\n  }],\n  \"next_step\": string\n}. When `ready` is true, return an empty remediation_items array.".to_string(),
    ]
    .concat()
}

pub(crate) fn final_review_user_prompt(input: &FinalReviewInput) -> Result<String, AppError> {
    Ok(format!(
        "Final review context:\n{}\n\nReview the finished solution now and return JSON only.",
        to_pretty_json(input)?,
    ))
}

pub(crate) fn release_assessment_system_prompt() -> String {
    "You are the adversarial release judge. Your job is to break the illusion of done. Compare the completed work against the frozen project contract and the evidence log. Identify what was claimed but not proven, what remains incomplete, and whether the result is actually releasable. Be adversarial, not agreeable. Return raw JSON only with this shape: {\n  \"contract_items_satisfied\": string[],\n  \"contract_items_incomplete\": string[],\n  \"claimed_but_not_proven\": string[],\n  \"known_gaps\": string[],\n  \"residual_risks\": string[],\n  \"releasable\": boolean,\n  \"summary\": string\n}".to_string()
}

pub(crate) fn release_assessment_user_prompt(
    input: &ReleaseAssessmentInput,
) -> Result<String, AppError> {
    Ok(format!(
        "Release judgment context:\n{}\n\nRender your verdict now. Return JSON only.",
        to_pretty_json(input)?,
    ))
}

pub(crate) fn implementation_management_system_prompt(web_search_enabled: bool) -> String {
    [
        "You are the implementation management stage. Turn the approved plan or remediation backlog into an executable worklist. Preserve ids, respect dependencies, and schedule only the items that should be acted on in this phase. ".to_string(),
        tool_guidance(web_search_enabled, false),
        format!(
            " Return raw JSON only with this shape: {{\n  \"summary\": string,\n  \"items\": [{{\n{}\n  }}]\n}}",
            task_card_schema_lines(4)
        ),
    ]
    .concat()
}

pub(crate) fn implementation_management_user_prompt(
    request: &ImplementationManagementRequest,
) -> Result<String, AppError> {
    Ok(format!(
        "Execution context:\n{}\n\nProduce the worklist for this phase now. Return JSON only.",
        to_pretty_json(request)?,
    ))
}

pub(crate) fn implementation_task_system_prompt() -> String {
    "You are a software development subtask. Implement exactly the requested item by returning file deltas only. Inspect the repository before proposing changes. Keep the solution minimal, coherent with the existing codebase, and scoped to the requested item. Return raw JSON only with this shape: {\n  \"summary\": string,\n  \"rationale\": string[],\n  \"changes\": [{\n    \"path\": string,\n    \"action\": string,\n    \"content\": string | null\n  }]\n}. Allowed actions are `write` and `delete`. For `write`, include the complete new file content. For `delete`, set content to null.".to_string()
}

pub(crate) fn implementation_task_user_prompt(
    input: &ImplementationTaskInput,
) -> Result<String, AppError> {
    Ok(format!(
        "Implementation context:\n{}\n\nImplement only this work item. If previous validator feedback exists, address it directly. Return JSON only.",
        to_pretty_json(input)?,
    ))
}

pub(crate) fn peer_review_system_prompt() -> String {
    "You are the peer review validator for one implementation subtask. Review the proposed file deltas for correctness, specification adherence, code quality, and code structure. Return raw JSON only with this shape: {\n  \"summary\": string,\n  \"findings\": [{\n    \"severity\": string,\n    \"category\": string,\n    \"message\": string\n  }]\n}. Use an empty findings array only when the subtask is ready to materialise.".to_string()
}

pub(crate) fn peer_review_user_prompt(
    input: &ImplementationTaskInput,
    delta: &ImplementationDelta,
) -> Result<String, AppError> {
    Ok(format!(
        "Review this implementation proposal and return JSON only:\n{}\n\nDelta:\n{}",
        to_pretty_json(input)?,
        to_pretty_json(delta)?,
    ))
}

pub(crate) fn contract_validation_system_prompt() -> String {
    "You are the contract validation validator for one implementation subtask. Check whether the proposed changes actually satisfy the task card's acceptance criteria and contract references, not just compile. Flag any stubs, placeholders, TODO comments, mock implementations, or incomplete behaviour. Return raw JSON only with this shape: {\n  \"summary\": string,\n  \"findings\": [{\n    \"severity\": string,\n    \"category\": string,\n    \"message\": string\n  }]\n}. Use an empty findings array only when the implementation genuinely satisfies the contract.".to_string()
}

pub(crate) fn contract_validation_user_prompt(
    input: &ImplementationTaskInput,
    delta: &ImplementationDelta,
) -> Result<String, AppError> {
    Ok(format!(
        "Validate this implementation against the task card contract and return JSON only:\nTask card context:\n{}\n\nProposed delta:\n{}",
        to_pretty_json(input)?,
        to_pretty_json(delta)?,
    ))
}

pub(crate) fn planning_system_prompt(web_search_enabled: bool) -> String {
    [
        "You are the planning stage that runs after the user has approved the project contract. Produce an execution plan with concrete milestones and execution-ready task cards. Every task card must be specific enough for implementation without inventing missing details later. ".to_string(),
        tool_guidance(web_search_enabled, false),
        format!(
            " Return raw JSON only with this shape: {{\n  \"summary\": string,\n  \"milestones\": [{{\n    \"id\": string,\n    \"title\": string,\n    \"objective\": string,\n    \"task_card_ids\": string[]\n  }}],\n  \"task_cards\": [{{\n{}\n  }}],\n  \"risks\": string[]\n}}",
            task_card_schema_lines(4)
        ),
    ]
    .concat()
}

pub(crate) fn planning_user_prompt(approved: &ApprovedContract) -> Result<String, AppError> {
    Ok(format!(
        "Approved project contract and context:\n{}\n\nDesign the execution plan now. Each milestone and task card must have a stable id. Return JSON only.",
        to_pretty_json(approved)?,
    ))
}

fn task_card_schema_lines(indent: usize) -> String {
    let prefix = " ".repeat(indent);
    let fields = [
        "\"id\": string,",
        "\"source\": string,",
        "\"milestone_id\": string | null,",
        "\"title\": string,",
        "\"objective\": string,",
        "\"contract_refs\": string[],",
        "\"acceptance_criteria\": string[],",
        "\"expected_files\": string[],",
        "\"verification_commands\": string[],",
        "\"dependencies\": string[],",
        "\"rollback_notes\": string[]",
    ];

    fields
        .into_iter()
        .map(|field| format!("{prefix}{field}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn reconcile_system_prompt() -> String {
    "You are the reconcile stage. Combine the strongest ideas from the candidate solutions into one proposal for the user. Balance conservative delivery, the recommended professional default, and ambitious upside. Return raw JSON only with this shape: {\n  \"title\": string,\n  \"executive_summary\": string,\n  \"recommended_direction\": string,\n  \"why_this_plan\": string,\n  \"adopted_ideas\": [{\n    \"source_branch\": string,\n    \"idea\": string,\n    \"reason\": string\n  }],\n  \"deferred_ideas\": [{\n    \"source_branch\": string,\n    \"idea\": string,\n    \"reason\": string\n  }],\n  \"scope\": string,\n  \"architecture\": string[],\n  \"delivery_plan\": string[],\n  \"technologies\": string[],\n  \"major_risks\": string[],\n  \"open_questions\": string[]\n}".to_string()
}

pub(crate) fn reconcile_user_prompt(solutions: &[ValidatedSolution]) -> Result<String, AppError> {
    Ok(format!(
        "Candidate validated solutions:\n{}\n\nReconcile these into the best overall proposal. Return JSON only.",
        to_pretty_json(solutions)?,
    ))
}

pub(crate) fn solution_generation_system_prompt(
    branch: SolutionBranch,
    web_search_enabled: bool,
) -> String {
    format!(
        "You are generating the `{}` branch of a workflow. {} {} Return raw JSON only with this shape: {{\n  \"branch\": string,\n  \"title\": string,\n  \"summary\": string,\n  \"scope\": string,\n  \"architecture\": string[],\n  \"delivery_plan\": string[],\n  \"technologies\": string[],\n  \"research_notes\": string[],\n  \"why_this_path\": string,\n  \"risks\": string[]\n}}",
        branch.slug(),
        branch.instruction(),
        tool_guidance(web_search_enabled, false),
    )
}

pub(crate) fn solution_generation_user_prompt(
    branch: SolutionBranch,
    discovery: &IntentBrief,
) -> Result<String, AppError> {
    Ok(format!(
        "Intent brief:\n{}\n\nProduce the `{}` solution branch now. Ensure the proposal is distinct, specific, and internally coherent. Return JSON only.",
        to_pretty_json(discovery)?,
        branch.slug(),
    ))
}

pub(crate) fn solution_validation_system_prompt() -> String {
    "You are the validation stage for a proposed solution. Assess feasibility, correctness, and delivery risk. Keep the proposal intact while adding a validation judgement. Return raw JSON only with this shape: {\n  \"branch\": string,\n  \"proposal\": {\n    \"branch\": string,\n    \"title\": string,\n    \"summary\": string,\n    \"scope\": string,\n    \"architecture\": string[],\n    \"delivery_plan\": string[],\n    \"technologies\": string[],\n    \"research_notes\": string[],\n    \"why_this_path\": string,\n    \"risks\": string[]\n  },\n  \"assessment_summary\": string,\n  \"feasibility\": string,\n  \"correctness\": string,\n  \"delivery_risk\": string,\n  \"recommendation\": string,\n  \"findings\": [{\n    \"severity\": string,\n    \"category\": string,\n    \"message\": string\n  }],\n  \"open_questions\": string[]\n}".to_string()
}

pub(crate) fn solution_validation_user_prompt(
    proposal: &SolutionProposal,
) -> Result<String, AppError> {
    Ok(format!(
        "Validate this proposal and return JSON only:\n{}",
        to_pretty_json(proposal)?,
    ))
}

fn tool_guidance(web_search_enabled: bool, ask_user_allowed: bool) -> String {
    let mut guidance = vec![
        "You may use `read_file`, `glob_paths`, and `search_files` to inspect the repository before answering.".to_string(),
    ];

    if web_search_enabled {
        guidance.push(
            "You may use `web_search` when current external references materially improve the answer."
                .to_string(),
        );
    }

    if ask_user_allowed {
        guidance.push(
            "Use `ask_user` only when a blocking ambiguity remains and the repository does not resolve it."
                .to_string(),
        );
    } else {
        guidance.push(
            "Do not ask the user follow-up questions unless the workflow explicitly instructs you to do so."
                .to_string(),
        );
    }

    guidance.join(" ")
}
