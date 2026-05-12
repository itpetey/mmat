//! The Architect role evaluates tradeoffs and produces Architecture Decision Records (ADRs),
//! interface specifications, and dependency rules.

use std::sync::Arc;

use async_trait::async_trait;
use mmat_coordinator::{
    AuthorityScope, Budget, CapabilityStatus, Role, RoleContext, RoleError, RoleLifecycleState,
    RoleReadiness, RoleSpec, RoleType,
};
use mmat_event_stream::event::{
    EscalationSeverity, EventId, EventType, EvidenceRef, RoleId as EventRoleId, SemanticEvent,
};
use mmat_llm::{
    client::LlmClient,
    executor::{Executor, ExecutorConfig},
    message::{CompletionRequest, Message},
};
use serde_json;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    artefacts::{Adr, DependencyRules, InterfaceSpec},
    tooling::{RoleToolRegistry, RoleToolRuntime},
};

/// The Architect role evaluates architectural tradeoffs and produces ADRs, interface specs, and dependency rules.
pub struct Architect {
    id: EventRoleId,
    llm_client: Option<Arc<dyn LlmClient>>,
    #[allow(dead_code)]
    executor: Executor,
    tool_registry: RoleToolRegistry,
    tool_runtime: RoleToolRuntime,
    model: String,
}

impl Architect {
    /// Creates a new Architect with default settings and no LLM client.
    pub fn new() -> Self {
        Self {
            id: EventRoleId("architect-001".to_string()),
            llm_client: None,
            executor: Executor,
            tool_registry: RoleToolRegistry::new(),
            tool_runtime: RoleToolRuntime::new(),
            model: "big-pickle".to_string(),
        }
    }

    /// Configures the Architect with an LLM client for making architecture decisions.
    pub fn with_llm_client(mut self, llm_client: Arc<dyn LlmClient>) -> Self {
        self.llm_client = Some(llm_client);
        self
    }

    /// Configures the Architect with a specific model identifier.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Configures the Architect with a custom tool registry.
    pub fn with_tool_registry(mut self, tool_registry: RoleToolRegistry) -> Self {
        self.tool_registry = tool_registry;
        self
    }

    /// Sets the event bus on the tool runtime so tools can publish events.
    pub fn set_tool_bus(&mut self, bus: mmat_event_stream::event_bus::EventBus) {
        self.tool_runtime.bus = Some(bus);
    }

    /// Registers a tool in this role's tool registry.
    pub fn register_tool(
        &mut self,
        tool: Box<dyn mmat_llm::tool::Tool<RoleToolRuntime, crate::tooling::RoleToolError>>,
    ) -> Result<(), mmat_llm::tool::RegistryError> {
        self.tool_registry.register(tool)
    }

    /// Returns whether an LLM client has been configured.
    pub fn has_llm_client(&self) -> bool {
        self.llm_client.is_some()
    }

    fn evidence_refs(description: &str) -> Vec<EvidenceRef> {
        vec![EvidenceRef {
            event_id: EventId(Uuid::new_v4()),
            description: description.to_string(),
        }]
    }

    async fn generate_adr(
        &self,
        ctx: &RoleContext,
        intent_brief: &str,
        research_brief: &str,
    ) -> Result<Adr, RoleError> {
        let _ctx = ctx;
        if let Some(client) = &self.llm_client {
            let system_prompt = "\
You are an architect evaluating tradeoffs and making architecture decisions. \
Produce an Architecture Decision Record (ADR) with the following structure: \
title, decision, context, at least two alternatives considered, tradeoffs, consequences, and references. \
Base your decision on the provided intent brief and research context.";

            let user_prompt = format!(
                "Intent Brief:\n{}\n\nResearch Context:\n{}",
                intent_brief, research_brief
            );

            let request = CompletionRequest::new(
                &self.model,
                vec![Message::system(system_prompt), Message::user(&user_prompt)],
            );

            let response = Executor::run(
                client.as_ref(),
                &self.tool_registry,
                &ExecutorConfig {
                    max_turns: 5,
                    max_tokens: None,
                },
                &self.tool_runtime,
                request,
            )
            .await
            .map_err(|e| RoleError::Internal(format!("Architect LLM call failed: {e}")))?;

            let content = match response {
                Message::Assistant { content, .. } => content.unwrap_or_default(),
                other => format!("{other:?}"),
            };

            return Ok(Adr {
                id: format!("adr-{}", Uuid::new_v4()),
                title: Self::extract_title(&content),
                status: "proposed".to_string(),
                context: format!("Intent: {}\nResearch: {}", intent_brief, research_brief),
                decision: content,
                alternatives: vec![
                    "Alternative evaluated via LLM analysis".to_string(),
                    "Alternative evaluated via LLM analysis".to_string(),
                ],
                tradeoffs: "Tradeoffs evaluated by LLM".to_string(),
                consequences: "Consequences documented in decision".to_string(),
                references: vec![intent_brief.to_string(), research_brief.to_string()],
            });
        }

        let adr = Adr {
            id: format!("adr-{}", Uuid::new_v4()),
            title: "Architecture Decision".to_string(),
            status: "proposed".to_string(),
            context: format!("Intent: {}\nResearch: {}", intent_brief, research_brief),
            decision: format!("Decision based on: {}", intent_brief),
            alternatives: vec!["Alternative 1".to_string(), "Alternative 2".to_string()],
            tradeoffs: "Tradeoffs to be evaluated".to_string(),
            consequences: "Consequences to be documented".to_string(),
            references: vec![intent_brief.to_string(), research_brief.to_string()],
        };
        Ok(adr)
    }

    fn extract_title(content: &str) -> String {
        content
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_string())
            .unwrap_or_else(|| "Architecture Decision".to_string())
    }

    async fn generate_interface_spec(
        &self,
        _ctx: &RoleContext,
        module_name: &str,
        adr_ref: &str,
    ) -> Result<InterfaceSpec, RoleError> {
        let spec = InterfaceSpec {
            id: format!("iface-{}", Uuid::new_v4()),
            module_name: module_name.to_string(),
            input_types: vec!["InputType".to_string()],
            output_types: vec!["OutputType".to_string()],
            error_modes: vec!["ErrorMode".to_string()],
            backwards_compatibility: "Compatible with v1".to_string(),
            adr_reference: adr_ref.to_string(),
        };
        Ok(spec)
    }

    async fn generate_dependency_rules(
        &self,
        _ctx: &RoleContext,
        module_name: &str,
    ) -> Result<DependencyRules, RoleError> {
        let rules = DependencyRules {
            id: format!("dep-rules-{}", Uuid::new_v4()),
            module: module_name.to_string(),
            allowed_dependencies: vec![],
            forbidden_dependencies: vec![],
        };
        Ok(rules)
    }

    async fn publish_adr(&self, ctx: &RoleContext, adr: &Adr) -> Result<(), RoleError> {
        let serialised = serde_json::to_string(adr)
            .map_err(|e| RoleError::Internal(format!("Failed to serialise ADR: {e}")))?;

        let event = SemanticEvent::new_decision_recorded(
            EventRoleId(self.id.0.clone()),
            &adr.decision,
            Self::evidence_refs(&adr.title),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish decision recorded event: {e:?}"))
        })?;

        let stored = ctx.store_artefact("adr", &serialised).await?;
        let artefact_event = SemanticEvent::new_artefact_produced_ref(
            EventRoleId(self.id.0.clone()),
            stored.artefact_id.clone(),
            "adr",
            stored.content_hash,
            stored.storage_uri,
            EventRoleId(self.id.0.clone()),
            Self::evidence_refs(&adr.title),
        );
        ctx.bus.publish(artefact_event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish artefact produced event: {e:?}"))
        })?;

        info!("Published ADR: {}", stored.artefact_id);
        Ok(())
    }

    async fn publish_interface_spec(
        &self,
        ctx: &RoleContext,
        spec: &InterfaceSpec,
    ) -> Result<(), RoleError> {
        let serialised = serde_json::to_string(spec)
            .map_err(|e| RoleError::Internal(format!("Failed to serialise interface spec: {e}")))?;

        let stored = ctx.store_artefact("interface_spec", &serialised).await?;
        let event = SemanticEvent::new_artefact_produced_ref(
            EventRoleId(self.id.0.clone()),
            stored.artefact_id.clone(),
            "interface_spec",
            stored.content_hash,
            stored.storage_uri,
            EventRoleId(self.id.0.clone()),
            Vec::new(),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish artefact produced event: {e:?}"))
        })?;

        info!("Published interface spec: {}", stored.artefact_id);
        Ok(())
    }

    async fn publish_dependency_rules(
        &self,
        ctx: &RoleContext,
        rules: &DependencyRules,
    ) -> Result<(), RoleError> {
        let serialised = serde_json::to_string(rules).map_err(|e| {
            RoleError::Internal(format!("Failed to serialise dependency rules: {e}"))
        })?;

        let stored = ctx.store_artefact("dependency_rules", &serialised).await?;
        let event = SemanticEvent::new_artefact_produced_ref(
            EventRoleId(self.id.0.clone()),
            stored.artefact_id.clone(),
            "dependency_rules",
            stored.content_hash,
            stored.storage_uri,
            EventRoleId(self.id.0.clone()),
            Vec::new(),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish artefact produced event: {e:?}"))
        })?;

        info!("Published dependency rules: {}", stored.artefact_id);
        Ok(())
    }

    fn extract_constraints(intent_brief: &str) -> Vec<String> {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(intent_brief)
            && let Some(constraints) = json.get("constraints")
            && let Some(arr) = constraints.as_array()
        {
            return arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        vec![]
    }

    fn validate_against_constraints(
        &self,
        adr: &Adr,
        constraints: &[String],
    ) -> Result<(), String> {
        let decision_lower = adr.decision.to_lowercase();
        for constraint in constraints {
            let constraint_lower = constraint.to_lowercase();
            if constraint_lower.contains("must not") || constraint_lower.contains("shall not") {
                let negated_term = constraint_lower
                    .split("must not")
                    .last()
                    .or_else(|| constraint_lower.split("shall not").last())
                    .unwrap_or("")
                    .trim();
                if !negated_term.is_empty() && decision_lower.contains(negated_term) {
                    return Err(format!(
                        "ADR '{}' contradicts constraint: {}",
                        adr.title, constraint
                    ));
                }
            }
            if constraint_lower.contains("must use") || constraint_lower.contains("shall use") {
                let required = constraint_lower
                    .split("must use")
                    .last()
                    .or_else(|| constraint_lower.split("shall use").last())
                    .unwrap_or("")
                    .trim();
                if !required.is_empty() && !decision_lower.contains(required) {
                    return Err(format!(
                        "ADR '{}' does not satisfy required constraint: {}",
                        adr.title, constraint
                    ));
                }
            }
        }
        Ok(())
    }

    async fn escalate_contradiction(
        &self,
        ctx: &RoleContext,
        contradiction: &str,
    ) -> Result<(), RoleError> {
        let event = SemanticEvent::new_escalation_requested(
            EventRoleId(self.id.0.clone()),
            EventRoleId(self.id.0.clone()),
            EventRoleId("intent-lead-001".to_string()),
            contradiction,
            EscalationSeverity::High,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish escalation event: {e:?}"))
        })?;

        warn!("Architect escalated contradiction: {}", contradiction);
        Ok(())
    }
}

#[async_trait]
impl Role for Architect {
    fn id(&self) -> EventRoleId {
        EventRoleId(self.id.0.clone())
    }

    fn spec(&self) -> RoleSpec {
        RoleSpec {
            id: EventRoleId(self.id.0.clone()),
            role_type: RoleType::Architect,
            authority_scope: AuthorityScope::Architecture,
            default_budget: Budget {
                time_limit_seconds: 1800,
                token_limit: 500_000,
                max_retries: 2,
            },
            escalation_paths: std::collections::HashMap::new(),
            input_contract: EventType::TaskAssigned,
            output_contract: vec![
                EventType::DecisionRecorded,
                EventType::ArtefactProduced,
                EventType::MemoryProposed,
            ],
        }
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::TaskAssigned]
    }

    fn role_readiness(&self) -> RoleReadiness {
        let has_llm = self.has_llm_client();
        let capability = if has_llm {
            CapabilityStatus::Configured
        } else {
            CapabilityStatus::Fallback
        };
        RoleReadiness {
            capability,
            has_llm_client: has_llm,
            has_tools: false,
            tool_count: 0,
            fallback_worktree: false,
            requires_llm: true,
            has_artefact_store: false,
            summary: format!(
                "LLM: {} — {}",
                if has_llm { "configured" } else { "missing" },
                capability,
            ),
        }
    }

    async fn run(self: Arc<Self>, ctx: RoleContext) -> Result<(), RoleError> {
        info!("Architect starting");

        ctx.coordinator
            .report_status(EventRoleId(self.id.0.clone()), RoleLifecycleState::Running)
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        let mut receiver = ctx.bus.subscribe(&[EventType::TaskAssigned]);
        let contract_ref = loop {
            let event = receiver.recv().await.map_err(|e| {
                RoleError::Internal(format!("Failed to receive task assigned event: {e:?}"))
            })?;

            if let SemanticEvent::TaskAssigned {
                contract_ref,
                worker_id,
                ..
            } = event.as_ref()
            {
                if worker_id.0 == self.id.0 {
                    break contract_ref.clone();
                }
                warn!("Architect ignoring task assigned to {}", worker_id.0);
            }
        };

        let intent_brief = &contract_ref.description;

        let adr = self
            .generate_adr(&ctx, intent_brief, "research context")
            .await?;

        let constraints = Self::extract_constraints(intent_brief);
        if let Err(contradiction) = self.validate_against_constraints(&adr, &constraints) {
            self.escalate_contradiction(&ctx, &contradiction).await?;
            return Err(RoleError::Internal(format!(
                "ADR contradicts constraints: {contradiction}"
            )));
        }

        self.publish_adr(&ctx, &adr).await?;

        let iface_spec = self
            .generate_interface_spec(&ctx, &adr.title, &adr.id)
            .await?;
        self.publish_interface_spec(&ctx, &iface_spec).await?;

        let dep_rules = self.generate_dependency_rules(&ctx, &adr.title).await?;
        self.publish_dependency_rules(&ctx, &dep_rules).await?;

        ctx.coordinator
            .report_status(
                EventRoleId(self.id.0.clone()),
                RoleLifecycleState::Completed,
            )
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        info!("Architect completed");
        Ok(())
    }
}

impl Default for Architect {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use mmat_coordinator::{AuthorityScope, Role, RoleType};
    use mmat_event_stream::event::EventType;

    use super::*;

    #[test]
    fn creates_with_default_id() {
        let architect = Architect::new();
        assert_eq!(architect.id().0, "architect-001");
    }

    #[test]
    fn spec_matches_architecture_authority_and_contracts() {
        let architect = Architect::new();
        let spec = architect.spec();
        assert_eq!(spec.role_type, RoleType::Architect);
        assert!(matches!(spec.authority_scope, AuthorityScope::Architecture));
        assert!(spec.output_contract.contains(&EventType::DecisionRecorded));
        assert!(spec.output_contract.contains(&EventType::ArtefactProduced));
    }

    #[test]
    fn subscribes_to_assigned_tasks() {
        let architect = Architect::new();
        let subscriptions = architect.subscriptions();
        assert!(subscriptions.contains(&EventType::TaskAssigned));
    }
}
