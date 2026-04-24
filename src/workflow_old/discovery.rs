use std::sync::Arc;

use futures::future::LocalBoxFuture;
use naaf_core::{Attempt, RetryPolicy, Step, TaskExt, check_fn, repair_fn, task_fn};
use naaf_llm::{HumanIO, HumanQuestion};
use serde::{Deserialize, Serialize};

use crate::workflow_old::WorkflowError;

pub trait DiscoveryTurnAgent<R>: Send + Sync + 'static {
    fn run_turn<'a>(
        &'a self,
        runtime: &'a R,
        input: DiscoveryInput,
        prompt: String,
    ) -> LocalBoxFuture<'a, Result<DiscoveryState, WorkflowError>>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryInput {
    pub initial_prompt: String,
    pub turn: usize,
    pub clarification_budget: usize,
    pub answers: Vec<DiscoveryAnswer>,
    pub findings: Vec<DiscoveryFinding>,
    pub prior_state: Option<DiscoveryState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryOutcome {
    pub state: DiscoveryState,
    pub answers: Vec<DiscoveryAnswer>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryState {
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
pub struct DiscoveryAnswer {
    pub question: String,
    pub answer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryQuestion {
    pub prompt: String,
    pub choices: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryTurn {
    pub input: DiscoveryInput,
    pub state: DiscoveryState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryFinding {
    MissingProblemStatement,
    MissingGoals,
    MissingConstraints,
    UnresolvedBlockingAmbiguity,
    NoClarificationQuestions,
    ExceededClarificationBudget,
}

impl DiscoveryInput {
    pub fn new(initial_prompt: impl Into<String>) -> Self {
        Self {
            initial_prompt: initial_prompt.into(),
            turn: 0,
            clarification_budget: 1,
            answers: Vec::new(),
            findings: Vec::new(),
            prior_state: None,
        }
    }

    pub fn with_clarification_budget(mut self, clarification_budget: usize) -> Self {
        self.clarification_budget = clarification_budget;
        self
    }
}

impl DiscoveryFinding {
    pub fn description(&self) -> &'static str {
        match self {
            Self::MissingProblemStatement => "missing problem statement",
            Self::MissingGoals => "missing goals",
            Self::MissingConstraints => "missing constraints",
            Self::UnresolvedBlockingAmbiguity => "unresolved blocking ambiguity",
            Self::NoClarificationQuestions => "no clarification questions despite not being ready",
            Self::ExceededClarificationBudget => "exceeded clarification budget",
        }
    }
}

fn build_discovery_prompt(input: &DiscoveryInput) -> String {
    let mut lines = vec![
        "You are the discovery stage for MMAT.".to_string(),
        format!("Initial prompt: {}", input.initial_prompt),
        format!("Discovery turn: {}", input.turn + 1),
        format!(
            "Clarification budget: {} turn(s)",
            input.clarification_budget
        ),
    ];

    if let Some(prior_state) = &input.prior_state {
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
                .map(|finding| format!("- {}", finding.description())),
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

fn has_non_empty_items(items: &[String]) -> bool {
    items.iter().any(|item| !item.trim().is_empty())
}

fn validate_discovery_turn(turn: &DiscoveryTurn) -> Vec<DiscoveryFinding> {
    let mut findings = Vec::new();
    let state = &turn.state;
    let has_open_questions = !state.open_questions.is_empty();
    let missing_problem_statement = state.problem_statement.trim().is_empty();
    let missing_goals = !has_non_empty_items(&state.goals);
    let missing_constraints = !has_non_empty_items(&state.constraints);
    let not_ready = !state.ready_for_solution || has_open_questions;

    if missing_problem_statement {
        findings.push(DiscoveryFinding::MissingProblemStatement);
    }

    if missing_goals {
        findings.push(DiscoveryFinding::MissingGoals);
    }

    if missing_constraints {
        findings.push(DiscoveryFinding::MissingConstraints);
    }

    if not_ready {
        findings.push(DiscoveryFinding::UnresolvedBlockingAmbiguity);
    }

    if !state.ready_for_solution && state.open_questions.is_empty() {
        findings.push(DiscoveryFinding::NoClarificationQuestions);
    }

    if not_ready && turn.input.turn + 1 >= turn.input.clarification_budget {
        findings.push(DiscoveryFinding::ExceededClarificationBudget);
    }

    findings
}

async fn plan_next_discovery_input<R>(
    runtime: &R,
    attempts: Vec<Attempt<DiscoveryInput, DiscoveryTurn, DiscoveryFinding>>,
) -> Result<DiscoveryInput, WorkflowError>
where
    R: HumanIO<Error = WorkflowError> + 'static,
{
    let latest_attempt = attempts
        .last()
        .expect("discovery repair requires an attempt");
    let mut answers = latest_attempt.artefact.input.answers.clone();

    for question in &latest_attempt.artefact.state.open_questions {
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
        initial_prompt: latest_attempt.artefact.input.initial_prompt.clone(),
        turn: latest_attempt.artefact.input.turn + 1,
        clarification_budget: latest_attempt.artefact.input.clarification_budget,
        answers,
        findings: latest_attempt.findings.clone(),
        prior_state: Some(latest_attempt.artefact.state.clone()),
    })
}

pub fn build_turn_step<R, A>(
    agent: Arc<A>,
    retry_policy: RetryPolicy,
) -> Step<R, DiscoveryInput, DiscoveryTurn, DiscoveryFinding, WorkflowError>
where
    R: HumanIO<Error = WorkflowError> + 'static,
    A: DiscoveryTurnAgent<R>,
{
    Step::builder(
        task_fn(move |runtime: &R, input: DiscoveryInput| {
            let agent = agent.clone();
            let prompt = build_discovery_prompt(&input);
            Box::pin(async move {
                let state = agent.run_turn(runtime, input.clone(), prompt).await?;
                Ok(DiscoveryTurn { input, state })
            })
        })
        .observed_as("discovery_turn"),
    )
    .validate(check_fn(|_runtime: &R, turn: DiscoveryTurn| {
        Box::pin(async move { Ok(validate_discovery_turn(&turn)) })
    }))
    .repair_with(repair_fn(|runtime: &R, attempts| {
        Box::pin(async move { plan_next_discovery_input(runtime, attempts).await })
    }))
    .retry_policy(retry_policy)
    .build()
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Arc};

    use naaf_core::StepReport;
    use parking_lot::Mutex;

    use crate::runtime::ScriptedRuntime;

    use super::*;

    #[derive(Default)]
    struct StubDiscoveryAgent {
        states: Mutex<VecDeque<DiscoveryState>>,
        prompts: Mutex<Vec<String>>,
    }

    impl StubDiscoveryAgent {
        fn new(states: Vec<DiscoveryState>) -> Self {
            Self {
                states: Mutex::new(states.into()),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().clone()
        }
    }

    impl DiscoveryTurnAgent<ScriptedRuntime> for StubDiscoveryAgent {
        fn run_turn<'a>(
            &'a self,
            _runtime: &'a ScriptedRuntime,
            _input: DiscoveryInput,
            prompt: String,
        ) -> LocalBoxFuture<'a, Result<DiscoveryState, WorkflowError>> {
            self.prompts.lock().push(prompt);
            let state = self
                .states
                .lock()
                .pop_front()
                .expect("stub discovery state should exist");
            Box::pin(async move { Ok(state) })
        }
    }

    async fn run_live_discovery<R>(
        runtime: &R,
        step: &Step<R, DiscoveryInput, DiscoveryTurn, DiscoveryFinding, WorkflowError>,
        initial_prompt: impl Into<String>,
        clarification_budget: usize,
    ) -> Result<DiscoveryOutcome, WorkflowError>
    where
        R: HumanIO<Error = WorkflowError> + 'static,
    {
        let traced = step
            .run_traced(
                runtime,
                DiscoveryInput::new(initial_prompt).with_clarification_budget(clarification_budget),
            )
            .await
            .map_err(|error| match error {
                naaf_core::StepError::Rejected(report) => WorkflowError::Discovery(
                    discovery_rejection_message(&report, clarification_budget),
                ),
                other => {
                    WorkflowError::Discovery(format!("discovery step execution failed: {other}"))
                }
            })?;

        Ok(DiscoveryOutcome {
            state: traced.output().state.clone(),
            answers: traced.output().input.answers.clone(),
        })
    }

    fn describe_findings(findings: &[DiscoveryFinding]) -> String {
        findings
            .iter()
            .map(DiscoveryFinding::description)
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn discovery_rejection_message(
        report: &StepReport<DiscoveryFinding>,
        clarification_budget: usize,
    ) -> String {
        let Some(last_attempt) = report.attempts().last() else {
            return "discovery step rejected without any attempts".to_string();
        };

        if last_attempt
            .findings
            .contains(&DiscoveryFinding::NoClarificationQuestions)
        {
            return "discovery is not solution-ready and produced no further questions".to_string();
        }

        if last_attempt
            .findings
            .contains(&DiscoveryFinding::ExceededClarificationBudget)
        {
            return format!(
                "discovery exceeded the configured turn budget of {clarification_budget}"
            );
        }

        if last_attempt.findings.is_empty() {
            return "discovery was rejected without validation findings".to_string();
        }

        format!(
            "discovery is not ready for solution generation: {}",
            describe_findings(&last_attempt.findings)
        )
    }

    fn ready_state() -> DiscoveryState {
        DiscoveryState {
            ready_for_solution: true,
            problem_statement: "Build a workflow rewrite".to_string(),
            goals: vec!["Preserve the workflow shape".to_string()],
            constraints: vec!["Use scoped knowledge".to_string()],
            assumptions: vec!["Live questions are acceptable".to_string()],
            risks: vec!["Architecture drift".to_string()],
            notes: vec!["Discovery complete".to_string()],
            recommended_path: "Generate solution branches".to_string(),
            open_questions: Vec::new(),
        }
    }

    #[tokio::test]
    async fn live_discovery_reuses_prior_answers_and_reaches_ready_state() {
        let runtime = ScriptedRuntime::new(["Use SQLite for metadata"]);
        let agent = Arc::new(StubDiscoveryAgent::new(vec![
            DiscoveryState {
                ready_for_solution: false,
                problem_statement: "Rewrite MMAT".to_string(),
                goals: vec!["Keep the workflow shape".to_string()],
                constraints: Vec::new(),
                assumptions: Vec::new(),
                risks: vec!["Prompt overload".to_string()],
                notes: Vec::new(),
                recommended_path: "Clarify persistence".to_string(),
                open_questions: vec![DiscoveryQuestion {
                    prompt: "What should back knowledge-group metadata?".to_string(),
                    choices: vec!["SQLite".to_string(), "Filesystem".to_string()],
                }],
            },
            DiscoveryState {
                constraints: vec![
                    "Use scoped knowledge".to_string(),
                    "SQLite metadata".to_string(),
                ],
                ..ready_state()
            },
        ]));
        let step = build_turn_step(agent.clone(), RetryPolicy::new(3));

        let outcome = run_live_discovery(&runtime, &step, "Rewrite MMAT", 3)
            .await
            .expect("discovery should finish");

        assert!(outcome.state.ready_for_solution);
        assert_eq!(outcome.answers.len(), 1);
        assert_eq!(outcome.answers[0].answer, "Use SQLite for metadata");
        assert_eq!(runtime.asked_questions().len(), 1);
        assert!(
            agent
                .prompts()
                .last()
                .expect("second prompt should exist")
                .contains("Use SQLite for metadata")
        );
    }

    #[tokio::test]
    async fn live_discovery_fails_when_more_questions_are_missing() {
        let runtime = ScriptedRuntime::new(std::iter::empty::<String>());
        let agent = Arc::new(StubDiscoveryAgent::new(vec![DiscoveryState {
            ready_for_solution: false,
            problem_statement: "Rewrite MMAT".to_string(),
            goals: Vec::new(),
            constraints: Vec::new(),
            assumptions: Vec::new(),
            risks: Vec::new(),
            notes: Vec::new(),
            recommended_path: "Clarify goals".to_string(),
            open_questions: Vec::new(),
        }]));
        let step = build_turn_step(agent, RetryPolicy::new(1));

        let error = run_live_discovery(&runtime, &step, "Rewrite MMAT", 1)
            .await
            .expect_err("discovery should fail");

        assert!(error.to_string().contains("produced no further questions"));
    }

    #[test]
    fn discovery_prompt_includes_previous_answers() {
        let prompt = build_discovery_prompt(&DiscoveryInput {
            initial_prompt: "Rewrite MMAT".to_string(),
            turn: 1,
            clarification_budget: 3,
            answers: vec![DiscoveryAnswer {
                question: "What should back metadata?".to_string(),
                answer: "SQLite".to_string(),
            }],
            findings: vec![DiscoveryFinding::MissingConstraints],
            prior_state: Some(ready_state()),
        });

        assert!(prompt.contains("Rewrite MMAT"));
        assert!(prompt.contains("What should back metadata? => SQLite"));
        assert!(prompt.contains("Build a workflow rewrite"));
        assert!(prompt.contains("missing constraints"));
    }

    #[tokio::test]
    async fn discovery_step_records_retry_findings_until_acceptance() {
        let runtime = ScriptedRuntime::new(["SQLite"]);
        let agent = Arc::new(StubDiscoveryAgent::new(vec![
            DiscoveryState {
                ready_for_solution: false,
                problem_statement: "Rewrite MMAT".to_string(),
                goals: vec!["Keep the workflow shape".to_string()],
                constraints: Vec::new(),
                assumptions: Vec::new(),
                risks: Vec::new(),
                notes: Vec::new(),
                recommended_path: "Clarify persistence".to_string(),
                open_questions: vec![DiscoveryQuestion {
                    prompt: "What should store metadata?".to_string(),
                    choices: vec!["SQLite".to_string(), "Filesystem".to_string()],
                }],
            },
            DiscoveryState {
                ready_for_solution: true,
                problem_statement: "Rewrite MMAT".to_string(),
                goals: vec!["Keep the workflow shape".to_string()],
                constraints: vec!["SQLite metadata".to_string()],
                assumptions: Vec::new(),
                risks: Vec::new(),
                notes: Vec::new(),
                recommended_path: "Generate branches".to_string(),
                open_questions: Vec::new(),
            },
        ]));
        let step = build_turn_step(agent, RetryPolicy::new(3));

        let traced = step
            .run_traced(
                &runtime,
                DiscoveryInput::new("Rewrite MMAT").with_clarification_budget(3),
            )
            .await
            .expect("discovery should recover");

        assert_eq!(traced.report().attempt_count(), 2);
        assert_eq!(
            traced.report().attempts()[0].findings,
            vec![
                DiscoveryFinding::MissingConstraints,
                DiscoveryFinding::UnresolvedBlockingAmbiguity,
            ]
        );
        assert!(traced.report().attempts()[1].accepted());
    }
}
