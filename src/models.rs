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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ApprovedProposal {
    pub(crate) proposal: ReconciledProposal,
    pub(crate) approval: ApprovalOutcome,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StageFinding {
    pub(crate) severity: String,
    pub(crate) category: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StageReview {
    pub(crate) summary: String,
    pub(crate) findings: Vec<StageFinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationPlan {
    pub(crate) summary: String,
    pub(crate) milestones: Vec<PlanMilestone>,
    pub(crate) risks: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PlanMilestone {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) objective: String,
    pub(crate) items: Vec<PlanItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PlanItem {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) dependencies: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationManagementRequest {
    pub(crate) phase: String,
    pub(crate) approved: ApprovedProposal,
    pub(crate) plan: ImplementationPlan,
    pub(crate) architect_review: StageReview,
    pub(crate) completed_items: Vec<ImplementationItemResult>,
    pub(crate) remediation_items: Vec<RemediationItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationWorklist {
    pub(crate) summary: String,
    pub(crate) items: Vec<ManagedItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ManagedItem {
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) milestone_id: Option<String>,
    pub(crate) title: String,
    pub(crate) objective: String,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) dependencies: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RemediationItem {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) related_item_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationTaskInput {
    pub(crate) approved: ApprovedProposal,
    pub(crate) plan: ImplementationPlan,
    pub(crate) work_item: ManagedItem,
    pub(crate) completed_items: Vec<ImplementationItemResult>,
    pub(crate) prior_feedback: Vec<StageFinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationDelta {
    pub(crate) summary: String,
    pub(crate) rationale: Vec<String>,
    pub(crate) changes: Vec<FileDelta>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationDraft {
    pub(crate) input: ImplementationTaskInput,
    pub(crate) delta: ImplementationDelta,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FileDelta {
    pub(crate) path: String,
    pub(crate) action: String,
    pub(crate) content: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationItemResult {
    pub(crate) item_id: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) changed_files: Vec<String>,
    pub(crate) rationale: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FinalReview {
    pub(crate) summary: String,
    pub(crate) ready: bool,
    pub(crate) strengths: Vec<String>,
    pub(crate) findings: Vec<StageFinding>,
    pub(crate) remediation_items: Vec<RemediationItem>,
    pub(crate) next_step: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FinalReviewInput {
    pub(crate) approved: ApprovedProposal,
    pub(crate) plan: ImplementationPlan,
    pub(crate) completed_items: Vec<ImplementationItemResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct WorkflowOutcome {
    pub(crate) status: String,
    pub(crate) approval: ApprovalOutcome,
    pub(crate) plan: Option<ImplementationPlan>,
    pub(crate) architect_review: Option<StageReview>,
    pub(crate) completed_items: Vec<ImplementationItemResult>,
    pub(crate) final_review: Option<FinalReview>,
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ImplementationDraft, ImplementationTaskInput, WorkflowOutcome};

    #[test]
    fn workflow_outcome_deserialises_nested_execution_state() {
        let outcome: WorkflowOutcome = serde_json::from_value(json!({
            "status": "completed",
            "approval": {
                "decision": "approve",
                "summary": "go ahead",
                "final_details": ["keep it minimal"],
                "next_step": "implement"
            },
            "plan": {
                "summary": "ship the post-approval stages",
                "milestones": [{
                    "id": "m1",
                    "title": "Planning",
                    "objective": "Produce the plan",
                    "items": [{
                        "id": "item-1",
                        "title": "Add planning stage",
                        "description": "Create the planning stage outputs",
                        "acceptance_criteria": ["returns milestones"],
                        "dependencies": []
                    }]
                }],
                "risks": ["review churn"]
            },
            "architect_review": {
                "summary": "reasonable plan",
                "findings": [{
                    "severity": "medium",
                    "category": "sequencing",
                    "message": "implement review after management"
                }]
            },
            "completed_items": [{
                "item_id": "item-1",
                "title": "Add planning stage",
                "summary": "implemented stage models",
                "changed_files": ["src/models.rs"],
                "rationale": ["keep ids stable"]
            }],
            "final_review": {
                "summary": "ready to ship",
                "ready": true,
                "strengths": ["structured outputs"],
                "findings": [],
                "remediation_items": [],
                "next_step": "merge"
            },
            "next_step": "merge"
        }))
        .expect("workflow outcome should parse");

        assert_eq!(outcome.status, "completed");
        assert_eq!(
            outcome.plan.expect("plan should exist").milestones[0].id,
            "m1"
        );
        assert_eq!(
            outcome
                .architect_review
                .expect("architect review should exist")
                .findings[0]
                .category,
            "sequencing"
        );
        assert!(
            outcome
                .final_review
                .expect("final review should exist")
                .ready
        );
    }

    #[test]
    fn implementation_draft_round_trips_nested_task_state() {
        let draft: ImplementationDraft = serde_json::from_value(json!({
            "input": {
                "approved": {
                    "proposal": {
                        "title": "Approved proposal",
                        "executive_summary": "summary",
                        "recommended_direction": "direction",
                        "why_this_plan": "why",
                        "adopted_ideas": [],
                        "deferred_ideas": [],
                        "scope": "scope",
                        "architecture": ["layered"],
                        "delivery_plan": ["plan"],
                        "technologies": ["rust"],
                        "major_risks": ["drift"],
                        "open_questions": []
                    },
                    "approval": {
                        "decision": "approve",
                        "summary": "looks good",
                        "final_details": ["no worktrees yet"],
                        "next_step": "implement"
                    }
                },
                "plan": {
                    "summary": "execute stages",
                    "milestones": [{
                        "id": "m1",
                        "title": "Execution",
                        "objective": "Ship the workflow",
                        "items": [{
                            "id": "item-1",
                            "title": "Implement validation",
                            "description": "Add cargo validators",
                            "acceptance_criteria": ["check passes"],
                            "dependencies": []
                        }]
                    }],
                    "risks": []
                },
                "work_item": {
                    "id": "item-1",
                    "source": "plan",
                    "milestone_id": "m1",
                    "title": "Implement validation",
                    "objective": "Add cargo validators",
                    "acceptance_criteria": ["check passes"],
                    "dependencies": []
                },
                "completed_items": [{
                    "item_id": "item-0",
                    "title": "Scaffold stage",
                    "summary": "done",
                    "changed_files": ["src/workflow.rs"],
                    "rationale": ["needed for the next item"]
                }],
                "prior_feedback": [{
                    "severity": "high",
                    "category": "testing",
                    "message": "add a remediation-loop test"
                }]
            },
            "delta": {
                "summary": "implemented validators",
                "rationale": ["reused the existing command helpers"],
                "changes": [{
                    "path": "src/workflow.rs",
                    "action": "write",
                    "content": "updated"
                }]
            }
        }))
        .expect("implementation draft should parse");

        let encoded = serde_json::to_value(&draft).expect("draft should serialise");
        let reparsed: ImplementationDraft =
            serde_json::from_value(encoded).expect("draft should round-trip");

        assert_eq!(reparsed.input.work_item.id, "item-1");
        assert_eq!(reparsed.input.prior_feedback[0].category, "testing");
        assert_eq!(reparsed.delta.changes[0].path, "src/workflow.rs");
    }

    #[test]
    fn implementation_task_input_accepts_null_milestone_id() {
        let input: ImplementationTaskInput = serde_json::from_value(json!({
            "approved": {
                "proposal": {
                    "title": "Approved proposal",
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
                    "next_step": "implement"
                }
            },
            "plan": {
                "summary": "remediate",
                "milestones": [],
                "risks": []
            },
            "work_item": {
                "id": "rem-1",
                "source": "final_review",
                "milestone_id": null,
                "title": "Fix remaining issue",
                "objective": "Address review finding",
                "acceptance_criteria": ["review passes"],
                "dependencies": []
            },
            "completed_items": [],
            "prior_feedback": []
        }))
        .expect("implementation task input should parse");

        assert!(input.work_item.milestone_id.is_none());
        assert_eq!(input.work_item.source, "final_review");
    }
}
