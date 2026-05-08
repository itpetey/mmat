//! The Scholar role researches the codebase and external sources to gather evidence,
//! producing research briefs, evidence packs, and open questions.

use std::sync::Arc;

use async_trait::async_trait;
use coordinator::{
    AuthorityScope, Budget, Role, RoleContext, RoleError, RoleLifecycleState, RoleSpec, RoleType,
};
use event_stream::event::{
    EscalationSeverity, EventId, EventType, EvidenceRef, RoleId as EventRoleId, SemanticEvent,
};
use llm::client::LlmClient;
use llm::executor::{Executor, ExecutorConfig};
use llm::message::{CompletionRequest, Message};
use serde_json;
use tracing::{info, warn};
use uuid::Uuid;

use crate::artefacts::{EvidenceFinding, EvidencePack, OpenQuestion, OpenQuestions, ResearchBrief};
use crate::tooling::{RoleToolRegistry, RoleToolRuntime};

const DEFAULT_MAX_LLM_CALLS: usize = 20;
const DEFAULT_MAX_TOOL_INVOCATIONS: usize = 50;
const DEFAULT_MAX_WEB_SEARCHES: usize = 10;

/// The Scholar role conducts research and gathers evidence about the codebase and problem domain.
pub struct Scholar {
    id: EventRoleId,
    llm_client: Option<Arc<dyn LlmClient>>,
    executor: Executor,
    tool_registry: RoleToolRegistry,
    tool_runtime: RoleToolRuntime,
    max_llm_calls: usize,
    max_web_searches: usize,
    max_tool_invocations: usize,
}

impl Scholar {
    /// Creates a new Scholar with default budget limits and no LLM client.
    pub fn new() -> Self {
        Self {
            id: EventRoleId("scholar-001".to_string()),
            llm_client: None,
            executor: Executor,
            tool_registry: RoleToolRegistry::new(),
            tool_runtime: RoleToolRuntime,
            max_llm_calls: DEFAULT_MAX_LLM_CALLS,
            max_web_searches: DEFAULT_MAX_WEB_SEARCHES,
            max_tool_invocations: DEFAULT_MAX_TOOL_INVOCATIONS,
        }
    }

    /// Configures the Scholar with an LLM client for research.
    pub fn with_llm_client(mut self, llm_client: Arc<dyn LlmClient>) -> Self {
        self.llm_client = Some(llm_client);
        self
    }

    /// Configures the Scholar with a custom tool registry.
    pub fn with_tool_registry(mut self, tool_registry: RoleToolRegistry) -> Self {
        self.tool_registry = tool_registry;
        self
    }

    /// Returns whether an LLM client has been configured.
    pub fn has_llm_client(&self) -> bool {
        self.llm_client.is_some()
    }

    /// Returns the number of configured tools.
    pub fn tool_count(&self) -> usize {
        self.tool_registry.tool_specs().len()
    }

    fn evidence_refs(description: &str) -> Vec<EvidenceRef> {
        vec![EvidenceRef {
            event_id: EventId(Uuid::new_v4()),
            description: description.to_string(),
        }]
    }

    /// Configures the Scholar's research budget limits.
    pub fn with_budget(
        mut self,
        llm_calls: usize,
        web_searches: usize,
        tool_invocations: usize,
    ) -> Self {
        self.max_llm_calls = llm_calls;
        self.max_web_searches = web_searches;
        self.max_tool_invocations = tool_invocations;
        self
    }

    async fn execute_research_loop(
        &self,
        ctx: &RoleContext,
        research_brief: &str,
    ) -> Result<(Vec<EvidenceFinding>, Vec<String>, Vec<String>), RoleError> {
        let mut findings = Vec::new();
        let constraints = Vec::new();
        let mut open_questions = Vec::new();
        let mut llm_calls = 0;
        let web_searches = 0;
        let mut tool_invocations = 0;

        info!("Scholar starting research loop for: {}", research_brief);

        let _executor = &self.executor;

        if let Some(client) = &self.llm_client {
            let request = CompletionRequest::new(
                "scholar-research",
                vec![
                    Message::system(
                        "Gather evidence only. Do not make architectural decisions or recommendations.",
                    ),
                    Message::user(research_brief),
                ],
            );

            let response = Executor::run(
                client.as_ref(),
                &self.tool_registry,
                &ExecutorConfig {
                    max_turns: self.max_llm_calls.max(1),
                    max_tokens: None,
                },
                &self.tool_runtime,
                request,
            )
            .await
            .map_err(|e| RoleError::Internal(format!("Scholar LLM research failed: {e}")))?;

            let extracted_content = match response {
                Message::Assistant { content, .. } => content.unwrap_or_default(),
                other => format!("{other:?}"),
            };

            let filtered = Self::filter_architectural_recommendendations(&extracted_content);
            findings.push(EvidenceFinding {
                claim: filtered.lines().next().unwrap_or(&filtered).to_string(),
                source_reference: "llm://scholar-research".to_string(),
                extracted_content: filtered,
                confidence: 0.7,
                relevance: "LLM and tool-assisted research result".to_string(),
            });

            return Ok((findings, constraints, open_questions));
        }

        while llm_calls < self.max_llm_calls && tool_invocations < self.max_tool_invocations {
            if web_searches >= self.max_web_searches {
                warn!("Scholar web search budget exhausted");
                open_questions.push(
                    "Web search budget exhausted - some questions may require additional research"
                        .to_string(),
                );
                break;
            }

            llm_calls += 1;
            tool_invocations += 1;

            let event_id = Uuid::new_v4();
            let finding = EvidenceFinding {
                claim: format!("Research finding from analysis of: {}", research_brief),
                source_reference: format!("event://{}", event_id),
                extracted_content: format!("Analysis of research brief: {}", research_brief),
                confidence: 0.7,
                relevance: "Directly addresses research question".to_string(),
            };
            findings.push(finding.clone());

            let event = SemanticEvent::new_claim_made(
                EventRoleId(self.id.0.clone()),
                &finding.claim,
                vec![event_stream::event::EvidenceRef {
                    event_id: event_stream::event::EventId(event_id),
                    description: "Research analysis".to_string(),
                }],
                0.7,
            );
            ctx.bus.publish(event).map_err(|e| {
                RoleError::Internal(format!("Failed to publish claim made event: {e:?}"))
            })?;

            if findings.len() >= 5 {
                break;
            }
        }

        if llm_calls >= self.max_llm_calls {
            warn!("Scholar LLM call budget exhausted");
            open_questions
                .push("LLM call budget exhausted - research may be incomplete".to_string());
        }

        if tool_invocations >= self.max_tool_invocations {
            warn!("Scholar tool invocation budget exhausted");
            open_questions
                .push("Tool invocation budget exhausted - research may be incomplete".to_string());
        }

        Ok((findings, constraints, open_questions))
    }

    async fn publish_research_brief(
        &self,
        ctx: &RoleContext,
        brief: &ResearchBrief,
    ) -> Result<(), RoleError> {
        let serialised = serde_json::to_string(brief)
            .map_err(|e| RoleError::Internal(format!("Failed to serialise research brief: {e}")))?;

        let reference = format!("research-brief-{}", Uuid::new_v4());

        let event = SemanticEvent::new_artefact_produced(
            EventRoleId(self.id.0.clone()),
            "research_brief",
            format!("{reference}|{serialised}"),
            EventRoleId(self.id.0.clone()),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish artefact produced event: {e:?}"))
        })?;

        let event = SemanticEvent::new_memory_proposed(
            EventRoleId(self.id.0.clone()),
            "Fact",
            &serialised,
            "Project",
            EventRoleId(self.id.0.clone()),
            Self::evidence_refs("scholar research brief"),
            0.7,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
        })?;

        info!("Published research brief: {}", reference);
        Ok(())
    }

    async fn publish_evidence_pack(
        &self,
        ctx: &RoleContext,
        pack: &EvidencePack,
    ) -> Result<(), RoleError> {
        let serialised = serde_json::to_string(pack)
            .map_err(|e| RoleError::Internal(format!("Failed to serialise evidence pack: {e}")))?;

        let reference = format!("evidence-pack-{}", Uuid::new_v4());

        let event = SemanticEvent::new_artefact_produced(
            EventRoleId(self.id.0.clone()),
            "evidence_pack",
            format!("{reference}|{serialised}"),
            EventRoleId(self.id.0.clone()),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish artefact produced event: {e:?}"))
        })?;

        let event = SemanticEvent::new_memory_proposed(
            EventRoleId(self.id.0.clone()),
            "Fact",
            &serialised,
            "Project",
            EventRoleId(self.id.0.clone()),
            Self::evidence_refs("scholar evidence pack"),
            0.7,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
        })?;

        info!("Published evidence pack: {}", reference);
        Ok(())
    }

    async fn publish_open_questions(
        &self,
        ctx: &RoleContext,
        questions: &OpenQuestions,
    ) -> Result<(), RoleError> {
        let serialised = serde_json::to_string(questions)
            .map_err(|e| RoleError::Internal(format!("Failed to serialise open questions: {e}")))?;

        let reference = format!("open-questions-{}", Uuid::new_v4());

        let event = SemanticEvent::new_artefact_produced(
            EventRoleId(self.id.0.clone()),
            "open_questions",
            format!("{reference}|{serialised}"),
            EventRoleId(self.id.0.clone()),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish artefact produced event: {e:?}"))
        })?;

        let event = SemanticEvent::new_memory_proposed(
            EventRoleId(self.id.0.clone()),
            "OpenQuestion",
            &serialised,
            "Project",
            EventRoleId(self.id.0.clone()),
            Self::evidence_refs("scholar open questions"),
            0.5,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
        })?;

        info!("Published open questions: {}", reference);
        Ok(())
    }

    fn contains_architectural_recommendendations(text: &str) -> bool {
        let patterns = [
            "should use",
            "should be architected",
            "architecture should",
            "recommend using",
            "design pattern",
            "microservices",
            "monolith",
            "layered architecture",
            "event-driven",
        ];

        let lower = text.to_lowercase();
        patterns.iter().any(|pattern| lower.contains(pattern))
    }

    pub(crate) fn filter_architectural_recommendendations(text: &str) -> String {
        if Self::contains_architectural_recommendendations(text) {
            warn!("Detected architectural recommendations in Scholar output, filtering");
            text.lines()
                .filter(|line| {
                    let lower = line.to_lowercase();
                    !lower.contains("should use")
                        && !lower.contains("architecture should")
                        && !lower.contains("recommend using")
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            text.to_string()
        }
    }

    async fn escalate_budget_exhausted(&self, ctx: &RoleContext) -> Result<(), RoleError> {
        let event = SemanticEvent::new_escalation_requested(
            EventRoleId(self.id.0.clone()),
            EventRoleId(self.id.0.clone()),
            EventRoleId("intent-lead-001".to_string()),
            "research budget exhausted",
            EscalationSeverity::Medium,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish escalation event: {e:?}"))
        })?;

        info!("Scholar escalated due to budget exhaustion");
        Ok(())
    }
}

#[async_trait]
impl Role for Scholar {
    fn id(&self) -> EventRoleId {
        EventRoleId(self.id.0.clone())
    }

    fn spec(&self) -> RoleSpec {
        RoleSpec {
            id: EventRoleId(self.id.0.clone()),
            role_type: RoleType::Scholar,
            authority_scope: AuthorityScope::Architecture,
            default_budget: Budget {
                time_limit_seconds: 900,
                token_limit: 200_000,
                max_retries: 2,
            },
            escalation_paths: std::collections::HashMap::new(),
            input_contract: EventType::TaskAssigned,
            output_contract: vec![
                EventType::ArtefactProduced,
                EventType::ClaimMade,
                EventType::MemoryProposed,
            ],
        }
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::TaskAssigned, EventType::HumanFeedbackReceived]
    }

    async fn run(self: Arc<Self>, ctx: RoleContext) -> Result<(), RoleError> {
        info!("Scholar starting");

        ctx.coordinator
            .report_status(EventRoleId(self.id.0.clone()), RoleLifecycleState::Running)
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        let mut receiver = ctx.bus.subscribe(&[EventType::TaskAssigned]);
        let event = receiver.recv().await.map_err(|e| {
            RoleError::Internal(format!("Failed to receive task assigned event: {e:?}"))
        })?;

        let (research_brief, worker_id) = match event.as_ref() {
            SemanticEvent::TaskAssigned {
                contract_ref,
                worker_id,
                ..
            } => (contract_ref.description.clone(), worker_id.clone()),
            _ => {
                return Err(RoleError::Internal(
                    "Expected TaskAssigned event".to_string(),
                ));
            }
        };

        if worker_id.0 != self.id.0 {
            return Err(RoleError::ContractViolation(format!(
                "Task assigned to {} but Scholar is {}",
                worker_id.0, self.id.0
            )));
        }

        let filtered_brief = Self::filter_architectural_recommendendations(&research_brief);

        let (findings, constraints, open_questions) =
            self.execute_research_loop(&ctx, &filtered_brief).await?;

        let research_brief_artefact = ResearchBrief {
            summary: format!("Research completed for: {}", filtered_brief),
            key_patterns: findings.iter().map(|f| f.claim.clone()).collect(),
            discovered_constraints: constraints.clone(),
        };

        let evidence_pack = EvidencePack {
            findings: findings.clone(),
        };

        let open_questions_artefact = OpenQuestions {
            questions: open_questions
                .iter()
                .map(|q| OpenQuestion {
                    question: q.clone(),
                    why_it_matters: "This affects the completeness of our understanding"
                        .to_string(),
                    suggested_approach: "Additional research with extended budget".to_string(),
                    current_confidence: 0.5,
                })
                .collect(),
        };

        self.publish_research_brief(&ctx, &research_brief_artefact)
            .await?;
        self.publish_evidence_pack(&ctx, &evidence_pack).await?;
        self.publish_open_questions(&ctx, &open_questions_artefact)
            .await?;

        for finding in &findings {
            let event = SemanticEvent::new_memory_proposed(
                EventRoleId(self.id.0.clone()),
                "Fact",
                &finding.claim,
                "Project",
                EventRoleId(self.id.0.clone()),
                Self::evidence_refs("scholar finding"),
                finding.confidence,
            );
            ctx.bus.publish(event).map_err(|e| {
                RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
            })?;
        }

        for constraint in &constraints {
            let event = SemanticEvent::new_memory_proposed(
                EventRoleId(self.id.0.clone()),
                "Constraint",
                constraint,
                "Project",
                EventRoleId(self.id.0.clone()),
                Self::evidence_refs("scholar discovered constraint"),
                0.7,
            );
            ctx.bus.publish(event).map_err(|e| {
                RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
            })?;
        }

        if !open_questions.is_empty() {
            self.escalate_budget_exhausted(&ctx).await?;
        }

        ctx.coordinator
            .report_status(
                EventRoleId(self.id.0.clone()),
                RoleLifecycleState::Completed,
            )
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        info!("Scholar completed");
        Ok(())
    }
}

impl Default for Scholar {
    fn default() -> Self {
        Self::new()
    }
}
