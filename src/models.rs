use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ProjectPrompt {
    pub(crate) raw: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DiscoveryBrief {
    pub(crate) problem_statement: String,
    pub(crate) desired_outcomes: Vec<String>,
    pub(crate) assumptions: Vec<String>,
    pub(crate) constraints: Vec<String>,
    pub(crate) clarification_summary: Vec<String>,
    pub(crate) research_notes: Vec<String>,
    pub(crate) recommended_path: String,
    pub(crate) open_questions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SolutionProposal {
    pub(crate) branch: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) scope: String,
    pub(crate) architecture: Vec<String>,
    pub(crate) delivery_plan: Vec<String>,
    pub(crate) technologies: Vec<String>,
    pub(crate) research_notes: Vec<String>,
    pub(crate) why_this_path: String,
    pub(crate) risks: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ValidationFinding {
    pub(crate) severity: String,
    pub(crate) category: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ValidatedSolution {
    pub(crate) branch: String,
    pub(crate) proposal: SolutionProposal,
    pub(crate) assessment_summary: String,
    pub(crate) feasibility: String,
    pub(crate) correctness: String,
    pub(crate) delivery_risk: String,
    pub(crate) recommendation: String,
    pub(crate) findings: Vec<ValidationFinding>,
    pub(crate) open_questions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ReconciledIdea {
    pub(crate) source_branch: String,
    pub(crate) idea: String,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ReconciledProposal {
    pub(crate) title: String,
    pub(crate) executive_summary: String,
    pub(crate) recommended_direction: String,
    pub(crate) why_this_plan: String,
    pub(crate) adopted_ideas: Vec<ReconciledIdea>,
    pub(crate) deferred_ideas: Vec<ReconciledIdea>,
    pub(crate) scope: String,
    pub(crate) architecture: Vec<String>,
    pub(crate) delivery_plan: Vec<String>,
    pub(crate) technologies: Vec<String>,
    pub(crate) major_risks: Vec<String>,
    pub(crate) open_questions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ApprovalOutcome {
    pub(crate) decision: String,
    pub(crate) summary: String,
    pub(crate) final_details: Vec<String>,
    pub(crate) next_step: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SolutionBranch {
    Pragmatic,
    BestPractice,
    Alternative,
    Contrarian,
    Ambitious,
}

impl SolutionBranch {
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Self::Pragmatic => "pragmatic",
            Self::BestPractice => "best_practice",
            Self::Alternative => "alternative",
            Self::Contrarian => "contrarian",
            Self::Ambitious => "ambitious",
        }
    }

    pub(crate) fn instruction(self) -> &'static str {
        match self {
            Self::Pragmatic => {
                "Optimise for the fastest credible delivery path with the lowest operational and implementation risk. Prefer boring technology and the smallest viable scope that still solves the problem well."
            }
            Self::BestPractice => {
                "Optimise for strong engineering discipline, maintainability, clarity, and long-term operability. Recommend the professional default even if it is not the quickest route."
            }
            Self::Alternative => {
                "Produce a materially different solution shape from the default path. Seek a different architecture, delivery model, or product framing while still aiming for viability."
            }
            Self::Contrarian => {
                "Challenge whether the requested solution is necessary at all. Consider simplification, process change, deferral, or not building anything new. If a build is still warranted, make the case reluctantly and clearly."
            }
            Self::Ambitious => {
                "Optimise for upside, leverage, and strategic advantage. You may expand scope, propose an enabling platform first, and recommend bleeding-edge technology when it materially improves the design. Extra ambition must still be justified against delivery risk."
            }
        }
    }
}
