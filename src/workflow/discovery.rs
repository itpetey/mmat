use std::fmt::Display;

use naaf_core::Attempt;
use naaf_llm::{HumanIO, HumanQuestion};
use serde::{Deserialize, Serialize};

pub const MODEL: &str = "gpt-5.5";
pub const SYSTEM_PROMPT: &str = "You are a curious sounding board for new ideas. Your job is to interrogate the idea, fleshing out any unknowns, researching prior art, and soliciting feedback from the user.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryInput {
    initial_prompt: String,
    answers: Vec<DiscoveryAnswer>,
    findings: Vec<DiscoveryFinding>,
    last_output: Option<DiscoveryOutput>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryOutput {
    pub ready_for_solution: bool,
    pub problem_statement: String,
    pub goals: Vec<String>,
    pub constraints: Vec<String>,
    pub assumptions: Vec<String>,
    pub risks: Vec<String>,
    pub notes: Vec<String>,
    pub recommended_path: String,
    pub open_questions: Vec<DiscoveryQuestion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryQuestion {
    pub prompt: String,
    pub choices: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryAnswer {
    pub question: String,
    pub answer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryFinding {
    MissingProblemStatement,
    MissingGoals,
    MissingConstraints,
    UnresolvedBlockingAmbiguity,
    NoClarificationQuestions,
}

impl Display for DiscoveryFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProblemStatement => write!(f, "missing problem statement"),
            Self::MissingGoals => write!(f, "missing goals"),
            Self::MissingConstraints => write!(f, "missing constraints"),
            Self::UnresolvedBlockingAmbiguity => write!(f, "unresolved blocking ambiguity"),
            Self::NoClarificationQuestions => {
                write!(f, "no clarification questions despite not being ready")
            }
        }
    }
}

pub fn build_prompt(input: DiscoveryInput) -> String {
    let mut lines = vec![format!("Initial prompt: {}", input.initial_prompt)];

    if let Some(prior_state) = &input.last_output {
        lines.push(String::new());
        lines.push("Current understanding:".to_string());
        lines.push(format!(
            "Problem statement: {}",
            prior_state.problem_statement
        ));

        if !prior_state.goals.is_empty() {
            lines.push(format!("Goals: {}", prior_state.goals.join(" | ")));
        }

        if !prior_state.constraints.is_empty() {
            lines.push(format!(
                "Constraints: {}",
                prior_state.constraints.join(" | ")
            ));
        }
    }

    if !input.findings.is_empty() {
        lines.push(String::new());
        lines.push("Validation findings to address in this turn:".to_string());
        lines.extend(
            input
                .findings
                .iter()
                .map(|finding| format!("- {}", finding)),
        );
    }

    if !input.answers.is_empty() {
        lines.push(String::new());
        lines.push("Answered clarifications:".to_string());
        lines.extend(
            input
                .answers
                .iter()
                .map(|answer| format!("- {} => {}", answer.question, answer.answer)),
        );
    }

    lines.push(String::new());
    lines.push(
        "Return the next structured discovery state, including explicit uncertainty and any remaining high-value questions."
            .to_string(),
    );
    lines.push(
        "Only mark ready_for_solution true when the hand-off is complete enough for solution generation without blocking ambiguity."
            .to_string(),
    );

    lines.join("\n")
}

pub fn validate<R>(_runtime: &R, output: DiscoveryOutput) -> Vec<DiscoveryFinding> {
    let mut findings = Vec::new();

    if output.problem_statement.trim().is_empty() {
        findings.push(DiscoveryFinding::MissingProblemStatement);
    }

    if output.goals.iter().any(|item| !item.trim().is_empty()) {
        findings.push(DiscoveryFinding::MissingGoals);
    }

    if output
        .constraints
        .iter()
        .any(|item| !item.trim().is_empty())
    {
        findings.push(DiscoveryFinding::MissingConstraints);
    }

    if !output.ready_for_solution || !output.open_questions.is_empty() {
        findings.push(DiscoveryFinding::UnresolvedBlockingAmbiguity);
    }

    if !output.ready_for_solution && output.open_questions.is_empty() {
        findings.push(DiscoveryFinding::NoClarificationQuestions);
    }

    findings
}

pub async fn repair<R>(
    runtime: &R,
    attempts: Vec<Attempt<DiscoveryInput, DiscoveryOutput, DiscoveryFinding>>,
) -> Result<DiscoveryInput, R::Error>
where
    R: HumanIO + 'static,
{
    let latest_attempt = attempts
        .last()
        .expect("discovery repair requires an attempt");
    let mut answers = latest_attempt.input.answers.clone();

    for question in &latest_attempt.output.open_questions {
        let reply = runtime
            .ask(HumanQuestion {
                question: question.prompt.clone(),
                choices: if question.choices.is_empty() {
                    None
                } else {
                    Some(question.choices.clone())
                },
            })
            .await?;
        answers.push(DiscoveryAnswer {
            question: question.prompt.clone(),
            answer: reply.content,
        });
    }

    Ok(DiscoveryInput {
        initial_prompt: latest_attempt.input.initial_prompt.clone(),
        answers,
        findings: latest_attempt.findings.clone(),
        last_output: Some(latest_attempt.output.clone()),
    })
}
