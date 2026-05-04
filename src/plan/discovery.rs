use std::{
    convert::Infallible,
    fmt::{Debug, Display},
    path::PathBuf,
};

use futures::future;
use naaf_core::{Attempt, RetryPolicy, Step, check_fn, repair_fn, task_fn};
#[cfg(test)]
use naaf_llm::ExecutionOutcome;
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

pub type ConvergentStep<C, R, E> =
    Step<R, DiscoveryInput, DiscoveryOutput, DiscoveryFinding, DiscoveryStepError<C, R, E>>;
pub type DivergentStep<C, R, E> =
    Step<R, DiscoveryInput, BigPicture, DivergentFinding, DiscoveryStepError<C, R, E>>;
type DiscoveryStepError<C, R, E> = WorkflowTaskError<C, R, E>;

pub const MODEL: &str = "gpt-5.5";

/// System prompt for the convergent (guided narrowing) discovery phase.
/// The BigPicture is binding — this phase chooses paths, never shrinks scope.
pub const SYSTEM_PROMPT: &str = r#"You are guided discovery within an established BigPicture.

The BigPicture defines the outer boundaries of the idea. These boundaries are IMMUTABLE.
Your job is to narrow WITHIN them: choose among divergent approaches, merge them, or
decompose into sub-domains.

CRITICAL DISTINCTION:
  • Narrowing = choosing a path through the design space, deciding which sub-domain
to tackle first, or how to decompose. This is good.
  • Scope-cutting = removing something from the BigPicture's outer_boundaries.
This is forbidden.

The BigPicture will be provided in the context. If your output would contradict it,
reconsider. You are choosing a path, not shrinking the idea."#;

/// System prompt for the divergent (broad exploration) discovery phase.
/// Maps the full design space, surfaces alternatives, and establishes outer boundaries.
pub const DIVERGENT_SYSTEM_PROMPT: &str = r#"You are exploring the outer boundaries of a new idea. This is thinking time, not planning time.

Your job is to map the full design space — what's possible, what's adjacent, what's
analogous. Surface multiple divergent approaches. Challenge assumptions. Reframe the
problem from different angles. Research prior art and concrete examples.

Key principles from explore mode:
  • Open threads, not interrogations — surface multiple interesting directions and
    let the user follow what resonates. Do not funnel them through a single path.
  • Patient exploration — don't rush to conclusions. Let the shape of the problem emerge.
  • Visual thinking — use ASCII diagrams when they help map the space.
  • Grounded exploration — reference concrete projects, papers, or patterns that
    illuminate the space.
  • Challenge assumptions — including the user's and your own.

CRITICAL:
  • Do NOT narrow yet. Do NOT suggest a recommended path.
  • Do NOT try to make the problem "concrete and solutionable" during this phase.
  • When the space feels well-mapped, offer to converge — set ready_to_converge to
    true and ask the user if they agree. Let them decide."#;

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

/// Immutable output of the divergent discovery phase. Defines the outer boundaries
/// and full design space of the problem. Passed through all downstream phases as
/// a binding constraint — never modified after initial creation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct BigPicture {
    pub(crate) ready_to_converge: bool,
    pub(crate) full_scope: String,
    pub(crate) outer_boundaries: Vec<String>,
    pub(crate) out_of_scope: Vec<String>,
    pub(crate) design_space: String,
    pub(crate) divergent_approaches: Vec<String>,
    pub(crate) trade_off_dimensions: Vec<String>,
    pub(crate) prior_art_insights: Vec<String>,
    pub(crate) non_obvious_risks: Vec<String>,
    pub(crate) binding_constraints: Vec<String>,
    pub(crate) open_choices: Vec<String>,
    #[serde(default)]
    pub(crate) assistant_message: String,
    #[serde(default)]
    pub(crate) open_questions: Vec<DiscoveryQuestion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
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
    /// The immutable BigPicture produced by the divergent phase. Always present
    /// after initial exploration. Carried through to all downstream phases.
    #[serde(default)]
    pub(crate) big_picture: Option<BigPicture>,
    /// Which divergent approach was selected during convergent discovery.
    /// Empty means no explicit choice was made yet.
    #[serde(default)]
    pub(crate) chosen_approach: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiscoveryAnswer {
    pub(crate) question: String,
    pub(crate) answer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DivergentFinding {
    InsufficientDesignSpace,
    NoAlternativesSurfaced,
    NoPriorArtExamined,
    PrematureConvergence,
    MissingOuterBoundaries,
    NoClarificationQuestions,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DiscoveryFinding {
    MissingProblemStatement,
    MissingGoals,
    MissingConstraints,
    UnresolvedBlockingAmbiguity,
    NoClarificationQuestions,
    /// Convergent output narrows beyond the BigPicture's outer boundaries.
    ContradictsBigPicture,
}

/// Internal task output that carries the conversation turn so it can be accumulated
/// across repair attempts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DiscoveryTaskOutput {
    pub(crate) output: DiscoveryOutput,
    pub(crate) conversation_turn: Vec<Message>,
}

/// Task output wrapper for the divergent discovery phase.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DivergentTaskOutput {
    pub(crate) output: BigPicture,
    pub(crate) conversation_turn: Vec<Message>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DiscoveryInput {
    initial_prompt: String,
    answers: Vec<DiscoveryAnswer>,
    /// Stringified findings from the validation step. Used for both convergent
    /// and divergent phases — the enum variants are converted to strings before
    /// storage so the input type remains uniform across phases.
    findings: Vec<String>,
    last_output: Option<DiscoveryOutput>,
    messages: Vec<Message>,
    turn_count: usize,
    /// Immutable BigPicture from the divergent phase. Passed through convergent
    /// discovery and all downstream phases as a binding constraint.
    #[serde(default)]
    pub(crate) big_picture: Option<BigPicture>,
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
            big_picture: None,
        }
    }
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

impl Display for DivergentFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientDesignSpace => {
                write!(f, "design space has not been sufficiently mapped")
            }
            Self::NoAlternativesSurfaced => write!(f, "no divergent approaches have been surfaced"),
            Self::NoPriorArtExamined => write!(f, "prior art has not been examined"),
            Self::PrematureConvergence => {
                write!(f, "convergence offered before design space is mapped")
            }
            Self::MissingOuterBoundaries => write!(f, "outer boundaries of scope are not defined"),
            Self::NoClarificationQuestions => {
                write!(
                    f,
                    "no breadth-oriented questions despite incomplete exploration"
                )
            }
        }
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
            Self::ContradictsBigPicture => {
                write!(
                    f,
                    "convergent output contradicts the BigPicture's outer boundaries"
                )
            }
        }
    }
}

#[cfg(test)]
pub(super) fn convergent_step<C, R, E>(agent: &LlmAgent<C, R, E>) -> ConvergentStep<C, R, E>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    Step::builder(agent.task(
        |_runtime: &R, input: DiscoveryInput| {
            Ok::<_, WorkflowBuildError<R::Error>>(build_convergent_request(&input, None))
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
            Box::pin(future::ok(validate_convergent(
                &o.output,
                _input.big_picture.as_ref(),
            )))
        },
    ))
    .repair_with(repair_fn(|r, a| {
        Box::pin(async move {
            repair_convergent(r, a)
                .await
                .map_err(|error| TaskError::Build(WorkflowBuildError::Human(error)))
        })
    }))
    .retry_policy(RetryPolicy::unlimited())
    .build_persistent()
    .map(|task_output| task_output.output)
}

#[cfg(test)]
pub(super) fn divergent_step<C, R, E>(agent: &LlmAgent<C, R, E>) -> DivergentStep<C, R, E>
where
    C: LlmClient<Runtime = R> + 'static,
    C::Error: Debug + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    Step::builder(agent.task(
        |_runtime: &R, input: DiscoveryInput| {
            Ok::<_, WorkflowBuildError<R::Error>>(build_divergent_request(&input, None))
        },
        |outcome: ExecutionOutcome| {
            let output = decode_outcome(outcome.clone())?;
            let conversation_turn = outcome.messages().to_vec();
            Ok(DivergentTaskOutput {
                output,
                conversation_turn,
            })
        },
    ))
    .validate(check_fn(
        |_r: &R, _input: DiscoveryInput, o: DivergentTaskOutput| {
            Box::pin(future::ok(validate_divergent(&o.output)))
        },
    ))
    .repair_with(repair_fn(|r, a| {
        Box::pin(async move {
            repair_divergent(r, a)
                .await
                .map_err(|error| TaskError::Build(WorkflowBuildError::Human(error)))
        })
    }))
    .retry_policy(RetryPolicy::unlimited())
    .build_persistent()
    .map(|task_output| task_output.output)
}

pub(super) fn convergent_step_with_repository_tools<C, R, E>(
    agent: &LlmAgent<C, R, E>,
    workspace_root: PathBuf,
) -> ConvergentStep<C, R, E>
where
    C: LlmClient<Runtime = R> + Clone + 'static,
    C::Error: Debug + Display + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    let client = (*agent.executor().client()).clone();
    let message_source = agent.message_source().cloned();
    let base_prompt = format!(
        "{}\n\nYou have access to repository tools rooted at the selected project: `glob_paths`, `search_files`, and `read_file`. Use them before making claims about existing project code, structure, documentation, or dependencies.",
        SYSTEM_PROMPT
    );

    let task = task_fn(move |runtime: &R, input: DiscoveryInput| {
        let client = client.clone();
        let base_prompt = base_prompt.clone();
        let workspace_root = workspace_root.clone();
        let message_source = message_source.clone();
        Box::pin(async move {
            // Compose the system prompt: base + BigPicture context if available.
            let system_prompt = if let Some(bp) = &input.big_picture {
                format!(
                    "{}\n\n## BIG PICTURE (binding constraints, never violated)\n\nFull scope: {}\n\nOuter boundaries (in scope): {}\n\nOut of scope: {}\n\nBinding constraints: {}\n\nDesign space: {}\n\nDivergent approaches: {}\n\nTrade-off dimensions: {}\n\nPrior art insights: {}\n\nNon-obvious risks: {}\n\nOpen choices: {}",
                    base_prompt,
                    bp.full_scope,
                    bp.outer_boundaries.join(" | "),
                    bp.out_of_scope.join(" | "),
                    bp.binding_constraints.join(" | "),
                    bp.design_space,
                    bp.divergent_approaches.join(" | "),
                    bp.trade_off_dimensions.join(" | "),
                    bp.prior_art_insights.join(" | "),
                    bp.non_obvious_risks.join(" | "),
                    bp.open_choices.join(" | ")
                )
            } else {
                base_prompt
            };

            let request = build_convergent_request(&input, Some(&system_prompt));

            let mut tools = ToolRegistry::<R, Infallible>::new();
            register_repository_tools(&mut tools, workspace_root);
            let mut executor = Executor::with_tools(client, tools);
            if let Some(source) = message_source {
                executor = executor.with_message_source(source);
            }
            let outcome = execute_with_turn_limit_retry(&executor, runtime, request)
                .await
                .map_err(|error| {
                    AdaptorError::Build(WorkflowBuildError::Workflow(format!(
                        "executor failed: {error}"
                    )))
                })?;

            let mut output: DiscoveryOutput =
                decode_outcome(outcome.clone()).map_err(AdaptorError::Decode)?;

            // Propagate the immutable BigPicture from the divergent phase into the
            // convergent output so downstream phases receive it via DiscoveryOutput.
            output.big_picture = input.big_picture.clone();

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
                Box::pin(future::ok(validate_convergent(
                    &o.output,
                    _input.big_picture.as_ref(),
                )))
            },
        ))
        .repair_with(repair_fn(|r, a| {
            Box::pin(async move {
                repair_convergent(r, a)
                    .await
                    .map_err(|error| TaskError::Build(WorkflowBuildError::Human(error)))
            })
        }))
        .retry_policy(RetryPolicy::unlimited())
        .build_persistent()
        .map(|task_output| task_output.output)
}

pub(super) fn divergent_step_with_repository_tools<C, R, E>(
    agent: &LlmAgent<C, R, E>,
    workspace_root: PathBuf,
) -> DivergentStep<C, R, E>
where
    C: LlmClient<Runtime = R> + Clone + 'static,
    C::Error: Debug + Display + 'static,
    E: Debug + 'static,
    R: HumanIO + 'static,
    R::Error: Debug + 'static,
{
    let client = (*agent.executor().client()).clone();
    let message_source = agent.message_source().cloned();
    let system_prompt = format!(
        "{}\n\nYou have access to repository tools rooted at the selected project: `glob_paths`, `search_files`, and `read_file`. Use them before making claims about existing project code, structure, documentation, or dependencies.",
        DIVERGENT_SYSTEM_PROMPT
    );

    let task = task_fn(move |runtime: &R, input: DiscoveryInput| {
        let client = client.clone();
        let system_prompt = system_prompt.clone();
        let workspace_root = workspace_root.clone();
        let message_source = message_source.clone();
        Box::pin(async move {
            let request = build_divergent_request(&input, Some(&system_prompt));

            let mut tools = ToolRegistry::<R, Infallible>::new();
            register_repository_tools(&mut tools, workspace_root);
            let mut executor = Executor::with_tools(client, tools);
            if let Some(source) = message_source {
                executor = executor.with_message_source(source);
            }
            let outcome = execute_with_turn_limit_retry(&executor, runtime, request)
                .await
                .map_err(|error| {
                    AdaptorError::Build(WorkflowBuildError::Workflow(format!(
                        "executor failed: {error}"
                    )))
                })?;

            let output: BigPicture =
                decode_outcome(outcome.clone()).map_err(AdaptorError::Decode)?;

            let input_message_count = input.messages.len();
            let conversation_turn = outcome
                .messages()
                .iter()
                .skip(input_message_count)
                .cloned()
                .collect();

            Ok(DivergentTaskOutput {
                output,
                conversation_turn,
            })
        })
    });

    Step::builder(task)
        .validate(check_fn(
            |_r: &R, _input: DiscoveryInput, o: DivergentTaskOutput| {
                Box::pin(future::ok(validate_divergent(&o.output)))
            },
        ))
        .repair_with(repair_fn(|r, a| {
            Box::pin(async move {
                repair_divergent(r, a)
                    .await
                    .map_err(|error| TaskError::Build(WorkflowBuildError::Human(error)))
            })
        }))
        .retry_policy(RetryPolicy::unlimited())
        .build_persistent()
        .map(|task_output| task_output.output)
}

// ─── Convergent discovery message builders ────────────────────────────────

fn build_convergent_initial_user_message(input: &DiscoveryInput) -> String {
    let mut lines = vec![format!("Initial prompt: {}", input.initial_prompt)];

    if let Some(bp) = &input.big_picture {
        lines.push(String::new());
        lines.push("## BIG PICTURE (binding, never violated)".to_string());
        lines.push(format!("Full scope: {}", bp.full_scope));
        lines.push(format!(
            "Outer boundaries: {}",
            bp.outer_boundaries.join(" | ")
        ));
        lines.push(format!("Out of scope: {}", bp.out_of_scope.join(" | ")));
        lines.push(format!(
            "Binding constraints: {}",
            bp.binding_constraints.join(" | ")
        ));
        lines.push(format!(
            "Divergent approaches: {}",
            bp.divergent_approaches.join(" | ")
        ));
        lines.push(format!("Open choices: {}", bp.open_choices.join(" | ")));
    }

    lines.push(String::new());
    lines.push(
        "Return only one JSON object. Do not include markdown, prose, code fences, or hidden reasoning in the assistant content."
            .to_string(),
    );
    lines.push(
        "The JSON object must use this exact shape: {\"assistant_message\":string,\"ready_for_solution\":boolean,\"problem_statement\":string,\"goals\":string[],\"constraints\":string[],\"assumptions\":string[],\"risks\":string[],\"notes\":string[],\"recommended_path\":string,\"open_questions\":[{\"prompt\":string,\"choices\":string[]}],\"sub_domains\":[{\"name\":string,\"description\":string}],\"chosen_approach\":string}"
            .to_string(),
    );
    lines.push(
        "Use assistant_message for concise, conversational exploration that can be shown to the user before the next question."
            .to_string(),
    );
    lines.push(
        "You are narrowing WITHIN the BigPicture's boundaries. Choose among the divergent approaches, or merge them. Decide on decomposition (sub_domains) if useful."
            .to_string(),
    );
    lines.push(
        "CRITICAL: Never reduce the BigPicture's outer_boundaries. If an approach requires dropping something in scope, pick a different approach instead."
            .to_string(),
    );
    lines.push(
        "Set chosen_approach to the name of the divergent approach you are selecting, or a merged description."
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

fn build_convergent_request(
    input: &DiscoveryInput,
    system_prompt_override: Option<&str>,
) -> CompletionRequest {
    let mut messages = input.messages.clone();
    let max_input_tokens = input_token_budget_for_model(MODEL);
    maybe_compact_messages(&mut messages, max_input_tokens);

    if messages.is_empty() {
        let base = system_prompt_override.unwrap_or(SYSTEM_PROMPT);
        messages.push(Message::system(build_convergent_system_prompt_with_base(
            input.turn_count,
            base,
        )));
        messages.push(Message::user(build_convergent_initial_user_message(input)));
    } else {
        messages.push(Message::user(build_convergent_turn_instructions(input)));
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

fn build_convergent_system_prompt_with_base(turn_count: usize, base: &str) -> String {
    let phase_hint = if turn_count == 0 {
        "This is the first turn of convergent discovery. Begin by reviewing the BigPicture and choosing or merging divergent approaches."
    } else {
        "This is a continuation of convergent discovery. Do not re-summarise the problem statement, goals, or constraints unless they have changed. Focus on what is new and respond to the user's latest answers."
    };
    format!(
        "{base}\n\n{phase_hint}\n\nDo not repeat information the user has already seen. Keep responses concise and forward-moving. It is normal to take many discovery turns.\n\n{}",
        crate::plan::ENGLISH_DIRECTIVE
    )
}

fn build_convergent_turn_instructions(input: &DiscoveryInput) -> String {
    let mut lines = vec![
        "Continue the convergent discovery conversation based on what has been discussed so far."
            .to_string(),
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

// ─── Divergent discovery message builders ─────────────────────────────────

fn build_divergent_initial_user_message(input: &DiscoveryInput) -> String {
    let mut lines = vec![format!("Initial prompt: {}", input.initial_prompt)];

    lines.push(String::new());
    lines.push(
        "Return only one JSON object. Do not include markdown, prose, code fences, or hidden reasoning in the assistant content."
            .to_string(),
    );
    lines.push(
        "The JSON object must use this exact shape: {\"ready_to_converge\":boolean,\"full_scope\":string,\"outer_boundaries\":string[],\"out_of_scope\":string[],\"design_space\":string,\"divergent_approaches\":string[],\"trade_off_dimensions\":string[],\"prior_art_insights\":string[],\"non_obvious_risks\":string[],\"binding_constraints\":string[],\"open_choices\":string[],\"assistant_message\":string,\"open_questions\":[{\"prompt\":string,\"choices\":string[]}]}"
            .to_string(),
    );
    lines.push(
        "Use assistant_message for concise, conversational exploration that can be shown to the user before the next question."
            .to_string(),
    );
    lines.push(
        "Map the full design space. Surface multiple divergent approaches. Research prior art. Identify binding constraints vs open choices. Use ASCII diagrams in design_space when they help visualise the space."
            .to_string(),
    );
    lines.push(
        "outer_boundaries are the things that are DEFINITELY in scope — they become immutable once this phase completes. out_of_scope are the things that are DEFINITELY not in scope."
            .to_string(),
    );
    lines.push(
        "trade_off_dimensions are the axes along which approaches differ (e.g. simplicity vs performance, time-to-market vs completeness)."
            .to_string(),
    );
    lines.push(
        "Only set ready_to_converge true when you have mapped enough of the design space to feel confident that the user can make an informed choice about direction. When you set it, ask the user if they agree — let them decide."
            .to_string(),
    );
    lines.push(
        "Include explicit uncertainty in prior_art_insights, non_obvious_risks, or open_questions as appropriate."
            .to_string(),
    );

    lines.join("\n")
}

fn build_divergent_request(
    input: &DiscoveryInput,
    system_prompt_override: Option<&str>,
) -> CompletionRequest {
    let mut messages = input.messages.clone();
    let max_input_tokens = input_token_budget_for_model(MODEL);
    maybe_compact_messages(&mut messages, max_input_tokens);

    if messages.is_empty() {
        let base = system_prompt_override.unwrap_or(DIVERGENT_SYSTEM_PROMPT);
        messages.push(Message::system(build_divergent_system_prompt_with_base(
            input.turn_count,
            base,
        )));
        messages.push(Message::user(build_divergent_initial_user_message(input)));
    } else {
        messages.push(Message::user(build_divergent_turn_instructions(input)));
    }

    CompletionRequest::new(MODEL.to_string(), messages).with_metadata(json!({
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "big_picture",
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

fn build_divergent_system_prompt_with_base(turn_count: usize, base: &str) -> String {
    let phase_hint = if turn_count == 0 {
        "This is the first turn of divergent discovery. Begin by understanding the user's idea and mapping the full design space. Surface alternatives and prior art."
    } else {
        "This is a continuation of divergent discovery. Do not re-summarise the full_scope, outer_boundaries, or design_space unless they have changed. Focus on what is new and respond to the user's latest answers."
    };
    format!(
        "{base}\n\n{phase_hint}\n\nDo not repeat information the user has already seen. Keep responses concise and forward-moving. It is normal to take many discovery turns.\n\n{}",
        crate::plan::ENGLISH_DIRECTIVE
    )
}

fn build_divergent_turn_instructions(input: &DiscoveryInput) -> String {
    let mut lines = vec![
        "Continue the divergent discovery conversation based on what has been discussed so far."
            .to_string(),
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

async fn repair_convergent<R>(
    runtime: &R,
    attempts: Vec<Attempt<DiscoveryInput, DiscoveryTaskOutput, DiscoveryFinding>>,
) -> Result<DiscoveryInput, R::Error>
where
    R: HumanIO + 'static,
{
    let latest_attempt = attempts
        .last()
        .expect("convergent repair requires an attempt");
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
        findings: latest_attempt
            .findings
            .iter()
            .map(|f| f.to_string())
            .collect(),
        last_output: Some(latest_attempt.output.output.clone()),
        messages,
        turn_count: latest_attempt.input.turn_count + 1,
        big_picture: latest_attempt.input.big_picture.clone(),
    })
}

async fn repair_divergent<R>(
    runtime: &R,
    attempts: Vec<Attempt<DiscoveryInput, DivergentTaskOutput, DivergentFinding>>,
) -> Result<DiscoveryInput, R::Error>
where
    R: HumanIO + 'static,
{
    let latest_attempt = attempts
        .last()
        .expect("divergent repair requires an attempt");
    let mut messages = latest_attempt.input.messages.clone();

    // Append the assistant response and any tool results from the latest turn.
    messages.extend(latest_attempt.output.conversation_turn.clone());

    let mut answers = latest_attempt.input.answers.clone();

    // Use structured open_questions where available, falling back to assistant_message
    // only when no questions are present. This mirrors repair_convergent.
    let output = &latest_attempt.output.output;
    let structured_questions: Vec<&DiscoveryQuestion> = output
        .open_questions
        .iter()
        .filter(|q| !q.prompt.trim().is_empty())
        .collect();

    if !structured_questions.is_empty() {
        for (index, question) in structured_questions.iter().enumerate() {
            let display_question = if index == 0 && !output.assistant_message.trim().is_empty() {
                format!("{}\n\n{}", output.assistant_message.trim(), question.prompt)
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
    } else if !output.assistant_message.trim().is_empty() {
        // No structured questions — fall back to assistant_message as a conversational prompt.
        let reply = runtime
            .ask(HumanQuestion {
                question: output.assistant_message.trim().to_string(),
                choices: None,
            })
            .await?;
        answers.push(DiscoveryAnswer {
            question: output.assistant_message.trim().to_string(),
            answer: reply.content.clone(),
        });
        messages.push(Message::user(reply.content));
    }

    Ok(DiscoveryInput {
        initial_prompt: latest_attempt.input.initial_prompt.clone(),
        answers,
        findings: latest_attempt
            .findings
            .iter()
            .map(|f| f.to_string())
            .collect(),
        last_output: None,
        messages,
        turn_count: latest_attempt.input.turn_count + 1,
        big_picture: None,
    })
}

fn validate_convergent(
    output: &DiscoveryOutput,
    authoritative_big_picture: Option<&BigPicture>,
) -> Vec<DiscoveryFinding> {
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

    // BigPicture fidelity: check that convergent output doesn't shrink scope.
    // Use the authoritative big_picture from the input, not the output-carried
    // copy, to avoid trusting a model that may have omitted or altered it.
    if let Some(bp) = authoritative_big_picture
        && convergent_contradicts_big_picture(output, bp)
    {
        findings.push(DiscoveryFinding::ContradictsBigPicture);
    }

    findings
}

fn validate_divergent(output: &BigPicture) -> Vec<DivergentFinding> {
    let mut findings = Vec::new();

    // Minimum breadth requirements before convergence is legitimate.
    let has_design_space = !output.design_space.trim().is_empty();
    let has_alternatives = !output.divergent_approaches.is_empty();
    let has_prior_art = !output.prior_art_insights.is_empty();
    let has_boundaries = !output.outer_boundaries.is_empty();

    if output.ready_to_converge {
        // When the model offers convergence, ensure it has mapped enough space.
        if !has_design_space {
            findings.push(DivergentFinding::InsufficientDesignSpace);
        }

        if !has_alternatives {
            findings.push(DivergentFinding::NoAlternativesSurfaced);
        }

        if !has_prior_art {
            findings.push(DivergentFinding::NoPriorArtExamined);
        }

        if !has_boundaries {
            findings.push(DivergentFinding::MissingOuterBoundaries);
        }

        // Emit PrematureConvergence if the model signalled readiness but
        // prerequisites are not met.
        if !has_design_space || !has_alternatives || !has_prior_art || !has_boundaries {
            findings.push(DivergentFinding::PrematureConvergence);
        }
    }

    let has_meaningful_questions = output
        .open_questions
        .iter()
        .any(|q| !q.prompt.trim().is_empty());

    if !output.ready_to_converge && !has_meaningful_questions {
        // For divergent phase, we rely on the assistant_message to engage the user.
        // If it's empty and we're not ready to converge, that's a problem.
        if output.assistant_message.trim().is_empty() {
            findings.push(DivergentFinding::NoClarificationQuestions);
        }
    }

    findings
}

/// Checks whether the convergent output narrows beyond the BigPicture boundaries.
fn convergent_contradicts_big_picture(output: &DiscoveryOutput, big_picture: &BigPicture) -> bool {
    // Check if any goal or constraint explicitly contradicts outer boundaries.
    let _out_of_scope_text = big_picture.out_of_scope.join(" ").to_lowercase();

    // If any convergent goal or constraint contains an item that is explicitly
    // out_of_scope, that's a contradiction.
    for goal in &output.goals {
        let goal_lower = goal.to_lowercase();
        for item in &big_picture.out_of_scope {
            if !item.is_empty() && goal_lower.contains(&item.to_lowercase()) {
                return true;
            }
        }
    }

    for constraint in &output.constraints {
        let constraint_lower = constraint.to_lowercase();
        for item in &big_picture.out_of_scope {
            if !item.is_empty() && constraint_lower.contains(&item.to_lowercase()) {
                return true;
            }
        }
    }

    // Note: we intentionally do NOT check whether the problem statement
    // contains literal boundary terms. Paraphrasing is legitimate during
    // convergent discovery, and a substring heuristic would produce false
    // positives. Out-of-scope checks above catch explicit scope cuts.

    false
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
            big_picture: None,
            chosen_approach: String::new(),
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
            big_picture: None,
            chosen_approach: String::new(),
        }
    }

    #[test]
    fn prompt_requires_json_object() {
        let prompt = build_convergent_initial_user_message(&DiscoveryInput::new("Hi"));

        assert!(prompt.contains("Return only one JSON object"));
        assert!(prompt.contains("\"assistant_message\":string"));
        assert!(prompt.contains("\"ready_for_solution\":boolean"));
        assert!(prompt.contains("\"open_questions\""));
        assert!(prompt.contains("Do not include markdown"));
    }

    #[test]
    fn validation_accepts_non_empty_goals_and_constraints() {
        let findings = validate_convergent(&complete_output(), None);

        assert!(findings.is_empty());
    }

    #[test]
    fn validation_reports_missing_goals_and_constraints() {
        let mut output = complete_output();
        output.goals = vec![String::new()];
        output.constraints = Vec::new();

        let findings = validate_convergent(&output, None);

        assert!(findings.contains(&DiscoveryFinding::MissingGoals));
        assert!(findings.contains(&DiscoveryFinding::MissingConstraints));
    }

    #[test]
    fn validation_allows_incomplete_handoff_while_discovery_continues() {
        let mut output = incomplete_output();
        output.problem_statement = String::new();
        output.goals = Vec::new();
        output.constraints = Vec::new();

        let findings = validate_convergent(&output, None);

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

        let findings = validate_convergent(&output, None);

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

        let output = convergent_step(&agent)
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

        let output = convergent_step(&agent)
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

        let output = convergent_step_with_repository_tools(&agent, root.clone())
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

    fn sample_big_picture() -> BigPicture {
        BigPicture {
            ready_to_converge: true,
            full_scope: "Build a real-time collaboration system".to_string(),
            outer_boundaries: vec![
                "CRDT sync".to_string(),
                "offline support".to_string(),
                "presence awareness".to_string(),
            ],
            out_of_scope: vec!["video chat".to_string(), "file sharing".to_string()],
            design_space: "Collaboration spectrum".to_string(),
            divergent_approaches: vec!["CRDT-first".to_string(), "op-first".to_string()],
            trade_off_dimensions: vec!["simplicity vs performance".to_string()],
            prior_art_insights: vec!["Yjs uses CRDTs".to_string()],
            non_obvious_risks: vec!["merge conflicts in presence".to_string()],
            binding_constraints: vec!["must work offline".to_string()],
            open_choices: vec!["sync protocol".to_string()],
            assistant_message: String::new(),
            open_questions: Vec::new(),
        }
    }

    #[test]
    fn validate_divergent_detects_premature_convergence() {
        let output = BigPicture {
            ready_to_converge: true,
            ..Default::default()
        };

        let findings = validate_divergent(&output);

        assert!(findings.contains(&DivergentFinding::PrematureConvergence));
        assert!(findings.contains(&DivergentFinding::InsufficientDesignSpace));
        assert!(findings.contains(&DivergentFinding::NoAlternativesSurfaced));
        assert!(findings.contains(&DivergentFinding::NoPriorArtExamined));
        assert!(findings.contains(&DivergentFinding::MissingOuterBoundaries));
    }

    #[test]
    fn validate_divergent_accepts_ready_when_space_mapped() {
        let output = sample_big_picture();

        let findings = validate_divergent(&output);

        assert!(
            !findings.contains(&DivergentFinding::PrematureConvergence),
            "ready_to_converge should be accepted when prerequisites are met"
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn validate_convergent_detects_contradicts_big_picture() {
        let mut output = complete_output();
        // "video chat" is explicitly out_of_scope in sample_big_picture
        output.goals.push("Implement video chat".to_string());
        let bp = sample_big_picture();

        let findings = validate_convergent(&output, Some(&bp));

        assert!(findings.contains(&DiscoveryFinding::ContradictsBigPicture));
    }

    #[test]
    fn validate_convergent_accepts_aligned_output() {
        let output = complete_output();
        let bp = sample_big_picture();

        let findings = validate_convergent(&output, Some(&bp));

        assert!(
            !findings.contains(&DiscoveryFinding::ContradictsBigPicture),
            "aligned output should not contradict BigPicture"
        );
    }

    #[tokio::test]
    async fn divergent_repair_asks_structured_questions() {
        let divergent_output = BigPicture {
            assistant_message: "Here's what I found.".to_string(),
            open_questions: vec![
                DiscoveryQuestion {
                    prompt: String::new(),
                    choices: Vec::new(),
                },
                DiscoveryQuestion {
                    prompt: "Which approach resonates?".to_string(),
                    choices: vec!["CRDT-first".to_string(), "op-first".to_string()],
                },
            ],
            ..sample_big_picture()
        };
        let client = ScriptedClient::new(vec![
            serde_json::to_string(&divergent_output).expect("output should serialise"),
        ]);
        let agent = LlmAgent::new(client);
        let runtime = AnsweringRuntime::new(["CRDT-first".to_string()]);

        let input = DiscoveryInput::new("Build a collab system");
        let _step = divergent_step_with_repository_tools(&agent, PathBuf::from("/tmp"));
        // We can't easily run the divergent step directly because it expects
        // repository tools, but we can test repair_divergent directly.
        let attempt = Attempt {
            input: input.clone(),
            output: DivergentTaskOutput {
                output: divergent_output,
                conversation_turn: Vec::new(),
            },
            findings: Vec::new(),
        };
        let repaired = repair_divergent(&runtime, vec![attempt])
            .await
            .expect("repair should succeed");

        assert_eq!(
            runtime.questions(),
            vec!["Here's what I found.\n\nWhich approach resonates?".to_string()]
        );
        assert_eq!(repaired.answers.len(), 1);
        assert_eq!(repaired.answers[0].answer, "CRDT-first");
    }

    #[tokio::test]
    async fn convergent_step_with_tools_propagates_big_picture() {
        let root = std::env::temp_dir().join(format!("mmat-discovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temporary project root should be created");

        let mut output = complete_output();
        output.big_picture = None;

        let client = ScriptedClient::new(vec![
            serde_json::to_string(&output).expect("output should serialise"),
        ]);
        let agent = LlmAgent::new(client);
        let runtime = AnsweringRuntime::new(Vec::<String>::new());

        let mut input = DiscoveryInput::new("Hi");
        input.big_picture = Some(sample_big_picture());

        let result = convergent_step_with_repository_tools(&agent, root.clone())
            .run(&runtime, input)
            .await
            .expect("convergent step should succeed");

        assert!(
            result.big_picture.is_some(),
            "big_picture should be propagated from input to output"
        );
        assert_eq!(
            result.big_picture.as_ref().unwrap().full_scope,
            "Build a real-time collaboration system"
        );

        fs::remove_dir_all(&root).expect("temporary project root should be removed");
    }

    #[tokio::test]
    async fn divergent_step_produces_big_picture() {
        let bp = sample_big_picture();
        let client = ScriptedClient::new(vec![
            serde_json::to_string(&bp).expect("big picture should serialise"),
        ]);
        let agent = LlmAgent::new(client);
        let runtime = AnsweringRuntime::new(Vec::<String>::new());

        let input = DiscoveryInput::new("Build a real-time collaboration system");

        let result = divergent_step(&agent)
            .run(&runtime, input)
            .await
            .expect("divergent step should succeed");

        assert!(
            result.ready_to_converge,
            "divergent step should produce a ready BigPicture"
        );
        assert_eq!(result.full_scope, "Build a real-time collaboration system");
        assert!(
            !result.outer_boundaries.is_empty(),
            "divergent step should produce outer boundaries"
        );
        assert!(
            !result.divergent_approaches.is_empty(),
            "divergent step should surface approaches"
        );
    }

    #[tokio::test]
    async fn divergent_step_repairs_premature_convergence() {
        let premature = BigPicture {
            ready_to_converge: true,
            design_space: String::new(),
            divergent_approaches: Vec::new(),
            prior_art_insights: Vec::new(),
            outer_boundaries: Vec::new(),
            open_questions: vec![DiscoveryQuestion {
                prompt: "Should we focus on one area?".to_string(),
                choices: Vec::new(),
            }],
            ..sample_big_picture()
        };
        let ready = sample_big_picture();

        let client = ScriptedClient::new(vec![
            serde_json::to_string(&premature).expect("should serialise"),
            serde_json::to_string(&ready).expect("should serialise"),
        ]);
        let agent = LlmAgent::new(client);
        let runtime = AnsweringRuntime::new(["Let's explore more".to_string()]);

        let input = DiscoveryInput::new("Build a system");
        let result = divergent_step(&agent)
            .run(&runtime, input)
            .await
            .expect("divergent step should repair and succeed");

        assert!(
            result.ready_to_converge,
            "repaired BigPicture should be ready to converge"
        );
        assert!(
            !result.design_space.is_empty(),
            "repaired BigPicture should have design space"
        );
    }
}
