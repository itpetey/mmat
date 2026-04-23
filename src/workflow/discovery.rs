use std::sync::Arc;

use futures::future::LocalBoxFuture;
use naaf_core::{Step, TaskExt, task_fn};
use naaf_llm::{HumanIO, HumanQuestion};
use serde::{Deserialize, Serialize};

use crate::workflow::WorkflowError;

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
    pub answers: Vec<DiscoveryAnswer>,
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

impl DiscoveryInput {
    pub fn new(initial_prompt: impl Into<String>) -> Self {
        Self {
            initial_prompt: initial_prompt.into(),
            turn: 0,
            answers: Vec::new(),
            prior_state: None,
        }
    }
}

pub fn build_discovery_prompt(input: &DiscoveryInput) -> String {
    let mut lines = vec![
        "You are the discovery stage for MMAT.".to_string(),
        format!("Initial prompt: {}", input.initial_prompt),
        format!("Discovery turn: {}", input.turn + 1),
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

    lines.join("\n")
}

pub fn build_turn_step<R: 'static, A>(
    agent: Arc<A>,
) -> Step<R, DiscoveryInput, DiscoveryState, (), WorkflowError>
where
    A: DiscoveryTurnAgent<R>,
{
    Step::builder(
        task_fn(move |runtime: &R, input: DiscoveryInput| {
            let agent = agent.clone();
            let prompt = build_discovery_prompt(&input);
            Box::pin(async move { agent.run_turn(runtime, input, prompt).await })
        })
        .observed_as("discovery_turn"),
    )
    .with_findings::<()>()
    .build()
}

pub async fn run_live_discovery<R>(
    runtime: &R,
    step: &Step<R, DiscoveryInput, DiscoveryState, (), WorkflowError>,
    initial_prompt: impl Into<String>,
    max_turns: usize,
) -> Result<DiscoveryOutcome, WorkflowError>
where
    R: HumanIO<Error = WorkflowError> + 'static,
{
    let mut input = DiscoveryInput::new(initial_prompt);

    for turn in 0..max_turns {
        input.turn = turn;
        let state = step.run(runtime, input.clone()).await.map_err(|error| {
            WorkflowError::Discovery(format!("discovery step execution failed: {error}"))
        })?;

        if state.ready_for_solution {
            return Ok(DiscoveryOutcome {
                state,
                answers: input.answers,
            });
        }

        if state.open_questions.is_empty() {
            return Err(WorkflowError::Discovery(
                "discovery is not solution-ready and produced no further questions".to_string(),
            ));
        }

        let mut answers = input.answers.clone();
        for question in &state.open_questions {
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

        input = DiscoveryInput {
            initial_prompt: input.initial_prompt.clone(),
            turn: turn + 1,
            answers,
            prior_state: Some(state),
        };
    }

    Err(WorkflowError::Discovery(format!(
        "discovery exceeded the configured turn budget of {max_turns}"
    )))
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Arc};

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
            ready_state(),
        ]));
        let step = build_turn_step(agent.clone());

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
        let step = build_turn_step(agent);

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
            answers: vec![DiscoveryAnswer {
                question: "What should back metadata?".to_string(),
                answer: "SQLite".to_string(),
            }],
            prior_state: Some(ready_state()),
        });

        assert!(prompt.contains("Rewrite MMAT"));
        assert!(prompt.contains("What should back metadata? => SQLite"));
        assert!(prompt.contains("Build a workflow rewrite"));
    }
}
