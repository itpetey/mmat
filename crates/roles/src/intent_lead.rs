//! The IntentLead role gathers goals, constraints, and preferences from the human stakeholder
//! and produces an IntentBrief artefact to guide the rest of the organisation.

use std::sync::Arc;

use async_trait::async_trait;
use mmat_coordinator::{
    AuthorityScope, Budget, Role, RoleContext, RoleError, RoleLifecycleState, RoleSpec, RoleType,
};
use mmat_event_stream::event::{EventType, RoleId as EventRoleId, SemanticEvent, TaskContract};
use mmat_llm::client::LlmClient;
use mmat_llm::executor::Executor;
use serde_json;
use tracing::{info, warn};
use uuid::Uuid;

use crate::artefacts::IntentBrief;
use crate::tooling::RoleToolRegistry;

/// The IntentLead role elicits goals, constraints, and preferences from the human stakeholder.
pub struct IntentLead {
    id: EventRoleId,
    llm_client: Option<Arc<dyn LlmClient>>,
    executor: Executor,
    read_tools: RoleToolRegistry,
}

impl IntentLead {
    /// Creates a new IntentLead with default settings and no LLM client.
    pub fn new() -> Self {
        Self {
            id: EventRoleId("intent-lead-001".to_string()),
            llm_client: None,
            executor: Executor,
            read_tools: RoleToolRegistry::new(),
        }
    }

    /// Configures the IntentLead with an LLM client.
    pub fn with_llm_client(mut self, llm_client: Arc<dyn LlmClient>) -> Self {
        self.llm_client = Some(llm_client);
        self
    }

    /// Configures the IntentLead with a set of read-only tools.
    pub fn with_read_tools(mut self, read_tools: RoleToolRegistry) -> Self {
        self.read_tools = read_tools;
        self
    }

    /// Returns whether an LLM client has been configured.
    pub fn has_llm_client(&self) -> bool {
        self.llm_client.is_some()
    }

    /// Returns the number of configured read tools.
    pub fn read_tool_count(&self) -> usize {
        self.read_tools.tool_specs().len()
    }

    async fn interrogate_human(
        &self,
        ctx: &RoleContext,
        question: &str,
        context: &str,
    ) -> Result<String, RoleError> {
        let request_id = EventRoleId(format!("feedback-req-{}", Uuid::new_v4()));

        let event = SemanticEvent::new_human_feedback_requested(
            EventRoleId(self.id.0.clone()),
            question,
            format!("{}|{}", request_id.0, context),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish human feedback request: {e:?}"))
        })?;

        info!("Published human feedback request: {}", question);

        let mut receiver = ctx.bus.subscribe(&[EventType::HumanFeedbackReceived]);
        loop {
            if let Ok(event) = receiver.recv().await
                && let SemanticEvent::HumanFeedbackReceived { answer, .. } = event.as_ref()
            {
                info!("Received human feedback: {}", answer);
                return Ok(answer.clone());
            }
        }
    }

    fn contains_implementation_suggestions(text: &str) -> bool {
        let implementation_patterns = [
            "use ",
            "implement",
            "create a ",
            "build a ",
            "architecture",
            "microservices",
            "database schema",
            "API endpoint",
            "function ",
            "class ",
            "module ",
            "framework",
            "library",
        ];

        let lower = text.to_lowercase();
        implementation_patterns.iter().any(|pattern| {
            lower.contains(&pattern.to_lowercase())
                && !lower.contains(&format!("the human should {}", pattern.to_lowercase()))
        })
    }

    pub(crate) fn filter_implementation_suggestions(text: &str) -> String {
        if Self::contains_implementation_suggestions(text) {
            warn!("Detected implementation suggestions in output, filtering");
            text.lines()
                .filter(|line| {
                    let lower = line.to_lowercase();
                    !lower.starts_with("use ")
                        && !lower.starts_with("implement")
                        && !lower.starts_with("create a")
                        && !lower.starts_with("build a")
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            text.to_string()
        }
    }

    async fn assemble_intent_brief(
        &self,
        goals: Vec<String>,
        non_goals: Vec<String>,
        constraints: Vec<String>,
        success_metrics: Vec<String>,
        stakeholder_preferences: Vec<String>,
        open_questions: Vec<String>,
    ) -> IntentBrief {
        let total_items = goals.len()
            + non_goals.len()
            + constraints.len()
            + success_metrics.len()
            + stakeholder_preferences.len();
        let confidence = if total_items >= 5 && open_questions.is_empty() {
            0.9
        } else if total_items >= 3 {
            0.7
        } else {
            0.5
        };

        IntentBrief {
            goals,
            non_goals,
            constraints,
            success_metrics,
            stakeholder_preferences,
            open_questions,
            confidence,
        }
    }

    async fn publish_intent_brief(
        &self,
        ctx: &RoleContext,
        brief: &IntentBrief,
    ) -> Result<(), RoleError> {
        let serialised = serde_json::to_string(brief)
            .map_err(|e| RoleError::Internal(format!("Failed to serialise intent brief: {e}")))?;

        let reference = format!("intent-brief-{}", Uuid::new_v4());

        let event = SemanticEvent::new_artefact_produced(
            EventRoleId(self.id.0.clone()),
            "intent_brief",
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
            vec![],
            brief.confidence,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
        })?;

        info!("Published intent brief with reference: {}", reference);
        Ok(())
    }

    async fn dispatch_scholar(
        &self,
        ctx: &RoleContext,
        research_question: &str,
        _scope_boundaries: &[String],
    ) -> Result<(), RoleError> {
        let task_id = format!("scholar-task-{}", Uuid::new_v4());
        let contract = TaskContract {
            contract_id: task_id.clone(),
            description: research_question.to_string(),
        };

        let event = SemanticEvent::new_task_assigned(
            EventRoleId(self.id.0.clone()),
            &task_id,
            EventRoleId("scholar-001".to_string()),
            contract,
            vec![],
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish task assigned event: {e:?}"))
        })?;

        info!(
            "Dispatched Scholar with task {} for research: {}",
            task_id, research_question
        );
        Ok(())
    }

    async fn dispatch_ops_manager(
        &self,
        ctx: &RoleContext,
        project_type: &str,
        required_standards: &[String],
    ) -> Result<(), RoleError> {
        let task_id = format!("ops-task-{}", Uuid::new_v4());
        let description = format!(
            "Define processes for {} project type. Required standards: {}",
            project_type,
            required_standards.join(", "),
        );
        let contract = TaskContract {
            contract_id: task_id.clone(),
            description,
        };

        let event = SemanticEvent::new_task_assigned(
            EventRoleId(self.id.0.clone()),
            &task_id,
            EventRoleId("ops-manager-001".to_string()),
            contract,
            vec![],
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish task assigned event: {e:?}"))
        })?;

        info!(
            "Dispatched Ops Manager with task {} for project type: {}",
            task_id, project_type
        );
        Ok(())
    }

    async fn persist_stakeholder_preference(
        &self,
        ctx: &RoleContext,
        preference: &str,
    ) -> Result<(), RoleError> {
        let event = SemanticEvent::new_memory_proposed(
            EventRoleId(self.id.0.clone()),
            "Preference",
            preference,
            "Project",
            EventRoleId(self.id.0.clone()),
            vec![],
            0.8,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
        })?;

        info!("Published stakeholder preference: {}", preference);
        Ok(())
    }
}

#[async_trait]
impl Role for IntentLead {
    fn id(&self) -> EventRoleId {
        EventRoleId(self.id.0.clone())
    }

    fn spec(&self) -> RoleSpec {
        RoleSpec {
            id: EventRoleId(self.id.0.clone()),
            role_type: RoleType::IntentLead,
            authority_scope: AuthorityScope::IntentOnly,
            default_budget: Budget {
                time_limit_seconds: 600,
                token_limit: 50_000,
                max_retries: 3,
            },
            escalation_paths: std::collections::HashMap::new(),
            input_contract: EventType::HumanFeedbackReceived,
            output_contract: vec![
                EventType::ArtefactProduced,
                EventType::TaskAssigned,
                EventType::HumanFeedbackRequested,
                EventType::MemoryProposed,
            ],
        }
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::HumanFeedbackReceived, EventType::TaskCompleted]
    }

    async fn run(self: Arc<Self>, mut ctx: RoleContext) -> Result<(), RoleError> {
        info!("IntentLead starting");

        let _executor = &self.executor;
        let _has_llm = self.has_llm_client();
        let _tool_count = self.read_tool_count();

        ctx.coordinator
            .report_status(EventRoleId(self.id.0.clone()), RoleLifecycleState::Running)
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        let mut goals = Vec::new();
        let mut non_goals = Vec::new();
        let mut constraints = Vec::new();
        let mut success_metrics = Vec::new();
        let mut open_questions = Vec::new();
        let mut stakeholder_preferences = Vec::new();

        let initial_prompt = loop {
            let event = ctx.receiver.recv().await.map_err(|e| {
                RoleError::Internal(format!("Failed to receive initial prompt: {e:?}"))
            })?;

            if let SemanticEvent::HumanFeedbackReceived { answer, .. } = event.as_ref() {
                break answer.clone();
            }
        };

        let filtered_prompt = Self::filter_implementation_suggestions(&initial_prompt);
        goals.push(filtered_prompt.clone());

        loop {
            let clarifying_question = if goals.len() < 2 {
                "Can you elaborate on the specific outcomes you want to achieve? What would success look like?"
            } else if non_goals.is_empty() {
                "Are there any explicit exclusions or things you do NOT want? (non-goals)"
            } else if constraints.is_empty() {
                "Are there any constraints I should be aware of? (budget, timeline, technology, compliance)"
            } else if success_metrics.is_empty() {
                "How will you measure success? What are the key metrics?"
            } else if stakeholder_preferences.len() < 2 {
                "Do you have any preferences or priorities I should keep in mind? (e.g., simplicity over performance)"
            } else {
                break;
            };

            let answer = self
                .interrogate_human(
                    &ctx,
                    clarifying_question,
                    &format!(
                        "Current understanding: goals={:?}, non_goals={:?}, constraints={:?}",
                        goals, non_goals, constraints
                    ),
                )
                .await?;

            let filtered_answer = Self::filter_implementation_suggestions(&answer);

            if goals.len() < 2 {
                if let Some(metric) = filtered_answer
                    .split(',')
                    .map(|s| s.trim())
                    .find(|s| !s.is_empty())
                {
                    success_metrics.push(metric.to_string());
                }
                goals.push(
                    filtered_answer
                        .lines()
                        .next()
                        .unwrap_or(&filtered_answer)
                        .to_string(),
                );
            } else if non_goals.is_empty() {
                if filtered_answer.to_lowercase().contains("no")
                    || filtered_answer.to_lowercase().contains("none")
                {
                    non_goals.push("None specified".to_string());
                } else {
                    filtered_answer
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .for_each(|l| non_goals.push(l.trim().to_string()));
                }
            } else if constraints.is_empty() {
                if filtered_answer.to_lowercase().contains("no")
                    || filtered_answer.to_lowercase().contains("none")
                {
                    constraints.push("None specified".to_string());
                } else {
                    filtered_answer
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .for_each(|l| constraints.push(l.trim().to_string()));
                }
            } else if success_metrics.is_empty() {
                filtered_answer
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .for_each(|s| success_metrics.push(s.to_string()));
            } else if stakeholder_preferences.len() < 2 {
                if filtered_answer.to_lowercase().contains("no")
                    || filtered_answer.to_lowercase().contains("none")
                {
                    stakeholder_preferences.push("None specified".to_string());
                } else {
                    filtered_answer
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .for_each(|l| {
                            let pref = l.trim().to_string();
                            stakeholder_preferences.push(pref.clone());
                        });
                }
            }

            for pref in &stakeholder_preferences {
                if pref != "None specified" {
                    let _ = self.persist_stakeholder_preference(&ctx, pref).await;
                }
            }
        }

        if goals.len() > 3 {
            open_questions.push("Should we prioritise certain goals over others?".to_string());
        }

        let brief = self
            .assemble_intent_brief(
                goals,
                non_goals,
                constraints,
                success_metrics,
                stakeholder_preferences,
                open_questions,
            )
            .await;

        info!(
            "Intent brief assembled with confidence: {}",
            brief.confidence
        );

        self.publish_intent_brief(&ctx, &brief).await?;

        if !brief.goals.is_empty() {
            let _ = self
                .dispatch_scholar(
                    &ctx,
                    &format!(
                        "Study the codebase to understand how to achieve: {}",
                        brief.goals.join(", ")
                    ),
                    &[],
                )
                .await;
        }

        if !brief.constraints.is_empty() {
            let _ = self
                .dispatch_ops_manager(&ctx, "general", &brief.constraints)
                .await;
        }

        ctx.coordinator
            .report_status(
                EventRoleId(self.id.0.clone()),
                RoleLifecycleState::Completed,
            )
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        info!("IntentLead completed");
        Ok(())
    }
}

impl Default for IntentLead {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use mmat_coordinator::{AuthorityScope, Role, RoleRegistry, RoleType};
    use mmat_event_stream::event::EventType;

    use super::*;

    #[test]
    fn creates_with_default_id() {
        let intent_lead = IntentLead::new();
        assert_eq!(intent_lead.id().0, "intent-lead-001");
    }

    #[test]
    fn subscribes_to_human_feedback_and_task_completion() {
        let intent_lead = IntentLead::new();
        let subscriptions = intent_lead.subscriptions();
        assert!(subscriptions.contains(&EventType::HumanFeedbackReceived));
        assert!(subscriptions.contains(&EventType::TaskCompleted));
    }

    #[test]
    fn spec_matches_intent_authority_and_contracts() {
        let intent_lead = IntentLead::new();
        let spec = intent_lead.spec();
        assert_eq!(spec.role_type, RoleType::IntentLead);
        assert!(matches!(spec.authority_scope, AuthorityScope::IntentOnly));
        assert_eq!(spec.input_contract, EventType::HumanFeedbackReceived);
        assert!(spec.output_contract.contains(&EventType::ArtefactProduced));
        assert!(spec.output_contract.contains(&EventType::TaskAssigned));
        assert!(
            spec.output_contract
                .contains(&EventType::HumanFeedbackRequested)
        );
        assert!(spec.output_contract.contains(&EventType::MemoryProposed));

        assert!(
            spec.authority_scope
                .can_publish(&EventType::HumanFeedbackRequested)
        );
        assert!(spec.authority_scope.can_publish(&EventType::TaskAssigned));
        assert!(
            spec.authority_scope
                .can_publish(&EventType::ArtefactProduced)
        );
        assert!(spec.authority_scope.can_publish(&EventType::MemoryProposed));

        let mut registry = RoleRegistry::new();
        registry.register(spec).unwrap();
    }

    #[test]
    fn filters_implementation_suggestions() {
        let input = "Use React for the frontend and Node.js for the backend";
        let filtered = IntentLead::filter_implementation_suggestions(input);
        assert!(
            !filtered.contains("Use React"),
            "Implementation suggestions should be filtered"
        );

        let safe_input = "I want a fast and responsive user interface";
        let filtered_safe = IntentLead::filter_implementation_suggestions(safe_input);
        assert_eq!(
            filtered_safe, safe_input,
            "Safe content should pass through unchanged"
        );
    }
}
