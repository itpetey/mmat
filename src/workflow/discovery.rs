use std::fmt::{Debug, Display};

use futures::future;
use naaf_core::{Attempt, RetryPolicy, Step, check_fn, repair_fn};
use naaf_llm::{HumanIO, HumanQuestion, LlmAgent, LlmClient, TaskError};
use serde::{Deserialize, Serialize};

use crate::workflow::parser::decode_outcome;

type DiscoveryStep<C, R, E> =
    Step<R, DiscoveryInput, DiscoveryOutput, DiscoveryFinding, DiscoveryStepError<C, R, E>>;
type DiscoveryStepError<C, R, E> =
    TaskError<<R as HumanIO>::Error, <C as LlmClient>::Error, E, serde_json::Error>;

pub const MODEL: &str = "qwen/qwen3.6-35b-a3b";
pub const SYSTEM_PROMPT: &str = "You are a curious sounding board for new ideas. Your job is to interrogate the idea, fleshing out any unknowns, researching prior art, and soliciting feedback from the user.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DiscoveryInput {
    initial_prompt: String,
    answers: Vec<DiscoveryAnswer>,
    findings: Vec<DiscoveryFinding>,
    last_output: Option<DiscoveryOutput>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DiscoveryOutput {
    pub(super) ready_for_solution: bool,
    pub(super) problem_statement: String,
    pub(super) goals: Vec<String>,
    pub(super) constraints: Vec<String>,
    #[serde(default)]
    pub(super) assumptions: Vec<String>,
    #[serde(default)]
    pub(super) risks: Vec<String>,
    #[serde(default)]
    pub(super) notes: Vec<String>,
    pub(super) recommended_path: String,
    pub(super) open_questions: Vec<DiscoveryQuestion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DiscoveryQuestion {
    pub(super) prompt: String,
    #[serde(default)]
    pub(super) choices: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DiscoveryAnswer {
    pub(super) question: String,
    pub(super) answer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum DiscoveryFinding {
    MissingProblemStatement,
    MissingGoals,
    MissingConstraints,
    UnresolvedBlockingAmbiguity,
    NoClarificationQuestions,
}

impl DiscoveryOutput {
    #[cfg(test)]
    pub fn is_ready(&self) -> bool {
        self.ready_for_solution
            && self.open_questions.is_empty()
            && !self.problem_statement.trim().is_empty()
            && !self.goals.iter().any(|item| item.trim().is_empty())
            && !self.constraints.iter().any(|item| item.trim().is_empty())
    }
}

impl DiscoveryInput {
    pub(super) fn new(initial_prompt: impl Into<String>) -> Self {
        Self {
            initial_prompt: initial_prompt.into(),
            answers: Vec::new(),
            findings: Vec::new(),
            last_output: None,
        }
    }
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

pub(super) fn step<C, R, E>(agent: &LlmAgent<C, R, E>) -> DiscoveryStep<C, R, E>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    Step::builder(agent.json_task(
        MODEL.into(),
        SYSTEM_PROMPT.into(),
        |i| Ok::<_, R::Error>(build_prompt(i)),
        decode_outcome,
        "discovery-turn".into(),
    ))
    .validate(check_fn(|r, _, o| Box::pin(future::ok(validate(r, o)))))
    .repair_with(repair_fn(|r, a| {
        Box::pin(async move { repair(r, a).await.map_err(TaskError::Build) })
    }))
    .retry_policy(RetryPolicy::unlimited())
    .build_persistent()
}

fn build_prompt(input: DiscoveryInput) -> String {
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
        "Return only one JSON object. Do not include markdown, prose, code fences, or hidden reasoning in the assistant content."
            .to_string(),
    );
    lines.push(
        "The JSON object must use this exact shape: {\"ready_for_solution\":boolean,\"problem_statement\":string,\"goals\":string[],\"constraints\":string[],\"assumptions\":string[],\"risks\":string[],\"notes\":string[],\"recommended_path\":string,\"open_questions\":[{\"prompt\":string,\"choices\":string[]}]}."
            .to_string(),
    );
    lines.push(
        "Include explicit uncertainty in assumptions, risks, notes, or open_questions as appropriate."
            .to_string(),
    );
    lines.push(
        "Only mark ready_for_solution true when the hand-off is complete enough for solution generation without blocking ambiguity."
            .to_string(),
    );

    lines.join("\n")
}

async fn repair<R>(
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

fn validate<R>(_runtime: &R, output: DiscoveryOutput) -> Vec<DiscoveryFinding> {
    let mut findings = Vec::new();

    if output.problem_statement.trim().is_empty() {
        findings.push(DiscoveryFinding::MissingProblemStatement);
    }

    if output.goals.iter().all(|item| item.trim().is_empty()) {
        findings.push(DiscoveryFinding::MissingGoals);
    }

    if output.constraints.iter().all(|item| item.trim().is_empty()) {
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

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, convert::Infallible, sync::Arc};

    use futures::future::LocalBoxFuture;
    use naaf_llm::{AssistantMessage, CompletionRequest, CompletionResponse, HumanAnswer, Message};
    use parking_lot::Mutex;

    use super::*;

    #[derive(Clone)]
    struct ScriptedClient {
        responses: Arc<Mutex<VecDeque<String>>>,
        prompts: Arc<Mutex<Vec<String>>>,
    }

    struct AnsweringRuntime {
        answers: Mutex<VecDeque<String>>,
        questions: Mutex<Vec<String>>,
    }

    impl ScriptedClient {
        fn new(responses: impl IntoIterator<Item = String>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
                prompts: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().clone()
        }
    }

    impl AnsweringRuntime {
        fn new(answers: impl IntoIterator<Item = String>) -> Self {
            Self {
                answers: Mutex::new(answers.into_iter().collect()),
                questions: Mutex::new(Vec::new()),
            }
        }

        fn questions(&self) -> Vec<String> {
            self.questions.lock().clone()
        }
    }

    impl LlmClient for ScriptedClient {
        type Error = Infallible;
        type Runtime = AnsweringRuntime;

        fn complete<'a>(
            &'a self,
            _runtime: &'a Self::Runtime,
            request: CompletionRequest,
        ) -> LocalBoxFuture<'a, Result<CompletionResponse, Self::Error>> {
            let prompt = request
                .messages
                .iter()
                .filter_map(|message| match message {
                    Message::User { content } => Some(content.clone()),
                    _ => None,
                })
                .next_back()
                .expect("request should include a user prompt");
            self.prompts.lock().push(prompt);
            let response = self
                .responses
                .lock()
                .pop_front()
                .expect("scripted response should exist");

            Box::pin(async move {
                Ok(CompletionResponse::new(AssistantMessage::from_text(
                    response,
                )))
            })
        }
    }

    impl HumanIO for AnsweringRuntime {
        type Error = Infallible;

        fn ask<'a>(
            &'a self,
            question: HumanQuestion,
        ) -> LocalBoxFuture<'a, Result<HumanAnswer, Self::Error>> {
            self.questions.lock().push(question.question);
            let answer = self
                .answers
                .lock()
                .pop_front()
                .expect("scripted answer should exist");

            Box::pin(async move { Ok(HumanAnswer { content: answer }) })
        }
    }

    fn complete_output() -> DiscoveryOutput {
        DiscoveryOutput {
            ready_for_solution: true,
            problem_statement: "Rewrite MMAT".to_string(),
            goals: vec!["Keep the workflow inspectable".to_string()],
            constraints: vec!["Use local models".to_string()],
            assumptions: vec!["LM Studio is running".to_string()],
            risks: vec!["Local model output may drift".to_string()],
            notes: Vec::new(),
            recommended_path: "Proceed to knowledge planning".to_string(),
            open_questions: Vec::new(),
        }
    }

    fn incomplete_output() -> DiscoveryOutput {
        DiscoveryOutput {
            ready_for_solution: false,
            problem_statement: "The idea is not yet clear".to_string(),
            goals: vec!["Clarify the idea".to_string()],
            constraints: vec!["Need user input".to_string()],
            assumptions: Vec::new(),
            risks: Vec::new(),
            notes: Vec::new(),
            recommended_path: "Ask a focused question".to_string(),
            open_questions: vec![DiscoveryQuestion {
                prompt: "What are we building?".to_string(),
                choices: Vec::new(),
            }],
        }
    }

    #[test]
    fn prompt_requires_json_object() {
        let prompt = build_prompt(DiscoveryInput::new("Hi"));

        assert!(prompt.contains("Return only one JSON object"));
        assert!(prompt.contains("\"ready_for_solution\":boolean"));
        assert!(prompt.contains("\"open_questions\""));
        assert!(prompt.contains("Do not include markdown"));
    }

    #[test]
    fn validation_accepts_non_empty_goals_and_constraints() {
        let findings = validate(&(), complete_output());

        assert!(findings.is_empty());
    }

    #[test]
    fn validation_reports_missing_goals_and_constraints() {
        let mut output = complete_output();
        output.goals = vec![String::new()];
        output.constraints = Vec::new();

        let findings = validate(&(), output);

        assert!(findings.contains(&DiscoveryFinding::MissingGoals));
        assert!(findings.contains(&DiscoveryFinding::MissingConstraints));
    }

    #[test]
    fn discovery_output_defaults_missing_optional_handoff_fields() {
        let output: DiscoveryOutput = serde_json::from_str(
            r#"{
                "ready_for_solution": true,
                "problem_statement": "Rewrite MMAT",
                "goals": ["Keep the workflow inspectable"],
                "constraints": ["Use local models"],
                "recommended_path": "Proceed",
                "open_questions": [{"prompt": "Any missing choices?"}]
            }"#,
        )
        .expect("missing handoff lists should default");

        assert_eq!(output.assumptions, Vec::<String>::new());
        assert_eq!(output.risks, Vec::<String>::new());
        assert_eq!(output.notes, Vec::<String>::new());
        assert_eq!(output.open_questions[0].choices, Vec::<String>::new());
    }

    #[tokio::test]
    async fn discovery_repairs_with_human_answers_before_rejecting() {
        let client = ScriptedClient::new(vec![
            serde_json::to_string(&incomplete_output()).expect("output should serialise"),
            serde_json::to_string(&complete_output()).expect("output should serialise"),
        ]);
        let agent = LlmAgent::new(client.clone());
        let runtime = AnsweringRuntime::new(["A local-first workflow tool".to_string()]);

        let output = step(&agent)
            .run(&runtime, DiscoveryInput::new("Hi"))
            .await
            .expect("discovery should repair using human answers");

        assert_eq!(output, complete_output());
        assert_eq!(
            runtime.questions(),
            vec!["What are we building?".to_string()]
        );

        let prompts = client.prompts();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].contains("Answered clarifications:"));
        assert!(prompts[1].contains("What are we building? => A local-first workflow tool"));
    }
}
