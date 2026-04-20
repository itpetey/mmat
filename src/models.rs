use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ApprovalRequest {
    pub(crate) proposal: ReconciledProposal,
    pub(crate) user_response: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationManagementRequest {
    pub(crate) pass_index: usize,
    pub(crate) phase: String,
    pub(crate) approved: ApprovedContract,
    pub(crate) plan: ExecutionPlan,
    pub(crate) architect_review: StageReview,
    pub(crate) completed_items: Vec<ImplementationItemResult>,
    pub(crate) remediation_items: Vec<RemediationItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationTaskInput {
    pub(crate) approved: ApprovedContract,
    pub(crate) plan: ExecutionPlan,
    pub(crate) work_item: TaskCard,
    pub(crate) completed_items: Vec<ImplementationItemResult>,
    pub(crate) prior_feedback: Vec<StageFinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct WorkflowOutcome {
    pub(crate) status: String,
    pub(crate) approval: ApprovalOutcome,
    pub(crate) contract: Option<ProjectContract>,
    pub(crate) contract_approval: Option<ApprovalOutcome>,
    pub(crate) plan: Option<ExecutionPlan>,
    pub(crate) architect_review: Option<StageReview>,
    pub(crate) completed_items: Vec<ImplementationItemResult>,
    pub(crate) final_review: Option<FinalReview>,
    pub(crate) next_step: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RunSummary {
    pub(crate) run_id: String,
    pub(crate) project_root: String,
    pub(crate) run_root: String,
    pub(crate) prompt: String,
    pub(crate) status: String,
    pub(crate) current_stage: String,
    pub(crate) next_step: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FinalReviewInput {
    pub(crate) approved: ApprovedContract,
    pub(crate) plan: ExecutionPlan,
    pub(crate) completed_items: Vec<ImplementationItemResult>,
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
pub(crate) struct ApprovedProposal {
    pub(crate) proposal: ReconciledProposal,
    pub(crate) approval: ApprovalOutcome,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ContractDraftInput {
    pub(crate) intent: IntentBrief,
    pub(crate) approved: ApprovedProposal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ContractApprovalRequest {
    pub(crate) contract: ProjectContract,
    pub(crate) user_response: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ProjectContract {
    pub(crate) problem_statement: String,
    pub(crate) user_goals: Vec<String>,
    pub(crate) non_goals: Vec<String>,
    pub(crate) assumptions: Vec<String>,
    pub(crate) constraints: Vec<String>,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) definition_of_done: Vec<String>,
    pub(crate) approved_tech_choices: Vec<String>,
    pub(crate) explicit_exclusions: Vec<String>,
    pub(crate) demo_scenarios: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ChangeRequest {
    pub(crate) summary: String,
    pub(crate) rationale: String,
    pub(crate) proposed_changes: Vec<String>,
    pub(crate) impact: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ApprovedContract {
    pub(crate) approved: ApprovedProposal,
    pub(crate) contract: ProjectContract,
    pub(crate) contract_approval: ApprovalOutcome,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationDraft {
    pub(crate) input: ImplementationTaskInput,
    pub(crate) worktree_name: String,
    pub(crate) delta: ImplementationDelta,
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
pub(crate) struct StageReview {
    pub(crate) summary: String,
    pub(crate) findings: Vec<StageFinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ExecutionPlan {
    pub(crate) summary: String,
    pub(crate) milestones: Vec<ExecutionMilestone>,
    pub(crate) task_cards: Vec<TaskCard>,
    pub(crate) risks: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ExecutionMilestone {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) objective: String,
    pub(crate) task_card_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationWorklist {
    pub(crate) summary: String,
    pub(crate) items: Vec<TaskCard>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationDelta {
    pub(crate) summary: String,
    pub(crate) rationale: Vec<String>,
    pub(crate) changes: Vec<FileDelta>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ProjectPrompt {
    pub(crate) raw: String,
    pub(crate) clarification_attempt: usize,
    pub(crate) clarification_limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct IntentBrief {
    pub(crate) ready_for_solution: bool,
    pub(crate) problem_statement: String,
    pub(crate) user_goals: Vec<String>,
    pub(crate) non_goals: Vec<String>,
    pub(crate) assumptions: Vec<String>,
    pub(crate) default_assumptions: Vec<String>,
    pub(crate) constraints: Vec<String>,
    pub(crate) ambiguities: Vec<String>,
    pub(crate) risks: Vec<String>,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) clarification_summary: Vec<String>,
    pub(crate) research_notes: Vec<String>,
    pub(crate) recommended_path: String,
    pub(crate) clarification_questions: Vec<String>,
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
pub(crate) struct ReconciledIdea {
    pub(crate) source_branch: String,
    pub(crate) idea: String,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ApprovalOutcome {
    pub(crate) decision: String,
    pub(crate) summary: String,
    pub(crate) final_details: Vec<String>,
    pub(crate) next_step: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StageFinding {
    pub(crate) severity: String,
    pub(crate) category: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct TaskCard {
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) milestone_id: Option<String>,
    pub(crate) title: String,
    pub(crate) objective: String,
    pub(crate) contract_refs: Vec<String>,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) expected_files: Vec<String>,
    pub(crate) verification_commands: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) rollback_notes: Vec<String>,
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
pub(crate) struct FileDelta {
    pub(crate) path: String,
    pub(crate) action: String,
    pub(crate) content: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImplementationItemResult {
    pub(crate) item_id: String,
    pub(crate) source: String,
    pub(crate) milestone_id: Option<String>,
    pub(crate) title: String,
    pub(crate) objective: String,
    pub(crate) summary: String,
    pub(crate) contract_refs: Vec<String>,
    pub(crate) changed_files: Vec<String>,
    pub(crate) rationale: Vec<String>,
    pub(crate) commands_run: Vec<CommandEvidence>,
    pub(crate) reviewer_findings: Vec<StageFinding>,
    pub(crate) manual_checks: Vec<String>,
    pub(crate) known_gaps: Vec<String>,
    pub(crate) scope_deviation: Option<String>,
    pub(crate) worktree_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct CommandEvidence {
    pub(crate) command: String,
    pub(crate) outcome: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct EvidenceLog {
    pub(crate) task_results: Vec<ImplementationItemResult>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SolutionBranch {
    Conservative,
    Recommended,
    Ambitious,
}

impl SolutionBranch {
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Recommended => "recommended",
            Self::Ambitious => "ambitious",
        }
    }

    pub(crate) fn default_set() -> [Self; 3] {
        [Self::Conservative, Self::Recommended, Self::Ambitious]
    }

    pub(crate) fn instruction(self) -> &'static str {
        match self {
            Self::Conservative => {
                "Optimise for the fastest credible delivery path with the lowest operational and implementation risk. Prefer boring technology and the smallest viable scope that still solves the problem well."
            }
            Self::Recommended => {
                "Optimise for strong engineering discipline, maintainability, clarity, and long-term operability. Recommend the professional default even if it is not the quickest route."
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

    use super::{
        ApprovalRequest, ImplementationDraft, ImplementationTaskInput, IntentBrief, RunSummary,
        SolutionBranch, WorkflowOutcome,
    };

    #[test]
    fn default_solution_branches_use_three_lane_set() {
        let branches = SolutionBranch::default_set();

        assert_eq!(branches.len(), 3);
        assert_eq!(branches[0].slug(), "conservative");
        assert_eq!(branches[1].slug(), "recommended");
        assert_eq!(branches[2].slug(), "ambitious");
    }

    #[test]
    fn intent_brief_deserialises_readiness_flag() {
        let brief: IntentBrief = serde_json::from_value(json!({
            "ready_for_solution": true,
            "problem_statement": "Build a task tracker",
            "user_goals": ["Capture tasks"],
            "non_goals": ["Collaboration"],
            "assumptions": ["Single user"],
            "default_assumptions": ["Web app"],
            "constraints": ["Use Python"],
            "ambiguities": ["Deployment target"],
            "risks": ["Too much scope"],
            "acceptance_criteria": ["Tasks can be created"],
            "clarification_summary": ["User asked for a small web app"],
            "research_notes": [],
            "recommended_path": "Generate solution branches",
            "clarification_questions": []
        }))
        .expect("intent brief should parse");

        assert!(brief.ready_for_solution);
        assert_eq!(brief.problem_statement, "Build a task tracker");
        assert_eq!(brief.user_goals, vec!["Capture tasks"]);
    }

    #[test]
    fn approval_request_round_trips_proposal_and_user_feedback() {
        let request: ApprovalRequest = serde_json::from_value(json!({
            "proposal": {
                "title": "Proposal",
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
            "user_response": "approve with Python"
        }))
        .expect("approval request should parse");

        assert_eq!(request.proposal.title, "Proposal");
        assert_eq!(request.user_response, "approve with Python");
    }

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
                    "task_card_ids": ["item-1"]
                }],
                "task_cards": [{
                    "id": "item-1",
                    "source": "plan",
                    "milestone_id": "m1",
                    "title": "Add planning stage",
                    "objective": "Create the planning stage outputs",
                    "contract_refs": ["AC-1"],
                    "acceptance_criteria": ["returns milestones"],
                    "expected_files": ["src/models.rs"],
                    "verification_commands": ["cargo test"],
                    "dependencies": [],
                    "rollback_notes": ["revert the milestone"]
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
                "source": "plan",
                "milestone_id": "m1",
                "title": "Add planning stage",
                "objective": "Create the planning stage outputs",
                "summary": "implemented stage models",
                "contract_refs": ["AC-1"],
                "changed_files": ["src/models.rs"],
                "rationale": ["keep ids stable"],
                "commands_run": [{
                    "command": "cargo test",
                    "outcome": "passed"
                }],
                "reviewer_findings": [],
                "manual_checks": ["returns milestones"],
                "known_gaps": [],
                "scope_deviation": null,
                "worktree_name": "initial-implementation-item-1"
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
                    "contract": {
                        "problem_statement": "Build the workflow",
                        "user_goals": ["Ship validation"],
                        "non_goals": ["Rewrite the framework"],
                        "assumptions": ["Rust workspace"],
                        "constraints": ["Use cargo validators"],
                        "acceptance_criteria": ["cargo check passes"],
                        "definition_of_done": ["workflow completes"],
                        "approved_tech_choices": ["rust"],
                        "explicit_exclusions": ["Python service"],
                        "demo_scenarios": ["Run MMAT"]
                    },
                    "contract_approval": {
                        "decision": "approve",
                        "summary": "contract looks good",
                        "final_details": [],
                        "next_step": "plan"
                    }
                },
                "plan": {
                    "summary": "execute stages",
                    "milestones": [{
                        "id": "m1",
                        "title": "Execution",
                        "objective": "Ship the workflow",
                        "task_card_ids": ["item-1"]
                    }],
                    "task_cards": [{
                        "id": "item-1",
                        "source": "plan",
                        "milestone_id": "m1",
                        "title": "Implement validation",
                        "objective": "Add cargo validators",
                        "contract_refs": ["AC-1"],
                        "acceptance_criteria": ["check passes"],
                        "expected_files": ["src/workflow.rs"],
                        "verification_commands": ["cargo check", "cargo test"],
                        "dependencies": [],
                        "rollback_notes": ["revert validator changes"]
                    }],
                    "risks": []
                },
                "work_item": {
                    "id": "item-1",
                    "source": "plan",
                    "milestone_id": "m1",
                    "title": "Implement validation",
                    "objective": "Add cargo validators",
                    "contract_refs": ["AC-1"],
                    "acceptance_criteria": ["check passes"],
                    "expected_files": ["src/workflow.rs"],
                    "verification_commands": ["cargo check", "cargo test"],
                    "dependencies": [],
                    "rollback_notes": ["revert validator changes"]
                },
                "completed_items": [{
                    "item_id": "item-0",
                    "source": "plan",
                    "milestone_id": "m0",
                    "title": "Scaffold stage",
                    "objective": "Prepare the workflow",
                    "summary": "done",
                    "contract_refs": ["AC-0"],
                    "changed_files": ["src/workflow.rs"],
                    "rationale": ["needed for the next item"],
                    "commands_run": [{
                        "command": "cargo check",
                        "outcome": "passed"
                    }],
                    "reviewer_findings": [],
                    "manual_checks": ["workflow exists"],
                    "known_gaps": [],
                    "scope_deviation": null,
                    "worktree_name": "initial-implementation-item-0"
                }],
                "prior_feedback": [{
                    "severity": "high",
                    "category": "testing",
                    "message": "add a remediation-loop test"
                }]
            },
            "worktree_name": "initial-implementation-item-1",
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
        assert_eq!(reparsed.worktree_name, "initial-implementation-item-1");
        assert_eq!(reparsed.input.prior_feedback[0].category, "testing");
        assert_eq!(reparsed.delta.changes[0].path, "src/workflow.rs");
    }

    #[test]
    fn implementation_task_input_accepts_null_milestone_id() {
        let input: ImplementationTaskInput = serde_json::from_value(json!({
            "approved": {
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
                "contract": {
                    "problem_statement": "Fix the issue",
                    "user_goals": ["Resolve the review finding"],
                    "non_goals": ["Expand scope"],
                    "assumptions": ["Existing implementation remains valid"],
                    "constraints": ["Stay within the repo"],
                    "acceptance_criteria": ["review passes"],
                    "definition_of_done": ["issue resolved"],
                    "approved_tech_choices": ["rust"],
                    "explicit_exclusions": ["New service"],
                    "demo_scenarios": ["Run review again"]
                },
                "contract_approval": {
                    "decision": "approve",
                    "summary": "contract approved",
                    "final_details": [],
                    "next_step": "plan"
                }
            },
            "plan": {
                "summary": "remediate",
                "milestones": [],
                "task_cards": [],
                "risks": []
            },
            "work_item": {
                "id": "rem-1",
                "source": "final_review",
                "milestone_id": null,
                "title": "Fix remaining issue",
                "objective": "Address review finding",
                "contract_refs": ["AC-2"],
                "acceptance_criteria": ["review passes"],
                "expected_files": ["src/workflow.rs"],
                "verification_commands": ["cargo test"],
                "dependencies": [],
                "rollback_notes": ["revert the remediation"]
            },
            "completed_items": [],
            "prior_feedback": []
        }))
        .expect("implementation task input should parse");

        assert!(input.work_item.milestone_id.is_none());
        assert_eq!(input.work_item.source, "final_review");
    }

    #[test]
    fn run_summary_round_trips_core_run_metadata() {
        let summary: RunSummary = serde_json::from_value(json!({
            "run_id": "run-1",
            "project_root": "/tmp/project",
            "run_root": "/tmp/project/.mmat/runs/run-1",
            "prompt": "build a todo app",
            "status": "running",
            "current_stage": "planning",
            "next_step": "architect_review"
        }))
        .expect("run summary should parse");

        assert_eq!(summary.run_id, "run-1");
        assert_eq!(summary.current_stage, "planning");
        assert_eq!(summary.next_step.as_deref(), Some("architect_review"));
    }
}
