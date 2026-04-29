use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub summary: String,
    pub milestones: Vec<ExecutionMilestone>,
    pub task_cards: Vec<TaskCard>,
    pub risks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionMilestone {
    pub id: String,
    pub title: String,
    pub objective: String,
    pub task_card_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCard {
    pub id: String,
    pub source: String,
    pub milestone_id: Option<String>,
    pub title: String,
    pub objective: String,
    pub contract_refs: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub expected_files: Vec<String>,
    pub verification_commands: Vec<String>,
    pub dependencies: Vec<String>,
    pub rollback_notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImplementationTaskInput {
    pub handoff: crate::plan::DesignHandoff,
    pub plan: ExecutionPlan,
    pub work_item: TaskCard,
    pub completed_items: Vec<ImplementationItemResult>,
    pub prior_feedback: Vec<StageFinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImplementationDelta {
    pub summary: String,
    pub rationale: Vec<String>,
    pub changes: Vec<FileDelta>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDelta {
    pub path: String,
    pub action: String,
    pub content: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageReview {
    pub summary: String,
    pub findings: Vec<StageFinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageFinding {
    pub severity: String,
    pub category: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImplementationDraft {
    pub input: ImplementationTaskInput,
    pub worktree_name: String,
    pub delta: ImplementationDelta,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImplementationItemResult {
    pub item_id: String,
    pub source: String,
    pub milestone_id: Option<String>,
    pub title: String,
    pub objective: String,
    pub summary: String,
    pub contract_refs: Vec<String>,
    pub changed_files: Vec<String>,
    pub rationale: Vec<String>,
    pub commands_run: Vec<CommandEvidence>,
    pub reviewer_findings: Vec<StageFinding>,
    pub manual_checks: Vec<String>,
    pub known_gaps: Vec<String>,
    pub scope_deviation: Option<String>,
    pub worktree_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEvidence {
    pub command: String,
    pub outcome: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceLog {
    pub task_results: Vec<ImplementationItemResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalReviewInput {
    pub handoff: crate::plan::DesignHandoff,
    pub plan: ExecutionPlan,
    pub completed_items: Vec<ImplementationItemResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalReview {
    pub summary: String,
    pub ready: bool,
    pub strengths: Vec<String>,
    pub findings: Vec<StageFinding>,
    pub remediation_items: Vec<RemediationItem>,
    pub next_step: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemediationItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub related_item_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryOutcome {
    pub status: String,
    pub plan: ExecutionPlan,
    pub completed_items: Vec<ImplementationItemResult>,
    pub final_review: FinalReview,
    pub next_step: String,
}
