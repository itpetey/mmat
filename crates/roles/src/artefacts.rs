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
