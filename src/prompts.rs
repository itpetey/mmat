use crate::{
    error::AppError,
    models::{
        ApprovedProposal, DiscoveryBrief, FinalReviewInput, ImplementationDelta,
        ImplementationManagementRequest, ImplementationPlan, ImplementationTaskInput,
        ReconciledProposal, SolutionBranch, SolutionProposal, ValidatedSolution,
    },
    parsing::to_pretty_json,
};

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

pub(crate) fn discovery_system_prompt(web_search_enabled: bool) -> String {
    format!(
        "You are the discovery stage in a NAAF workflow for complex, unstructured work. Assess the user's prompt, gather missing information, and recommend the best path forward. {} Return raw JSON only with this shape: {{\n  \"problem_statement\": string,\n  \"desired_outcomes\": string[],\n  \"assumptions\": string[],\n  \"constraints\": string[],\n  \"clarification_summary\": string[],\n  \"research_notes\": string[],\n  \"recommended_path\": string,\n  \"open_questions\": string[]\n}}",
        tool_guidance(web_search_enabled, true)
    )
}

pub(crate) fn discovery_user_prompt(prompt: &str) -> String {
    format!(
        "User prompt:\n{prompt}\n\nClarify only when needed, do any useful research available to you, and finish with JSON only."
    )
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
    discovery: &DiscoveryBrief,
) -> Result<String, AppError> {
    Ok(format!(
        "Discovery brief:\n{}\n\nProduce the `{}` solution branch now. Ensure the proposal is distinct, specific, and internally coherent. Return JSON only.",
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

pub(crate) fn reconcile_system_prompt() -> String {
    "You are the reconcile stage. Combine the strongest ideas from the candidate solutions into one proposal for the user. Balance short-term pragmatism, sound engineering practice, alternative thinking, contrarian pressure, and ambitious upside. Return raw JSON only with this shape: {\n  \"title\": string,\n  \"executive_summary\": string,\n  \"recommended_direction\": string,\n  \"why_this_plan\": string,\n  \"adopted_ideas\": [{\n    \"source_branch\": string,\n    \"idea\": string,\n    \"reason\": string\n  }],\n  \"deferred_ideas\": [{\n    \"source_branch\": string,\n    \"idea\": string,\n    \"reason\": string\n  }],\n  \"scope\": string,\n  \"architecture\": string[],\n  \"delivery_plan\": string[],\n  \"technologies\": string[],\n  \"major_risks\": string[],\n  \"open_questions\": string[]\n}".to_string()
}

pub(crate) fn reconcile_user_prompt(solutions: &[ValidatedSolution]) -> Result<String, AppError> {
    Ok(format!(
        "Candidate validated solutions:\n{}\n\nReconcile these into the best overall proposal. Return JSON only.",
        to_pretty_json(solutions)?,
    ))
}

pub(crate) fn approval_system_prompt() -> String {
    "You are the approval stage. First make sure the user has a fair chance to approve, request revisions, or add constraints by using `ask_user`. You may ask follow-up questions if the first answer is ambiguous. Then return raw JSON only with this shape: {\n  \"decision\": string,\n  \"summary\": string,\n  \"final_details\": string[],\n  \"next_step\": string\n}".to_string()
}

pub(crate) fn approval_user_prompt(proposal: &ReconciledProposal) -> Result<String, AppError> {
    Ok(format!(
        "Present this reconciled proposal back to the user, collect approval or revision notes, and summarise the result as JSON only:\n{}",
        to_pretty_json(proposal)?,
    ))
}

pub(crate) fn planning_system_prompt(web_search_enabled: bool) -> String {
    [
        "You are the planning stage that runs after the user has approved the reconciled proposal. Produce an implementation plan with concrete milestones and specific items within each milestone. ".to_string(),
        tool_guidance(web_search_enabled, false),
        " Return raw JSON only with this shape: {\n  \"summary\": string,\n  \"milestones\": [{\n    \"id\": string,\n    \"title\": string,\n    \"objective\": string,\n    \"items\": [{\n      \"id\": string,\n      \"title\": string,\n      \"description\": string,\n      \"acceptance_criteria\": string[],\n      \"dependencies\": string[]\n    }]\n  }],\n  \"risks\": string[]\n}".to_string(),
    ]
    .concat()
}

pub(crate) fn planning_user_prompt(approved: &ApprovedProposal) -> Result<String, AppError> {
    Ok(format!(
        "Approved proposal and constraints:\n{}\n\nDesign the implementation plan now. Each milestone and item must have a stable id. Return JSON only.",
        to_pretty_json(approved)?,
    ))
}

pub(crate) fn architect_review_system_prompt() -> String {
    "You are the planning validator acting as a senior software architect. Critique the implementation plan for feasibility, sequencing, architecture, risk management, and missing work. Return raw JSON only with this shape: {\n  \"summary\": string,\n  \"findings\": [{\n    \"severity\": string,\n    \"category\": string,\n    \"message\": string\n  }]\n}. Use an empty findings array only when the plan is ready to execute.".to_string()
}

pub(crate) fn architect_review_user_prompt(plan: &ImplementationPlan) -> Result<String, AppError> {
    Ok(format!(
        "Review this implementation plan as the senior architect and return JSON only:\n{}",
        to_pretty_json(plan)?,
    ))
}

pub(crate) fn implementation_management_system_prompt(web_search_enabled: bool) -> String {
    [
        "You are the implementation management stage. Turn the approved plan or remediation backlog into an executable worklist. Preserve ids, respect dependencies, and schedule only the items that should be acted on in this phase. ".to_string(),
        tool_guidance(web_search_enabled, false),
        " Return raw JSON only with this shape: {\n  \"summary\": string,\n  \"items\": [{\n    \"id\": string,\n    \"source\": string,\n    \"milestone_id\": string | null,\n    \"title\": string,\n    \"objective\": string,\n    \"acceptance_criteria\": string[],\n    \"dependencies\": string[]\n  }]\n}".to_string(),
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
