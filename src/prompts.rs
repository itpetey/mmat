use crate::{
    error::AppError,
    models::{
        DiscoveryBrief, ReconciledProposal, SolutionBranch, SolutionProposal, ValidatedSolution,
    },
    parsing::to_pretty_json,
};

pub(crate) fn discovery_system_prompt(web_search_enabled: bool) -> String {
    let tool_guidance = if web_search_enabled {
        "You may use `web_search` for lightweight prior-art and best-practice research, and `ask_user` whenever missing information materially affects the recommendation."
    } else {
        "The `web_search` tool is not configured for this run. Use `ask_user` whenever missing information materially affects the recommendation."
    };

    format!(
        "You are the discovery stage in a NAAF workflow for complex, unstructured work. Assess the user's prompt, gather missing information, and recommend the best path forward. {tool_guidance} Return raw JSON only with this shape: {{\n  \"problem_statement\": string,\n  \"desired_outcomes\": string[],\n  \"assumptions\": string[],\n  \"constraints\": string[],\n  \"clarification_summary\": string[],\n  \"research_notes\": string[],\n  \"recommended_path\": string,\n  \"open_questions\": string[]\n}}"
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
    let tool_guidance = if web_search_enabled {
        "Use `web_search` when prior art, standards, or current tooling would improve the proposal. Use `ask_user` only if a blocking ambiguity remains after discovery."
    } else {
        "`web_search` is unavailable for this run. Use `ask_user` only if a blocking ambiguity remains after discovery."
    };

    format!(
        "You are generating the `{}` branch of a workflow. {} {} Return raw JSON only with this shape: {{\n  \"branch\": string,\n  \"title\": string,\n  \"summary\": string,\n  \"scope\": string,\n  \"architecture\": string[],\n  \"delivery_plan\": string[],\n  \"technologies\": string[],\n  \"research_notes\": string[],\n  \"why_this_path\": string,\n  \"risks\": string[]\n}}",
        branch.slug(),
        branch.instruction(),
        tool_guidance,
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
