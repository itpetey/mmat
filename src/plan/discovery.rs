use std::{
    convert::Infallible,
    fmt::{Debug, Display},
    path::PathBuf,
};

#[cfg(test)]
use naaf_llm::ExecutionOutcome;
use futures::future;
use naaf_core::{Attempt, RetryPolicy, Step, check_fn, repair_fn, task_fn};
use naaf_llm::{
    AdaptorError, CompletionRequest, Executor, HumanIO, HumanQuestion, LlmAgent, LlmClient,
    Message, TaskError, ToolRegistry,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::plan::{
    WorkflowBuildError, WorkflowTaskError, execute_with_turn_limit_retry,
    input_token_budget_for_model, parser::decode_outcome,
};

type DiscoveryStep<C, R, E> =
    Step<R, DiscoveryInput, DiscoveryOutput, DiscoveryFinding, DiscoveryStepError<C, R, E>>;
type DiscoveryStepError<C, R, E> = WorkflowTaskError<C, R, E>;

pub const MODEL: &str = "gpt-5.5";
pub const SYSTEM_PROMPT: &str = "You are a curious sounding board for new ideas. Your job is to interrogate the idea, fleshing out any unknowns, researching prior art, and soliciting feedback from the user.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiscoveryQuestion {
    #[serde(default)]
    pub(crate) prompt: String,
    #[serde(default)]
    pub(crate) choices: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SubDomainSuggestion {
    pub(crate) name: String,
    pub(crate) description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiscoveryOutput {
    #[serde(default)]
    pub(crate) assistant_message: String,
    pub(crate) ready_for_solution: bool,
    pub(crate) problem_statement: String,
    pub(crate) goals: Vec<String>,
    pub(crate) constraints: Vec<String>,
    #[serde(default)]
    pub(crate) assumptions: Vec<String>,
    #[serde(default)]
    pub(crate) risks: Vec<String>,
    #[serde(default)]
    pub(crate) notes: Vec<String>,
    pub(crate) recommended_path: String,
    pub(crate) open_questions: Vec<DiscoveryQuestion>,
    #[serde(default)]
    pub(crate) sub_domains: Vec<SubDomainSuggestion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiscoveryAnswer {
    pub(crate) question: String,
    pub(crate) answer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DiscoveryFinding {
    MissingProblemStatement,
    MissingGoals,
    MissingConstraints,
    UnresolvedBlockingAmbiguity,
    NoClarificationQuestions,
}

/// Internal task output that carries the conversation turn so it can be accumulated
/// across repair attempts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DiscoveryTaskOutput {
    pub(crate) output: DiscoveryOutput,
    pub(crate) conversation_turn: Vec<Message>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DiscoveryInput {
    initial_prompt: String,
    answers: Vec<DiscoveryAnswer>,
    findings: Vec<DiscoveryFinding>,
    last_output: Option<DiscoveryOutput>,
    messages: Vec<Message>,
    turn_count: usize,
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

impl Display for DiscoveryFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProblemStatement => write!(f, "missing problem statement"),
            Self::MissingGoals => write!(f, "missing goals"),
            Self::MissingConstraints => write!(f, "missing constraints"),
            Self::UnresolvedBlockingAmbiguity => {
                write!(f, "discovery is still in progress")
            }
            Self::NoClarificationQuestions => {
                write!(f, "no clarification questions despite not being ready")
            }
        }
    }
}

impl DiscoveryInput {
    pub(super) fn new(initial_prompt: impl Into<String>) -> Self {
        Self {
            initial_prompt: initial_prompt.into(),
            answers: Vec::new(),
            findings: Vec::new(),
            last_output: None,
            messages: Vec::new(),
            turn_count: 0,
        }
    }
}

#[cfg(test)]
pub(super) fn step<C, R, E>(agent: &LlmAgent<C, R, E>) -> DiscoveryStep<C, R, E>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    Step::builder(agent.task(
        |_runtime: &R, input: DiscoveryInput| {
            Ok::<_, WorkflowBuildError<R::Error>>(build_request(&input, None))
        },
        |outcome: ExecutionOutcome| {
            let output = decode_outcome(outcome.clone())?;
            let conversation_turn = outcome.messages().to_vec();
            Ok(DiscoveryTaskOutput {
                output,
                conversation_turn,
            })
        },
    ))
    .validate(check_fn(
        |_r: &R, _input: DiscoveryInput, o: DiscoveryTaskOutput| {
            Box::pin(future::ok(validate(_r, &o.output)))
        },
    ))
    .repair_with(repair_fn(|r, a| {
        Box::pin(async move {
            repair(r, a)
                .await
                .map_err(|error| TaskError::Build(WorkflowBuildError::Human(error)))
        })
    }))
    .retry_policy(RetryPolicy::unlimited())
    .build_persistent()
    .map(|task_output| task_output.output)
}

pub(super) fn step_with_repository_tools<C, R, E>(
    agent: &LlmAgent<C, R, E>,
    workspace_root: PathBuf,
) -> DiscoveryStep<C, R, E>
where
    C: LlmClient<Runtime = R> + Clone + 'static,
    C::Error: Debug + Display + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    let client = (*agent.executor().client()).clone();
    let system_prompt = format!(
        "{}\n\nYou have access to repository tools rooted at the selected project: `glob_paths`, `search_files`, and `read_file`. Use them before making claims about existing project code, structure, documentation, or dependencies.",
        SYSTEM_PROMPT
    );

    let task = task_fn(move |runtime: &R, input: DiscoveryInput| {
        let client = client.clone();
        let system_prompt = system_prompt.clone();
        let workspace_root = workspace_root.clone();
        Box::pin(async move {
            let request = build_request(&input, Some(&system_prompt));

            let mut tools = ToolRegistry::<R, Infallible>::new();
            register_repository_tools(&mut tools, workspace_root);
            let executor = Executor::with_tools(client, tools);
            let outcome = execute_with_turn_limit_retry(&executor, runtime, request)
                .await
                .map_err(|error| {
                    AdaptorError::Build(WorkflowBuildError::Workflow(format!(
                        "executor failed: {error}"
                    )))
                })?;

            let output: DiscoveryOutput =
                decode_outcome(outcome.clone()).map_err(AdaptorError::Decode)?;

            let input_message_count = input.messages.len();
            let conversation_turn = outcome
                .messages()
                .iter()
                .skip(input_message_count)
                .cloned()
                .collect();

            Ok(DiscoveryTaskOutput {
                output,
                conversation_turn,
            })
        })
    });

    Step::builder(task)
        .validate(check_fn(
            |_r: &R, _input: DiscoveryInput, o: DiscoveryTaskOutput| {
                Box::pin(future::ok(validate(_r, &o.output)))
            },
        ))
        .repair_with(repair_fn(|r, a| {
            Box::pin(async move {
                repair(r, a)
                    .await
                    .map_err(|error| TaskError::Build(WorkflowBuildError::Human(error)))
            })
        }))
        .retry_policy(RetryPolicy::unlimited())
        .build_persistent()
        .map(|task_output| task_output.output)
}

fn build_initial_user_message(input: &DiscoveryInput) -> String {
    let mut lines = vec![format!("Initial prompt: {}", input.initial_prompt)];

    lines.push(String::new());
    lines.push(
        "Return only one JSON object. Do not include markdown, prose, code fences, or hidden reasoning in the assistant content."
            .to_string(),
    );
    lines.push(
        "The JSON object must use this exact shape: {\"assistant_message\":string,\"ready_for_solution\":boolean,\"problem_statement\":string,\"goals\":string[],\"constraints\":string[],\"assumptions\":string[],\"risks\":string[],\"notes\":string[],\"recommended_path\":string,\"open_questions\":[{\"prompt\":string,\"choices\":string[]}],\"sub_domains\":[{\"name\":string,\"description\":string}]}"
            .to_string(),
    );
    lines.push(
        "Use assistant_message for concise, conversational exploration that can be shown to the user before the next question."
            .to_string(),
    );
    lines.push(
        "Keep ready_for_solution false while exploring, and ask focused open_questions that guide the user toward a concrete, solutionable problem statement."
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
    lines.push(
        "If the problem is broad enough to benefit from decomposition, include up to 5 sub_domains. Each sub_domain should be a distinct, independently implementable part of the overall system. Leave sub_domains empty when the problem is already concrete enough for a single solution."
            .to_string(),
    );

    lines.join("\n")
}

fn build_request(
    input: &DiscoveryInput,
    system_prompt_override: Option<&str>,
) -> CompletionRequest {
    let mut messages = input.messages.clone();
    let max_input_tokens = input_token_budget_for_model(MODEL);
    maybe_compact_messages(&mut messages, max_input_tokens);

    if messages.is_empty() {
        let base = system_prompt_override.unwrap_or(SYSTEM_PROMPT);
        messages.push(Message::system(build_system_prompt_with_base(
            input.turn_count,
            base,
        )));
        messages.push(Message::user(build_initial_user_message(input)));
    } else {
        messages.push(Message::user(build_turn_instructions(input)));
    }

    CompletionRequest::new(MODEL.to_string(), messages).with_metadata(json!({
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "discovery_output",
                "strict": false,
                "schema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": true
                }
            }
        }
    }))
}

fn build_system_prompt_with_base(turn_count: usize, base: &str) -> String {
    let phase_hint = if turn_count == 0 {
        "This is the first turn. Begin by understanding the user's idea and asking focused clarifying questions."
    } else {
        "This is a continuation of an ongoing conversation. Do not re-summarise the problem statement, goals, or constraints unless they have changed. Focus on what is new and respond to the user's latest answers."
    };
    format!(
        "{base}\n\n{phase_hint}\n\nDo not repeat information the user has already seen. Keep responses concise and forward-moving. It is normal to take many discovery turns."
    )
}

fn build_turn_instructions(input: &DiscoveryInput) -> String {
    let mut lines = vec![
        "Continue the discovery conversation based on what has been discussed so far.".to_string(),
    ];

    if !input.findings.is_empty() {
        lines.push(String::new());
        lines.push("Address these validation findings in your response:".to_string());
        lines.extend(
            input
                .findings
                .iter()
                .map(|finding| format!("- {}", finding)),
        );
    }

    lines.push(String::new());
    lines.push(
        "Return only one JSON object with the same shape as before. Only update fields if they have changed based on the latest user answers."
            .to_string(),
    );

    lines.join("\n")
}

fn maybe_compact_messages(messages: &mut Vec<Message>, max_input_tokens: usize) {
    let total_tokens: usize = messages.iter().map(|m| m.token_count()).sum();
    if total_tokens <= max_input_tokens {
        return;
    }

    let system = messages
        .first()
        .cloned()
        .filter(|m| matches!(m, Message::System { .. }));
    let system_end = system.as_ref().map_or(0, |_| 1);

    // Reserve tokens for the summary message itself.
    const SUMMARY_RESERVE: usize = 30;
    let budget = max_input_tokens
        .saturating_sub(system.as_ref().map_or(0, |m| m.token_count()))
        .saturating_sub(SUMMARY_RESERVE);

    // Work backwards from the end to find the oldest message we can keep
    // while staying within the token budget.
    let mut suffix_tokens = 0usize;
    let mut split_point = messages.len();

    for i in (system_end..messages.len()).rev() {
        let msg_tokens = messages[i].token_count();
        if suffix_tokens + msg_tokens > budget {
            split_point = i + 1;
            break;
        }
        suffix_tokens += msg_tokens;
    }

    // Never split an assistant + tool_results group. If split_point points into
    // tool results, move it back to include the preceding assistant so every
    // tool message in the kept suffix has its matching assistant with tool_calls.
    if split_point > system_end && matches!(messages.get(split_point), Some(Message::Tool(_))) {
        if let Some(assistant_idx) = messages[..split_point]
            .iter()
            .rposition(|m| matches!(m, Message::Assistant(_)))
        {
            split_point = assistant_idx;
        } else {
            // No assistant found before this tool result – cannot safely compact.
            return;
        }
    }

    if split_point <= system_end {
        return;
    }

    let middle = &messages[system_end..split_point];
    let summary = format!(
        "[Earlier conversation: {} messages covering the initial idea and approximately {} clarification turns.]",
        middle.len(),
        middle.len() / 2
    );

    let mut compacted = Vec::new();
    if let Some(sys) = system {
        compacted.push(sys);
    }
    compacted.push(Message::user(summary));
    compacted.extend(messages[split_point..].iter().cloned());

    *messages = compacted;
}

fn register_repository_tools<R>(tools: &mut ToolRegistry<R, Infallible>, workspace_root: PathBuf)
where
    R: 'static,
{
    if let Err(error) = tools.register(naaf_llm::repository::ReadFileTool::<R>::new(
        workspace_root.clone(),
    )) {
        tracing::warn!(%error, "failed to register repository read tool");
    }
    if let Err(error) = tools.register(naaf_llm::repository::GlobPathsTool::<R>::new(
        workspace_root.clone(),
    )) {
        tracing::warn!(%error, "failed to register repository glob tool");
    }
    if let Err(error) = tools.register(naaf_llm::repository::SearchFilesTool::<R>::new(
        workspace_root,
    )) {
        tracing::warn!(%error, "failed to register repository search tool");
    }
}

async fn repair<R>(
    runtime: &R,
    attempts: Vec<Attempt<DiscoveryInput, DiscoveryTaskOutput, DiscoveryFinding>>,
) -> Result<DiscoveryInput, R::Error>
where
    R: HumanIO + 'static,
{
    let latest_attempt = attempts
        .last()
        .expect("discovery repair requires an attempt");
    let mut messages = latest_attempt.input.messages.clone();

    // Append the assistant response and any tool results from the latest turn.
    messages.extend(latest_attempt.output.conversation_turn.clone());

    let mut answers = latest_attempt.input.answers.clone();

    for (index, question) in latest_attempt
        .output
        .output
        .open_questions
        .iter()
        .filter(|q| !q.prompt.trim().is_empty())
        .enumerate()
    {
        let display_question = if index == 0
            && !latest_attempt
                .output
                .output
                .assistant_message
                .trim()
                .is_empty()
        {
            format!(
                "{}\n\n{}",
                latest_attempt.output.output.assistant_message.trim(),
                question.prompt
            )
        } else {
            question.prompt.clone()
        };
        let reply = runtime
            .ask(HumanQuestion {
                question: display_question,
                choices: if question.choices.is_empty() {
                    None
                } else {
                    Some(question.choices.clone())
                },
            })
            .await?;
        answers.push(DiscoveryAnswer {
            question: question.prompt.clone(),
            answer: reply.content.clone(),
        });
        messages.push(Message::user(reply.content));
    }

    Ok(DiscoveryInput {
        initial_prompt: latest_attempt.input.initial_prompt.clone(),
        answers,
        findings: latest_attempt.findings.clone(),
        last_output: Some(latest_attempt.output.output.clone()),
        messages,
        turn_count: latest_attempt.input.turn_count + 1,
    })
}

fn validate<R>(_runtime: &R, output: &DiscoveryOutput) -> Vec<DiscoveryFinding> {
    let mut findings = Vec::new();

    if output.ready_for_solution {
        if output.problem_statement.trim().is_empty() {
            findings.push(DiscoveryFinding::MissingProblemStatement);
        }

        if output.goals.iter().all(|item| item.trim().is_empty()) {
            findings.push(DiscoveryFinding::MissingGoals);
        }

        if output.constraints.iter().all(|item| item.trim().is_empty()) {
            findings.push(DiscoveryFinding::MissingConstraints);
        }
    }

    if !output.ready_for_solution || !output.open_questions.is_empty() {
        findings.push(DiscoveryFinding::UnresolvedBlockingAmbiguity);
    }

    let has_meaningful_questions = output
        .open_questions
        .iter()
        .any(|q| !q.prompt.trim().is_empty());

    if !output.ready_for_solution && !has_meaningful_questions {
        findings.push(DiscoveryFinding::NoClarificationQuestions);
    }

    findings
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, convert::Infallible, fs, sync::Arc};

    use futures::future::LocalBoxFuture;
    use naaf_llm::{
        AssistantMessage, CompletionRequest, CompletionResponse, HumanAnswer, Message, ToolCall,
    };
    use parking_lot::Mutex;

    use super::*;

    #[derive(Clone)]
    struct ScriptedClient {
        responses: Arc<Mutex<VecDeque<AssistantMessage>>>,
        prompts: Arc<Mutex<Vec<String>>>,
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
    }

    struct AnsweringRuntime {
        answers: Mutex<VecDeque<String>>,
        questions: Mutex<Vec<String>>,
    }

    impl ScriptedClient {
        fn new(responses: impl IntoIterator<Item = String>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(
                    responses
                        .into_iter()
                        .map(AssistantMessage::from_text)
                        .collect(),
                )),
                prompts: Arc::new(Mutex::new(Vec::new())),
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn with_messages(responses: impl IntoIterator<Item = AssistantMessage>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
                prompts: Arc::new(Mutex::new(Vec::new())),
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn requests(&self) -> Vec<CompletionRequest> {
            self.requests.lock().clone()
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
            self.requests.lock().push(request);
            let response = self
                .responses
                .lock()
                .pop_front()
                .expect("scripted response should exist");

            Box::pin(async move { Ok(CompletionResponse::new(response)) })
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
            assistant_message: "I have enough context to hand this off.".to_string(),
            ready_for_solution: true,
            problem_statement: "Rewrite MMAT".to_string(),
            goals: vec!["Keep the plan inspectable".to_string()],
            constraints: vec!["Use local models".to_string()],
            assumptions: vec!["LM Studio is running".to_string()],
            risks: vec!["Local model output may drift".to_string()],
            notes: Vec::new(),
            recommended_path: "Proceed to knowledge planning".to_string(),
            open_questions: Vec::new(),
            sub_domains: Vec::new(),
        }
    }

    fn incomplete_output() -> DiscoveryOutput {
        DiscoveryOutput {
            assistant_message: "I need to understand the idea a bit more.".to_string(),
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
            sub_domains: Vec::new(),
        }
    }

    #[test]
    fn prompt_requires_json_object() {
        let prompt = build_initial_user_message(&DiscoveryInput::new("Hi"));

        assert!(prompt.contains("Return only one JSON object"));
        assert!(prompt.contains("\"assistant_message\":string"));
        assert!(prompt.contains("\"ready_for_solution\":boolean"));
        assert!(prompt.contains("\"open_questions\""));
        assert!(prompt.contains("Do not include markdown"));
    }

    #[test]
    fn validation_accepts_non_empty_goals_and_constraints() {
        let findings = validate(&(), &complete_output());

        assert!(findings.is_empty());
    }

    #[test]
    fn validation_reports_missing_goals_and_constraints() {
        let mut output = complete_output();
        output.goals = vec![String::new()];
        output.constraints = Vec::new();

        let findings = validate(&(), &output);

        assert!(findings.contains(&DiscoveryFinding::MissingGoals));
        assert!(findings.contains(&DiscoveryFinding::MissingConstraints));
    }

    #[test]
    fn validation_allows_incomplete_handoff_while_discovery_continues() {
        let mut output = incomplete_output();
        output.problem_statement = String::new();
        output.goals = Vec::new();
        output.constraints = Vec::new();

        let findings = validate(&(), &output);

        assert!(!findings.contains(&DiscoveryFinding::MissingProblemStatement));
        assert!(!findings.contains(&DiscoveryFinding::MissingGoals));
        assert!(!findings.contains(&DiscoveryFinding::MissingConstraints));
        assert!(findings.contains(&DiscoveryFinding::UnresolvedBlockingAmbiguity));
    }

    #[test]
    fn discovery_output_defaults_missing_optional_handoff_fields() {
        let output: DiscoveryOutput = serde_json::from_str(
            r#"{
                "ready_for_solution": true,
                "problem_statement": "Rewrite MMAT",
                "goals": ["Keep the plan inspectable"],
                "constraints": ["Use local models"],
                "recommended_path": "Proceed",
                "open_questions": [{"prompt": "Any missing choices?"}]
            }"#,
        )
        .expect("missing handoff lists should default");

        assert_eq!(output.assumptions, Vec::<String>::new());
        assert_eq!(output.risks, Vec::<String>::new());
        assert_eq!(output.notes, Vec::<String>::new());
        assert_eq!(output.assistant_message, String::new());
        assert_eq!(output.open_questions[0].choices, Vec::<String>::new());
    }

    #[test]
    fn discovery_output_deserialises_when_question_prompt_is_missing() {
        let output: DiscoveryOutput = serde_json::from_str(
            r#"{
                "ready_for_solution": false,
                "problem_statement": "Rewrite MMAT",
                "goals": ["Keep the plan inspectable"],
                "constraints": ["Use local models"],
                "recommended_path": "Proceed",
                "open_questions": [{"choices": ["a", "b"]}]
            }"#,
        )
        .expect("missing question prompt should default to empty string");

        assert_eq!(output.open_questions[0].prompt, String::new());
        assert_eq!(output.open_questions[0].choices, vec!["a", "b"]);
    }

    #[test]
    fn validation_reports_no_clarification_questions_when_all_prompts_are_empty() {
        let mut output = incomplete_output();
        output.open_questions = vec![DiscoveryQuestion {
            prompt: String::new(),
            choices: vec!["a".to_string()],
        }];

        let findings = validate(&(), &output);

        assert!(findings.contains(&DiscoveryFinding::NoClarificationQuestions));
    }

    #[tokio::test]
    async fn repair_skips_open_questions_with_empty_prompts() {
        let mut first_output = incomplete_output();
        first_output.open_questions = vec![
            DiscoveryQuestion {
                prompt: String::new(),
                choices: Vec::new(),
            },
            DiscoveryQuestion {
                prompt: "What are we building?".to_string(),
                choices: Vec::new(),
            },
        ];
        let client = ScriptedClient::new(vec![
            serde_json::to_string(&first_output).expect("output should serialise"),
            serde_json::to_string(&complete_output()).expect("output should serialise"),
        ]);
        let agent = LlmAgent::new(client.clone());
        let runtime = AnsweringRuntime::new(["A local-first plan tool".to_string()]);

        let output = step(&agent)
            .run(&runtime, DiscoveryInput::new("Hi"))
            .await
            .expect("discovery should repair using human answers");

        assert_eq!(output, complete_output());
        assert_eq!(
            runtime.questions(),
            vec!["I need to understand the idea a bit more.\n\nWhat are we building?".to_string()]
        );
    }

    #[tokio::test]
    async fn discovery_repairs_with_human_answers_before_rejecting() {
        let client = ScriptedClient::new(vec![
            serde_json::to_string(&incomplete_output()).expect("output should serialise"),
            serde_json::to_string(&complete_output()).expect("output should serialise"),
        ]);
        let agent = LlmAgent::new(client.clone());
        let runtime = AnsweringRuntime::new(["A local-first plan tool".to_string()]);

        let output = step(&agent)
            .run(&runtime, DiscoveryInput::new("Hi"))
            .await
            .expect("discovery should repair using human answers");

        assert_eq!(output, complete_output());
        assert_eq!(
            runtime.questions(),
            vec!["I need to understand the idea a bit more.\n\nWhat are we building?".to_string()]
        );

        let requests = client.requests();
        assert_eq!(requests.len(), 2);

        // First request: system + initial user message.
        assert_eq!(requests[0].messages.len(), 2);
        assert!(matches!(requests[0].messages[0], Message::System { .. }));

        // Second request should contain the conversation history with the user's answer.
        let second = &requests[1];
        assert!(second.messages.len() > 2);
        let all_text = second
            .messages
            .iter()
            .filter_map(|m| match m {
                Message::User { content } => Some(content.as_str()),
                Message::Assistant(msg) => msg.content.as_deref(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all_text.contains("A local-first plan tool"));
    }

    #[tokio::test]
    async fn repository_tool_step_exposes_and_executes_project_root_tools() {
        let root = std::env::temp_dir().join(format!("mmat-discovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temporary project root should be created");
        fs::write(root.join("README.md"), "# Demo\nProject summary\n")
            .expect("temporary project file should be written");

        let client = ScriptedClient::with_messages(vec![
            AssistantMessage::with_tool_calls(
                None,
                vec![ToolCall {
                    call_id: "read-1".to_string(),
                    tool_name: "read_file".to_string(),
                    arguments: serde_json::json!({
                        "path": "README.md",
                        "max_lines": 10,
                    }),
                }],
            ),
            AssistantMessage::from_text(
                serde_json::to_string(&complete_output()).expect("output should serialise"),
            ),
        ]);
        let agent = LlmAgent::new(client.clone());
        let runtime = AnsweringRuntime::new(Vec::<String>::new());

        let output = step_with_repository_tools(&agent, root.clone())
            .run(&runtime, DiscoveryInput::new("Summarise the project code"))
            .await
            .expect("discovery should execute repository tools");

        assert_eq!(output, complete_output());
        let requests = client.requests();
        assert_eq!(requests.len(), 2);
        let tool_names = requests[0]
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(tool_names, vec!["glob_paths", "read_file", "search_files"]);
        assert!(requests[1].messages.iter().any(|message| {
            matches!(
                message,
                Message::Tool(tool)
                    if tool.tool_name == "read_file"
                        && tool.content.to_string().contains("Project summary")
            )
        }));

        fs::remove_dir_all(&root).expect("temporary project root should be removed");
    }
}
