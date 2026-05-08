//! Structured artefact types exchanged between roles during the software engineering process.

use serde::{Deserialize, Serialize};

/// Captures the goals, non-goals, constraints, and success metrics for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentBrief {
    /// The goals the project aims to achieve.
    pub goals: Vec<String>,
    /// Explicitly excluded objectives.
    pub non_goals: Vec<String>,
    /// Constraints such as budget, timeline, technology, or compliance.
    pub constraints: Vec<String>,
    /// Measurable outcomes that define success.
    pub success_metrics: Vec<String>,
    /// Stakeholder preferences and priorities.
    pub stakeholder_preferences: Vec<String>,
    /// Unresolved questions that may affect decision-making.
    pub open_questions: Vec<String>,
    /// Confidence in the completeness of the brief (0.0–1.0).
    pub confidence: f64,
}

/// Summary of a research investigation into a codebase or problem domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchBrief {
    /// High-level summary of research findings.
    pub summary: String,
    /// Key patterns discovered during research.
    pub key_patterns: Vec<String>,
    /// Constraints discovered during the investigation.
    pub discovered_constraints: Vec<String>,
}

/// A single piece of evidence gathered during research.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceFinding {
    /// The claim or statement this evidence supports.
    pub claim: String,
    /// Reference to the source of the evidence.
    pub source_reference: String,
    /// The extracted content from the source.
    pub extracted_content: String,
    /// Confidence in the reliability of this evidence (0.0–1.0).
    pub confidence: f64,
    /// Description of how relevant this finding is.
    pub relevance: String,
}

/// A collection of evidence findings assembled by the Scholar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePack {
    /// Individual evidence findings.
    pub findings: Vec<EvidenceFinding>,
}

/// A single unresolved question requiring further investigation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenQuestion {
    /// The question that remains unanswered.
    pub question: String,
    /// Why answering this question matters.
    pub why_it_matters: String,
    /// Suggested approach for resolving the question.
    pub suggested_approach: String,
    /// Current confidence that the question will be resolved (0.0–1.0).
    pub current_confidence: f64,
}

/// A collection of open questions produced by the Scholar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenQuestions {
    /// The list of open questions.
    pub questions: Vec<OpenQuestion>,
}

/// A profile describing the process requirements for a project type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessProfile {
    /// The project type identifier.
    pub project_type: String,
    /// Standard operating procedures applicable to this project.
    pub applicable_sops: Vec<String>,
    /// Validation requirements for this project type.
    pub validation_requirements: Vec<String>,
}

/// A single dimension along which a review is conducted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDimension {
    /// Name of the review dimension (e.g. correctness, security).
    pub name: String,
    /// Description of what this dimension evaluates.
    pub description: String,
    /// Specific items to check within this dimension.
    pub check_items: Vec<String>,
}

/// A structured rubric containing multiple review dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRubric {
    /// The dimensions that make up this rubric.
    pub dimensions: Vec<ReviewDimension>,
}

/// A single validation step to be executed during quality checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationStep {
    /// The command to execute for this step.
    pub command: String,
    /// The criteria that define a passing result.
    pub pass_criteria: String,
    /// How to handle failure of this step.
    pub failure_handling: String,
}

/// A validation policy defining the required checks for a project type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationPolicy {
    /// The project type this policy applies to.
    pub project_type: String,
    /// The ordered validation steps to execute.
    pub steps: Vec<ValidationStep>,
}

/// A rule mapping a failure class to an escalation target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationRule {
    /// The class of failure that triggers this rule.
    pub failure_class: String,
    /// The role to escalate to.
    pub escalation_target: String,
    /// Description of the escalation rule.
    pub description: String,
}

/// A collection of escalation rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationRules {
    /// The individual escalation rules.
    pub rules: Vec<EscalationRule>,
}

/// Delivery standards enforced by the OpsManager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryStandards {
    /// Convention for naming branches.
    pub branch_naming_convention: String,
    /// Format for commit messages.
    pub commit_message_format: String,
    /// Maximum size of a pull request.
    pub pr_size_limit: String,
    /// Requirements that must be met before merging.
    pub review_requirements: Vec<String>,
}

/// An Architecture Decision Record capturing a design decision and its rationale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Adr {
    /// Unique identifier for this ADR.
    pub id: String,
    /// Title of the decision.
    pub title: String,
    /// Status of the decision (e.g. proposed, accepted).
    pub status: String,
    /// The context in which the decision was made.
    pub context: String,
    /// The decision itself.
    pub decision: String,
    /// Alternatives that were considered.
    pub alternatives: Vec<String>,
    /// Tradeoffs of the chosen approach.
    pub tradeoffs: String,
    /// Expected consequences of the decision.
    pub consequences: String,
    /// References to related documents or discussions.
    pub references: Vec<String>,
}

/// A specification for a module interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceSpec {
    /// Unique identifier for this specification.
    pub id: String,
    /// Name of the module.
    pub module_name: String,
    /// Expected input types.
    pub input_types: Vec<String>,
    /// Expected output types.
    pub output_types: Vec<String>,
    /// Known error modes.
    pub error_modes: Vec<String>,
    /// Statement about backwards compatibility.
    pub backwards_compatibility: String,
    /// Reference to the associated ADR.
    pub adr_reference: String,
}

/// Rules defining allowed and forbidden dependencies for a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyRules {
    /// Unique identifier for these dependency rules.
    pub id: String,
    /// The module these rules apply to.
    pub module: String,
    /// Dependencies that are permitted.
    pub allowed_dependencies: Vec<String>,
    /// Dependencies that are forbidden.
    pub forbidden_dependencies: Vec<String>,
}

/// A task card describing a unit of work to be implemented.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCard {
    /// Unique identifier for this task.
    pub id: String,
    /// Description of the work to be done.
    pub description: String,
    /// The contract specification for the task.
    pub contract: String,
    /// IDs of tasks that this task depends on.
    pub dependencies: Vec<String>,
    /// References to relevant ADRs.
    pub adr_references: Vec<String>,
    /// Optional validation policy for this task.
    pub validation_policy: Option<ValidationPolicy>,
    /// Criteria that must be met for the task to be accepted.
    pub acceptance_criteria: Vec<String>,
}

/// A milestone marking the completion of a set of tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    /// Unique identifier for this milestone.
    pub id: String,
    /// Human-readable name of the milestone.
    pub name: String,
    /// IDs of tasks completed at this milestone.
    pub completed_tasks: Vec<String>,
    /// Timestamp when the milestone was reached.
    pub timestamp: String,
}

/// A patch representing an implementation's diff output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplementationPatch {
    /// Unique identifier for this patch.
    pub id: String,
    /// The task this patch implements.
    pub task_id: String,
    /// The unified diff content.
    pub diff: String,
    /// List of files changed by this patch.
    pub files_changed: Vec<String>,
}

/// Classification of failure types identified during review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureClass {
    /// The implementation contains a defect or bug.
    ImplementationDefect,
    /// The implementation conflicts with established architecture.
    ArchitecturalConflict,
    /// Necessary knowledge is missing to complete the task.
    MissingKnowledge,
    /// The intent or requirements are ambiguous.
    AmbiguousIntent,
    /// A process or SOP is broken.
    BrokenProcess,
}

/// A detailed finding from a review, linked to a specific dimension and failure class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFindingDetail {
    /// The review dimension this finding relates to.
    pub dimension: String,
    /// Description of the finding.
    pub description: String,
    /// Optional location in the code where the issue was found.
    pub location: Option<String>,
    /// The class of failure identified.
    pub failure_class: FailureClass,
}

/// A collection of review findings for a specific task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFindings {
    /// The task being reviewed.
    pub task_id: String,
    /// The individual findings from the review.
    pub findings: Vec<ReviewFindingDetail>,
    /// Whether the task was accepted after review.
    pub accepted: bool,
}

/// Status of evidence chain verification for a claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceChainStatus {
    /// The claim being verified.
    pub claim_id: String,
    /// Evidence references that were checked.
    pub evidence_refs_checked: Vec<String>,
    /// References that were found to be broken.
    pub broken_refs: Vec<String>,
    /// Overall status (e.g. intact, broken).
    pub status: String,
}

/// A check that a required process step was followed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessAdherenceCheck {
    /// The claim being checked.
    pub claim_id: String,
    /// The required process step.
    pub required_step: String,
    /// Whether the step was found.
    pub found: bool,
    /// Whether the temporal ordering of the step is valid.
    pub temporal_order_valid: bool,
}

/// Assessment of the confidence stated in a claim against available evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceAssessment {
    /// The claim being assessed.
    pub claim_id: String,
    /// The confidence claimed in the claim.
    pub claimed_confidence: f64,
    /// Strength of the supporting evidence.
    pub evidence_strength: String,
    /// Overall assessment of the confidence.
    pub assessment: String,
}

/// A periodic report produced by the Auditor summarising violations and assessments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    /// Unique identifier for this report.
    pub report_id: String,
    /// Counts of violations by type.
    pub violation_counts: std::collections::HashMap<String, u32>,
    /// Status of evidence chains checked.
    pub evidence_chain_statuses: Vec<EvidenceChainStatus>,
    /// Process adherence checks performed.
    pub process_checks: Vec<ProcessAdherenceCheck>,
    /// Confidence assessments performed.
    pub confidence_assessments: Vec<ConfidenceAssessment>,
    /// Human-readable summary of the audit findings.
    pub summary: String,
}

impl FailureClass {
    /// Returns the string representation of this failure class.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ImplementationDefect => "ImplementationDefect",
            Self::ArchitecturalConflict => "ArchitecturalConflict",
            Self::MissingKnowledge => "MissingKnowledge",
            Self::AmbiguousIntent => "AmbiguousIntent",
            Self::BrokenProcess => "BrokenProcess",
        }
    }
}
