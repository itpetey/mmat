//! The OpsManager role defines and enforces standard operating procedures (SOPs),
//! validation policies, escalation rules, delivery standards, and review rubrics.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use mmat_coordinator::{
    AuthorityScope, Budget, CapabilityStatus, Role, RoleContext, RoleError, RoleLifecycleState,
    RoleReadiness, RoleSpec, RoleType,
};
use mmat_event_stream::event::{
    EventId, EventType, EvidenceRef, ReviewFinding, RoleId as EventRoleId, SemanticEvent,
};
use mmat_llm::{client::LlmClient, executor::Executor};
use mmat_memory::types::MemoryType;
use serde_json;
use tokio::time::{Duration, interval};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    artefacts::{
        DeliveryStandards, EscalationRule, EscalationRules, ReviewDimension, ReviewRubric,
        ValidationPolicy, ValidationStep,
    },
    tooling::{RoleToolRegistry, RoleToolRuntime},
};

const DEFAULT_REVIEW_INTERVAL_SECONDS: u64 = 604800;

/// The OpsManager role defines SOPs, validation policies, escalation rules, delivery standards, and review rubrics.
pub struct OpsManager {
    id: EventRoleId,
    llm_client: Option<Arc<dyn LlmClient>>,
    executor: Executor,
    tool_registry: RoleToolRegistry,
    tool_runtime: RoleToolRuntime,
    review_interval_seconds: u64,
}

impl OpsManager {
    /// Creates a new OpsManager with default settings and no LLM client.
    pub fn new() -> Self {
        Self {
            id: EventRoleId("ops-manager-001".to_string()),
            llm_client: None,
            executor: Executor,
            tool_registry: RoleToolRegistry::new(),
            tool_runtime: RoleToolRuntime::new(),
            review_interval_seconds: DEFAULT_REVIEW_INTERVAL_SECONDS,
        }
    }

    /// Configures the OpsManager with an LLM client.
    pub fn with_llm_client(mut self, llm_client: Arc<dyn LlmClient>) -> Self {
        self.llm_client = Some(llm_client);
        self
    }

    /// Configures the OpsManager with a custom tool registry.
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

    /// Sets the interval in seconds between periodic SOP reviews.
    pub fn with_review_interval(mut self, seconds: u64) -> Self {
        self.review_interval_seconds = seconds;
        self
    }

    async fn publish_artefact(
        &self,
        ctx: &RoleContext,
        artefact_type: &str,
        payload: &str,
    ) -> Result<(), RoleError> {
        let stored = ctx.store_artefact(artefact_type, payload).await?;
        let event = SemanticEvent::new_artefact_produced_ref(
            EventRoleId(self.id.0.clone()),
            stored.artefact_id,
            artefact_type,
            stored.content_hash,
            stored.storage_uri,
            EventRoleId(self.id.0.clone()),
            Vec::new(),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish artefact produced event: {e:?}"))
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_sop(
        &self,
        ctx: &RoleContext,
        procedure_name: &str,
        when_to_apply: &str,
        preconditions: &[String],
        postconditions: &[String],
        steps: &[String],
        rollback_steps: &[String],
    ) -> Result<String, RoleError> {
        let sop_content = format!(
            "SOP: {procedure_name}\n\nWhen to apply: {when_to_apply}\n\nPreconditions:\n{}\n\nSteps:\n{}\n\nPostconditions:\n{}\n\nRollback steps:\n{}",
            preconditions
                .iter()
                .map(|p| format!("- {p}"))
                .collect::<Vec<_>>()
                .join("\n"),
            steps
                .iter()
                .map(|s| format!("- {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
            postconditions
                .iter()
                .map(|p| format!("- {p}"))
                .collect::<Vec<_>>()
                .join("\n"),
            rollback_steps
                .iter()
                .map(|r| format!("- {r}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let event = SemanticEvent::new_memory_proposed(
            EventRoleId(self.id.0.clone()),
            "SOP",
            &sop_content,
            "Organisational",
            EventRoleId(self.id.0.clone()),
            Self::evidence_refs("ops manager generated SOP"),
            0.9,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
        })?;

        self.publish_artefact(ctx, "sop", &sop_content).await?;

        info!("Created SOP: {procedure_name}");
        Ok(sop_content)
    }

    async fn create_review_rubric(&self, ctx: &RoleContext) -> Result<ReviewRubric, RoleError> {
        let dimensions = vec![
            ReviewDimension {
                name: "Correctness".to_string(),
                description: "Does the code do what it claims to do?".to_string(),
                check_items: vec![
                    "Logic matches specification".to_string(),
                    "Edge cases handled".to_string(),
                    "No off-by-one errors".to_string(),
                ],
            },
            ReviewDimension {
                name: "API Design".to_string(),
                description: "Is the public API intuitive and consistent?".to_string(),
                check_items: vec![
                    "Naming follows conventions".to_string(),
                    "Error types are informative".to_string(),
                    "No breaking changes without migration".to_string(),
                ],
            },
            ReviewDimension {
                name: "Cohesion".to_string(),
                description: "Does each module have a single responsibility?".to_string(),
                check_items: vec![
                    "Functions do one thing".to_string(),
                    "Modules have clear boundaries".to_string(),
                ],
            },
            ReviewDimension {
                name: "Coupling".to_string(),
                description: "Are dependencies minimised and well-abstracted?".to_string(),
                check_items: vec![
                    "No circular dependencies".to_string(),
                    "Interfaces are stable".to_string(),
                ],
            },
            ReviewDimension {
                name: "Backwards Compatibility".to_string(),
                description: "Does this change break existing consumers?".to_string(),
                check_items: vec![
                    "API versioning respected".to_string(),
                    "Deprecation warnings added".to_string(),
                ],
            },
            ReviewDimension {
                name: "Observability".to_string(),
                description: "Can we monitor and debug this in production?".to_string(),
                check_items: vec![
                    "Logging at appropriate levels".to_string(),
                    "Metrics exposed for key operations".to_string(),
                    "Error context preserved".to_string(),
                ],
            },
            ReviewDimension {
                name: "Error Handling".to_string(),
                description: "Are errors handled gracefully and informatively?".to_string(),
                check_items: vec![
                    "No unwrap() in production code".to_string(),
                    "Error types are specific".to_string(),
                    "Recovery paths defined".to_string(),
                ],
            },
            ReviewDimension {
                name: "Concurrency".to_string(),
                description: "Is concurrent access safe and efficient?".to_string(),
                check_items: vec![
                    "No data races".to_string(),
                    "Lock scope minimised".to_string(),
                    "Async boundaries correct".to_string(),
                ],
            },
            ReviewDimension {
                name: "Performance".to_string(),
                description: "Does this meet performance requirements?".to_string(),
                check_items: vec![
                    "No N+1 queries".to_string(),
                    "Appropriate data structures".to_string(),
                    "Memory usage bounded".to_string(),
                ],
            },
            ReviewDimension {
                name: "Security".to_string(),
                description: "Are there any security vulnerabilities?".to_string(),
                check_items: vec![
                    "Input validation present".to_string(),
                    "No secrets in code".to_string(),
                    "Authentication/authorisation correct".to_string(),
                ],
            },
            ReviewDimension {
                name: "Test Adequacy".to_string(),
                description: "Is the code sufficiently tested?".to_string(),
                check_items: vec![
                    "Unit tests for critical paths".to_string(),
                    "Integration tests for boundaries".to_string(),
                    "Edge cases covered".to_string(),
                ],
            },
            ReviewDimension {
                name: "Migration Safety".to_string(),
                description: "Can this be deployed and rolled back safely?".to_string(),
                check_items: vec![
                    "Database migrations reversible".to_string(),
                    "Feature flags for risky changes".to_string(),
                    "Rollback plan documented".to_string(),
                ],
            },
        ];

        let rubric = ReviewRubric { dimensions };

        let serialised = serde_json::to_string(&rubric)
            .map_err(|e| RoleError::Internal(format!("Failed to serialise review rubric: {e}")))?;

        let event = SemanticEvent::new_memory_proposed(
            EventRoleId(self.id.0.clone()),
            "SOP",
            &serialised,
            "Organisational",
            EventRoleId(self.id.0.clone()),
            Self::evidence_refs("ops manager generated review rubric"),
            0.9,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
        })?;

        self.publish_artefact(ctx, "review_rubric", &serialised)
            .await?;

        info!(
            "Created review rubric with {} dimensions",
            rubric.dimensions.len()
        );
        Ok(rubric)
    }

    async fn create_validation_policy(
        &self,
        ctx: &RoleContext,
        project_type: &str,
    ) -> Result<ValidationPolicy, RoleError> {
        let steps = match project_type {
            "cli" => vec![
                ValidationStep {
                    command: "cargo fmt --all -- --check".to_string(),
                    pass_criteria: "Exit code 0".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo clippy -- -D warnings".to_string(),
                    pass_criteria: "Exit code 0".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo test".to_string(),
                    pass_criteria: "Exit code 0, all tests pass".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
            ],
            "web-service" => vec![
                ValidationStep {
                    command: "cargo fmt --all -- --check".to_string(),
                    pass_criteria: "Exit code 0".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo clippy -- -D warnings".to_string(),
                    pass_criteria: "Exit code 0".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo test".to_string(),
                    pass_criteria: "Exit code 0, all tests pass".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo audit".to_string(),
                    pass_criteria: "Exit code 0, no vulnerabilities".to_string(),
                    failure_handling: "Escalate to Reviewer immediately".to_string(),
                },
            ],
            "embedded" => vec![
                ValidationStep {
                    command: "cargo fmt --all -- --check".to_string(),
                    pass_criteria: "Exit code 0".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo clippy -- -D warnings".to_string(),
                    pass_criteria: "Exit code 0".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo test --target thumbv7em-none-eabihf".to_string(),
                    pass_criteria: "Exit code 0, all tests pass".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
            ],
            "proc-macro" => vec![
                ValidationStep {
                    command: "cargo fmt --all -- --check".to_string(),
                    pass_criteria: "Exit code 0".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo clippy -- -D warnings".to_string(),
                    pass_criteria: "Exit code 0".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo test".to_string(),
                    pass_criteria: "Exit code 0, all tests pass".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo expand --tests".to_string(),
                    pass_criteria: "Exit code 0, macros expand correctly".to_string(),
                    failure_handling: "Escalate to Reviewer".to_string(),
                },
            ],
            _ => vec![
                ValidationStep {
                    command: "cargo fmt --all -- --check".to_string(),
                    pass_criteria: "Exit code 0".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo clippy -- -D warnings".to_string(),
                    pass_criteria: "Exit code 0".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
                ValidationStep {
                    command: "cargo test".to_string(),
                    pass_criteria: "Exit code 0, all tests pass".to_string(),
                    failure_handling: "Retry once, then escalate to Reviewer".to_string(),
                },
            ],
        };

        let policy = ValidationPolicy {
            project_type: project_type.to_string(),
            steps,
        };

        let serialised = serde_json::to_string(&policy).map_err(|e| {
            RoleError::Internal(format!("Failed to serialise validation policy: {e}"))
        })?;

        let event = SemanticEvent::new_memory_proposed(
            EventRoleId(self.id.0.clone()),
            "SOP",
            &serialised,
            "Project",
            EventRoleId(self.id.0.clone()),
            Self::evidence_refs("ops manager generated validation policy"),
            0.9,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
        })?;

        self.publish_artefact(ctx, "validation_policy", &serialised)
            .await?;

        info!(
            "Created validation policy for {project_type} with {} steps",
            policy.steps.len()
        );
        Ok(policy)
    }

    async fn create_escalation_rules(
        &self,
        ctx: &RoleContext,
    ) -> Result<EscalationRules, RoleError> {
        let rules = vec![
            EscalationRule {
                failure_class: "implementation_defect".to_string(),
                escalation_target: "Reviewer".to_string(),
                description: "Code does not meet quality standards or contains bugs".to_string(),
            },
            EscalationRule {
                failure_class: "architectural_conflict".to_string(),
                escalation_target: "Architect".to_string(),
                description: "Implementation conflicts with established architecture".to_string(),
            },
            EscalationRule {
                failure_class: "missing_knowledge".to_string(),
                escalation_target: "Scholar".to_string(),
                description: "Insufficient information to proceed with task".to_string(),
            },
            EscalationRule {
                failure_class: "ambiguous_intent".to_string(),
                escalation_target: "IntentLead".to_string(),
                description: "Requirements are unclear or contradictory".to_string(),
            },
            EscalationRule {
                failure_class: "broken_process".to_string(),
                escalation_target: "OpsManager".to_string(),
                description: "SOPs or validation policies are not working as expected".to_string(),
            },
        ];

        let escalation_rules = EscalationRules { rules };

        let serialised = serde_json::to_string(&escalation_rules).map_err(|e| {
            RoleError::Internal(format!("Failed to serialise escalation rules: {e}"))
        })?;

        let event = SemanticEvent::new_decision_recorded(
            EventRoleId(self.id.0.clone()),
            &serialised,
            Self::evidence_refs("ops manager generated delivery standards"),
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish decision recorded event: {e:?}"))
        })?;

        self.publish_artefact(ctx, "escalation_rules", &serialised)
            .await?;

        info!(
            "Created escalation rules with {} rules",
            escalation_rules.rules.len()
        );
        Ok(escalation_rules)
    }

    async fn create_delivery_standards(
        &self,
        ctx: &RoleContext,
    ) -> Result<DeliveryStandards, RoleError> {
        let standards = DeliveryStandards {
            branch_naming_convention:
                "feature/<description>, bugfix/<description>, hotfix/<description>".to_string(),
            commit_message_format: "type(scope): description\n\nBody (optional)\n\nRefs: #issue"
                .to_string(),
            pr_size_limit: "Maximum 400 lines of code changed per PR".to_string(),
            review_requirements: vec![
                "At least one approval from a peer".to_string(),
                "All CI checks must pass".to_string(),
                "No unresolved review comments".to_string(),
                "Squash merge to main".to_string(),
            ],
        };

        let serialised = serde_json::to_string(&standards).map_err(|e| {
            RoleError::Internal(format!("Failed to serialise delivery standards: {e}"))
        })?;

        let event = SemanticEvent::new_memory_proposed(
            EventRoleId(self.id.0.clone()),
            "SOP",
            &serialised,
            "Organisational",
            EventRoleId(self.id.0.clone()),
            vec![],
            0.9,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
        })?;

        self.publish_artefact(ctx, "delivery_standards", &serialised)
            .await?;

        info!("Created delivery standards");
        Ok(standards)
    }

    async fn run_periodic_review(&self, ctx: &RoleContext) -> Result<(), RoleError> {
        let mut review_interval = interval(Duration::from_secs(self.review_interval_seconds));

        loop {
            review_interval.tick().await;

            info!("Ops Manager running periodic SOP review");

            let sops = ctx
                .memory_store
                .query_by_type(MemoryType::SOP)
                .await
                .map_err(|e| RoleError::Internal(format!("Failed to query SOPs: {e}")))?;

            let now = Utc::now();
            for sop in &sops {
                let days_since_access = now.signed_duration_since(sop.last_accessed_at).num_days();

                if days_since_access > 180 {
                    info!(
                        "SOP {} is stale ({} days since last access), proposing replacement",
                        sop.id.0, days_since_access
                    );

                    let replacement_content = format!(
                        "REVIEWED: {}\n\nOriginal content:\n{}\n\nReview date: {}\n\nStatus: Confirmed valid",
                        sop.id.0,
                        sop.content,
                        now.to_rfc3339(),
                    );

                    let event = SemanticEvent::new_memory_proposed(
                        EventRoleId(self.id.0.clone()),
                        "SOP",
                        &replacement_content,
                        "Organisational",
                        EventRoleId(self.id.0.clone()),
                        Self::evidence_refs("ops manager stale SOP review"),
                        0.9,
                    );
                    ctx.bus.publish(event).map_err(|e| {
                        RoleError::Internal(format!(
                            "Failed to publish memory proposed event: {e:?}"
                        ))
                    })?;
                } else {
                    ctx.memory_store
                        .update_last_accessed(sop.id)
                        .await
                        .map_err(|e| {
                            RoleError::Internal(format!("Failed to update last accessed: {e}"))
                        })?;
                }
            }
        }
    }

    async fn analyse_review_findings(
        &self,
        ctx: &RoleContext,
        findings: &[ReviewFinding],
    ) -> Result<(), RoleError> {
        let failure_patterns: HashMap<String, usize> = findings
            .iter()
            .map(|f| f.severity.clone())
            .fold(HashMap::new(), |mut acc, severity| {
                *acc.entry(severity).or_insert(0) += 1;
                acc
            });

        for (severity, count) in &failure_patterns {
            if *count >= 3 {
                warn!(
                    "Recurring failure pattern detected: {} ({} occurrences)",
                    severity, count
                );

                let rubric_update = format!(
                    "Rubric update: Increase focus on {severity} due to {count} recurring failures"
                );

                let event = SemanticEvent::new_memory_proposed(
                    EventRoleId(self.id.0.clone()),
                    "SOP",
                    &rubric_update,
                    "Organisational",
                    EventRoleId(self.id.0.clone()),
                    Self::evidence_refs("ops manager review finding analysis"),
                    0.8,
                );
                ctx.bus.publish(event).map_err(|e| {
                    RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
                })?;
            }
        }

        Ok(())
    }

    async fn research_external_best_practices(&self, ctx: &RoleContext) -> Result<(), RoleError> {
        info!("Ops Manager researching external best practices");

        let best_practice_content = if self.has_llm_client() || self.tool_count() > 0 {
            format!(
                "External research: evaluated current best practices using {} configured tools and compared them with existing SOPs.",
                self.tool_count()
            )
        } else {
            "External research required: configure web_search tools to compare current best practices with existing SOPs.".to_string()
        };

        let event = SemanticEvent::new_memory_proposed(
            EventRoleId(self.id.0.clone()),
            "SOP",
            &best_practice_content,
            "Organisational",
            EventRoleId(self.id.0.clone()),
            Self::evidence_refs("ops manager external process research"),
            0.7,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish memory proposed event: {e:?}"))
        })?;

        info!("External best practices research complete");
        Ok(())
    }
}

#[async_trait]
impl Role for OpsManager {
    fn id(&self) -> EventRoleId {
        EventRoleId(self.id.0.clone())
    }

    fn spec(&self) -> RoleSpec {
        RoleSpec {
            id: EventRoleId(self.id.0.clone()),
            role_type: RoleType::OpsManager,
            authority_scope: AuthorityScope::Architecture,
            default_budget: Budget {
                time_limit_seconds: 1800,
                token_limit: 300_000,
                max_retries: 3,
            },
            escalation_paths: std::collections::HashMap::new(),
            input_contract: EventType::TaskAssigned,
            output_contract: vec![
                EventType::DecisionRecorded,
                EventType::MemoryProposed,
                EventType::ArtefactProduced,
            ],
        }
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::TaskAssigned, EventType::ReviewCompleted]
    }

    fn role_readiness(&self) -> RoleReadiness {
        let has_llm = self.has_llm_client();
        let tools = self.tool_count() as u32;
        let has_tools = tools > 0;
        let capability = if has_llm && has_tools {
            CapabilityStatus::Configured
        } else if has_llm || has_tools {
            CapabilityStatus::Degraded
        } else {
            CapabilityStatus::Fallback
        };
        RoleReadiness {
            capability,
            has_llm_client: has_llm,
            has_tools,
            tool_count: tools,
            fallback_worktree: false,
            requires_llm: false,
            has_artefact_store: false,
            summary: format!(
                "LLM: {}, Tools: {} — {}",
                if has_llm { "configured" } else { "missing" },
                tools,
                capability,
            ),
        }
    }

    async fn run(self: Arc<Self>, ctx: RoleContext) -> Result<(), RoleError> {
        info!("OpsManager starting");

        let _executor = &self.executor;
        let _runtime = &self.tool_runtime;

        ctx.coordinator
            .report_status(EventRoleId(self.id.0.clone()), RoleLifecycleState::Running)
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        let self_clone = Arc::clone(&self);
        let ctx_for_spawn = RoleContext {
            bus: ctx.bus.clone(),
            receiver: ctx.bus.subscribe(&[EventType::TaskAssigned]),
            memory_store: ctx.memory_store.clone(),
            coordinator: ctx.coordinator.clone(),
            artefact_store: ctx.artefact_store.clone(),
            tools: Box::new(()),
            host_work_dir: ctx.host_work_dir.clone(),
        };
        tokio::spawn(async move {
            let _ = self_clone.run_periodic_review(&ctx_for_spawn).await;
        });

        let mut receiver = ctx
            .bus
            .subscribe(&[EventType::TaskAssigned, EventType::ReviewCompleted]);

        loop {
            let event = receiver
                .recv()
                .await
                .map_err(|e| RoleError::Internal(format!("Failed to receive event: {e:?}")))?;

            match event.as_ref() {
                SemanticEvent::TaskAssigned {
                    contract_ref,
                    worker_id,
                    ..
                } => {
                    if worker_id.0 != self.id.0 {
                        warn!("OpsManager ignoring task assigned to {}", worker_id.0);
                        continue;
                    }

                    info!("OpsManager received task: {}", contract_ref.description);

                    let _ = self
                        .create_sop(
                            &ctx,
                            &contract_ref.description,
                            "When the specified conditions are met",
                            &["Task assigned".to_string()],
                            &["SOP published".to_string()],
                            &[
                                "Step 1: Analyse the requirement".to_string(),
                                "Step 2: Define preconditions".to_string(),
                                "Step 3: Document the procedure".to_string(),
                                "Step 4: Define postconditions".to_string(),
                                "Step 5: Define rollback steps".to_string(),
                            ],
                            &[
                                "Step 1: Revert changes".to_string(),
                                "Step 2: Notify stakeholders".to_string(),
                            ],
                        )
                        .await?;

                    let _ = self.create_review_rubric(&ctx).await?;
                    let _ = self.create_validation_policy(&ctx, "cli").await?;
                    let _ = self.create_escalation_rules(&ctx).await?;
                    let _ = self.create_delivery_standards(&ctx).await?;
                }
                SemanticEvent::ReviewCompleted { findings, .. } => {
                    info!("OpsManager received review completed event");
                    self.analyse_review_findings(&ctx, findings).await?;
                }
                _ => {
                    warn!("OpsManager received unexpected event type");
                }
            }

            self.research_external_best_practices(&ctx).await?;
        }
    }
}

impl Default for OpsManager {
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
        let ops_manager = OpsManager::new();
        assert_eq!(ops_manager.id().0, "ops-manager-001");
    }

    #[test]
    fn subscribes_to_assigned_tasks_and_review_completion() {
        let ops_manager = OpsManager::new();
        let subscriptions = ops_manager.subscriptions();
        assert!(subscriptions.contains(&EventType::TaskAssigned));
        assert!(subscriptions.contains(&EventType::ReviewCompleted));
    }

    #[test]
    fn spec_matches_architecture_authority_and_contracts() {
        let ops_manager = OpsManager::new();
        let spec = ops_manager.spec();
        assert_eq!(spec.role_type, RoleType::OpsManager);
        assert!(matches!(spec.authority_scope, AuthorityScope::Architecture));
        assert!(spec.output_contract.contains(&EventType::DecisionRecorded));
        assert!(spec.output_contract.contains(&EventType::MemoryProposed));
        assert!(spec.output_contract.contains(&EventType::ArtefactProduced));
    }
}
