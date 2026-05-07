use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentBrief {
    pub goals: Vec<String>,
    pub non_goals: Vec<String>,
    pub constraints: Vec<String>,
    pub success_metrics: Vec<String>,
    pub stakeholder_preferences: Vec<String>,
    pub open_questions: Vec<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchBrief {
    pub summary: String,
    pub key_patterns: Vec<String>,
    pub discovered_constraints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceFinding {
    pub claim: String,
    pub source_reference: String,
    pub extracted_content: String,
    pub confidence: f64,
    pub relevance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePack {
    pub findings: Vec<EvidenceFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenQuestion {
    pub question: String,
    pub why_it_matters: String,
    pub suggested_approach: String,
    pub current_confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenQuestions {
    pub questions: Vec<OpenQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessProfile {
    pub project_type: String,
    pub applicable_sops: Vec<String>,
    pub validation_requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDimension {
    pub name: String,
    pub description: String,
    pub check_items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRubric {
    pub dimensions: Vec<ReviewDimension>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationStep {
    pub command: String,
    pub pass_criteria: String,
    pub failure_handling: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationPolicy {
    pub project_type: String,
    pub steps: Vec<ValidationStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationRule {
    pub failure_class: String,
    pub escalation_target: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationRules {
    pub rules: Vec<EscalationRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryStandards {
    pub branch_naming_convention: String,
    pub commit_message_format: String,
    pub pr_size_limit: String,
    pub review_requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Adr {
    pub id: String,
    pub title: String,
    pub status: String,
    pub context: String,
    pub decision: String,
    pub alternatives: Vec<String>,
    pub tradeoffs: String,
    pub consequences: String,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceSpec {
    pub id: String,
    pub module_name: String,
    pub input_types: Vec<String>,
    pub output_types: Vec<String>,
    pub error_modes: Vec<String>,
    pub backwards_compatibility: String,
    pub adr_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyRules {
    pub id: String,
    pub module: String,
    pub allowed_dependencies: Vec<String>,
    pub forbidden_dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCard {
    pub id: String,
    pub description: String,
    pub contract: String,
    pub dependencies: Vec<String>,
    pub adr_references: Vec<String>,
    pub validation_policy: Option<ValidationPolicy>,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub id: String,
    pub name: String,
    pub completed_tasks: Vec<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplementationPatch {
    pub id: String,
    pub task_id: String,
    pub diff: String,
    pub files_changed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureClass {
    ImplementationDefect,
    ArchitecturalConflict,
    MissingKnowledge,
    AmbiguousIntent,
    BrokenProcess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFindingDetail {
    pub dimension: String,
    pub description: String,
    pub location: Option<String>,
    pub failure_class: FailureClass,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFindings {
    pub task_id: String,
    pub findings: Vec<ReviewFindingDetail>,
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceChainStatus {
    pub claim_id: String,
    pub evidence_refs_checked: Vec<String>,
    pub broken_refs: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessAdherenceCheck {
    pub claim_id: String,
    pub required_step: String,
    pub found: bool,
    pub temporal_order_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceAssessment {
    pub claim_id: String,
    pub claimed_confidence: f64,
    pub evidence_strength: String,
    pub assessment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    pub report_id: String,
    pub violation_counts: std::collections::HashMap<String, u32>,
    pub evidence_chain_statuses: Vec<EvidenceChainStatus>,
    pub process_checks: Vec<ProcessAdherenceCheck>,
    pub confidence_assessments: Vec<ConfidenceAssessment>,
    pub summary: String,
}

impl FailureClass {
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
