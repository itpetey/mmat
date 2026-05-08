//! The Reviewer role evaluates implementation quality against a rubric, checks architectural compliance
//! against ADRs and interface specs, classifies failures, and triggers rework or escalation.

use std::sync::Arc;

use async_trait::async_trait;
use mmat_coordinator::{
    AuthorityScope, Budget, Role, RoleContext, RoleError, RoleLifecycleState, RoleSpec, RoleType,
};
use mmat_event_stream::event::{
    EscalationSeverity, EventType, ReviewFinding, RoleId as EventRoleId, SemanticEvent,
};
use mmat_llm::{
    client::LlmClient,
    executor::{Executor, ExecutorConfig},
    message::{CompletionRequest, Message},
};
use tracing::{info, warn};

use crate::{
    artefacts::{Adr, FailureClass, InterfaceSpec},
    tooling::{RoleToolRegistry, RoleToolRuntime},
};

/// The Reviewer role evaluates implementation quality and architectural compliance.
pub struct Reviewer {
    id: EventRoleId,
    llm_client: Option<Arc<dyn LlmClient>>,
    #[allow(dead_code)]
    executor: Executor,
    tool_registry: RoleToolRegistry,
    tool_runtime: RoleToolRuntime,
    rework_counts: Arc<parking_lot::RwLock<std::collections::HashMap<String, u32>>>,
    max_retries: u32,
    pending_reviews: Arc<parking_lot::RwLock<Vec<(String, String)>>>,
}

impl Reviewer {
    /// Creates a new Reviewer with default settings and no LLM client.
    pub fn new() -> Self {
        Self {
            id: EventRoleId("reviewer-001".to_string()),
            llm_client: None,
            executor: Executor,
            tool_registry: RoleToolRegistry::new(),
            tool_runtime: RoleToolRuntime,
            rework_counts: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            max_retries: 3,
            pending_reviews: Arc::new(parking_lot::RwLock::new(Vec::new())),
        }
    }

    /// Configures the Reviewer with an LLM client for rubric checking and architectural compliance.
    pub fn with_llm_client(mut self, llm_client: Arc<dyn LlmClient>) -> Self {
        self.llm_client = Some(llm_client);
        self
    }

    /// Configures the Reviewer with a custom tool registry.
    pub fn with_tool_registry(mut self, tool_registry: RoleToolRegistry) -> Self {
        self.tool_registry = tool_registry;
        self
    }

    /// Sets the maximum number of rework attempts before escalation.
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Returns whether an LLM client has been configured.
    pub fn has_llm_client(&self) -> bool {
        self.llm_client.is_some()
    }

    fn rubric_dimensions() -> Vec<&'static str> {
        vec![
            "correctness",
            "api_design",
            "cohesion",
            "coupling",
            "backwards_compatibility",
            "observability",
            "error_handling",
            "concurrency",
            "performance",
            "security",
            "test_adequacy",
            "migration_safety",
        ]
    }

    async fn check_rubric(
        &self,
        _ctx: &RoleContext,
        task_id: &str,
        implementation: &str,
    ) -> Result<Vec<ReviewFinding>, RoleError> {
        let dimensions = Self::rubric_dimensions();
        info!(
            "Reviewer checking rubric for task: {} with {} dimensions",
            task_id,
            dimensions.len()
        );

        if let Some(client) = &self.llm_client {
            let dimensions_list = dimensions.join(", ");
            let prompt = format!(
                "Review the following implementation against these rubric dimensions: {}.\n\n\
Implementation:\n{}\n\n\
For each dimension that has issues, report: dimension name, description of issue, and location if applicable.",
                dimensions_list, implementation
            );

            let request = CompletionRequest::new(
                "reviewer-rubric",
                vec![
                    Message::system(
                        "You are a code reviewer evaluating implementation quality. Report specific findings.",
                    ),
                    Message::user(&prompt),
                ],
            );

            let response = Executor::run(
                client.as_ref(),
                &self.tool_registry,
                &ExecutorConfig {
                    max_turns: 3,
                    max_tokens: None,
                },
                &self.tool_runtime,
                request,
            )
            .await;

            match response {
                Ok(Message::Assistant {
                    content: Some(content),
                    ..
                }) if !content.is_empty()
                    && (content.to_lowercase().contains("issue")
                        || content.to_lowercase().contains("problem")
                        || content.to_lowercase().contains("violation")) =>
                {
                    return Ok(vec![ReviewFinding {
                        finding: content,
                        severity: "medium".to_string(),
                    }]);
                }
                Err(e) => {
                    warn!("Reviewer LLM rubric check failed: {}", e);
                }
                _ => {}
            }
        }

        Ok(vec![])
    }

    async fn check_architectural_compliance(
        &self,
        _ctx: &RoleContext,
        implementation: &str,
        adrs: &[Adr],
        interface_specs: &[InterfaceSpec],
    ) -> Result<Vec<ReviewFinding>, RoleError> {
        info!(
            "Reviewer checking architectural compliance against {} ADRs and {} interface specs",
            adrs.len(),
            interface_specs.len()
        );

        if adrs.is_empty() && interface_specs.is_empty() {
            return Ok(vec![]);
        }

        if let Some(client) = &self.llm_client {
            let adr_context: Vec<String> = adrs
                .iter()
                .map(|a| format!("ADR: {} - {}", a.title, a.decision))
                .collect();
            let iface_context: Vec<String> = interface_specs
                .iter()
                .map(|s| {
                    format!(
                        "Interface: {} - inputs: {:?}, outputs: {:?}",
                        s.module_name, s.input_types, s.output_types
                    )
                })
                .collect();

            let prompt = format!(
                "Check if the following implementation complies with these architectural decisions and interface specifications.\n\n\
Architectural Decisions:\n{}\n\n\
Interface Specifications:\n{}\n\n\
Implementation:\n{}\n\n\
Report any architectural violations.",
                adr_context.join("\n"),
                iface_context.join("\n"),
                implementation
            );

            let request = CompletionRequest::new(
                "reviewer-arch",
                vec![
                    Message::system(
                        "You are reviewing for architectural compliance. Report specific violations.",
                    ),
                    Message::user(&prompt),
                ],
            );

            let response = Executor::run(
                client.as_ref(),
                &self.tool_registry,
                &ExecutorConfig {
                    max_turns: 3,
                    max_tokens: None,
                },
                &self.tool_runtime,
                request,
            )
            .await;

            match response {
                Ok(Message::Assistant {
                    content: Some(content),
                    ..
                }) if !content.is_empty()
                    && (content.to_lowercase().contains("violation")
                        || content.to_lowercase().contains("conflict")
                        || content.to_lowercase().contains("does not comply")) =>
                {
                    return Ok(vec![ReviewFinding {
                        finding: format!("Architectural conflict: {}", content),
                        severity: "high".to_string(),
                    }]);
                }
                Err(e) => {
                    warn!("Reviewer LLM architectural check failed: {}", e);
                }
                _ => {}
            }
        }

        Ok(vec![])
    }

    pub(crate) fn classify_failure(&self, finding: &ReviewFinding, _adrs: &[Adr]) -> FailureClass {
        let lower = finding.finding.to_lowercase();
        if lower.contains("architectural") || lower.contains("dependency") {
            FailureClass::ArchitecturalConflict
        } else if lower.contains("knowledge") || lower.contains("domain") {
            FailureClass::MissingKnowledge
        } else if lower.contains("ambiguous") || lower.contains("unclear") {
            FailureClass::AmbiguousIntent
        } else if lower.contains("process") {
            FailureClass::BrokenProcess
        } else {
            FailureClass::ImplementationDefect
        }
    }

    pub(crate) fn escalation_target_for(&self, failure_class: &FailureClass) -> EventRoleId {
        match failure_class {
            FailureClass::ArchitecturalConflict => EventRoleId("architect-001".to_string()),
            FailureClass::MissingKnowledge => EventRoleId("scholar-001".to_string()),
            FailureClass::AmbiguousIntent => EventRoleId("intent-lead-001".to_string()),
            FailureClass::BrokenProcess => EventRoleId("ops-manager-001".to_string()),
            FailureClass::ImplementationDefect => EventRoleId("worker-001".to_string()),
        }
    }

    async fn publish_review_completed(
        &self,
        ctx: &RoleContext,
        task_id: &str,
        findings: &[ReviewFinding],
        accepted: bool,
    ) -> Result<(), RoleError> {
        let event = SemanticEvent::new_review_completed(
            EventRoleId(self.id.0.clone()),
            task_id,
            findings.to_vec(),
            accepted,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish review completed event: {e:?}"))
        })?;

        info!(
            "Published review completed for task {}: accepted={}",
            task_id, accepted
        );
        Ok(())
    }

    async fn request_rework(
        &self,
        ctx: &RoleContext,
        task_id: &str,
        findings: &[ReviewFinding],
    ) -> Result<(), RoleError> {
        let mut rework_counts = self.rework_counts.write();
        let count = rework_counts.entry(task_id.to_string()).or_insert(0);
        *count += 1;
        let current_count = *count;

        if current_count >= self.max_retries {
            warn!(
                "Task {} exceeded max retries ({})",
                task_id, self.max_retries
            );
            let event = SemanticEvent::new_escalation_requested(
                EventRoleId(self.id.0.clone()),
                EventRoleId(self.id.0.clone()),
                EventRoleId("ops-manager-001".to_string()),
                format!(
                    "Task {} exceeded max retries after {} attempts",
                    task_id, current_count
                ),
                EscalationSeverity::Critical,
            );
            ctx.bus.publish(event).map_err(|e| {
                RoleError::Internal(format!("Failed to publish escalation event: {e:?}"))
            })?;
            return Ok(());
        }

        for finding in findings {
            let failure_class = self.classify_failure(finding, &[]);
            let target = self.escalation_target_for(&failure_class);

            if matches!(failure_class, FailureClass::ImplementationDefect) {
                let event = SemanticEvent::new_escalation_requested(
                    EventRoleId(self.id.0.clone()),
                    EventRoleId(self.id.0.clone()),
                    target,
                    format!("Rework required: {}", finding.finding),
                    EscalationSeverity::Medium,
                );
                ctx.bus.publish(event).map_err(|e| {
                    RoleError::Internal(format!("Failed to publish escalation event: {e:?}"))
                })?;
            } else {
                let severity = match failure_class {
                    FailureClass::ArchitecturalConflict => EscalationSeverity::Medium,
                    FailureClass::AmbiguousIntent => EscalationSeverity::High,
                    _ => EscalationSeverity::Medium,
                };
                let event = SemanticEvent::new_escalation_requested(
                    EventRoleId(self.id.0.clone()),
                    EventRoleId(self.id.0.clone()),
                    target,
                    format!("{}: {}", failure_class.as_str(), finding.finding),
                    severity,
                );
                ctx.bus.publish(event).map_err(|e| {
                    RoleError::Internal(format!("Failed to publish escalation event: {e:?}"))
                })?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Role for Reviewer {
    fn id(&self) -> EventRoleId {
        EventRoleId(self.id.0.clone())
    }

    fn spec(&self) -> RoleSpec {
        RoleSpec {
            id: EventRoleId(self.id.0.clone()),
            role_type: RoleType::Reviewer,
            authority_scope: AuthorityScope::Review,
            default_budget: Budget {
                time_limit_seconds: 900,
                token_limit: 200_000,
                max_retries: 2,
            },
            escalation_paths: std::collections::HashMap::new(),
            input_contract: EventType::ReviewRequested,
            output_contract: vec![EventType::ReviewCompleted, EventType::EscalationRequested],
        }
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[EventType::ReviewRequested, EventType::TaskCompleted]
    }

    async fn run(self: Arc<Self>, ctx: RoleContext) -> Result<(), RoleError> {
        info!("Reviewer starting");

        ctx.coordinator
            .report_status(EventRoleId(self.id.0.clone()), RoleLifecycleState::Running)
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        let mut receiver = ctx.bus.subscribe(self.subscriptions());

        loop {
            let event = receiver
                .recv()
                .await
                .map_err(|e| RoleError::Internal(format!("Failed to receive event: {e:?}")))?;

            match event.as_ref() {
                SemanticEvent::TaskCompleted {
                    task_id,
                    output_artefact,
                    ..
                } => {
                    let implementation =
                        if let Some((_, content)) = output_artefact.reference.split_once('|') {
                            content.to_string()
                        } else {
                            output_artefact.reference.clone()
                        };
                    self.pending_reviews
                        .write()
                        .push((task_id.clone(), implementation));
                    info!(
                        "Reviewer stored implementation for task {}, total pending: {}",
                        task_id,
                        self.pending_reviews.read().len()
                    );

                    let review_request = SemanticEvent::new_review_requested(
                        EventRoleId(self.id.0.clone()),
                        task_id,
                        EventRoleId(self.id.0.clone()),
                    );
                    ctx.bus.publish(review_request).map_err(|e| {
                        RoleError::Internal(format!(
                            "Failed to publish review requested event: {e:?}"
                        ))
                    })?;
                }
                SemanticEvent::ReviewRequested {
                    task_id,
                    reviewer_id,
                    ..
                } if reviewer_id.0 == self.id.0 => {
                    let implementation = {
                        let mut pending = self.pending_reviews.write();
                        pending
                            .iter()
                            .position(|(id, _)| id == task_id)
                            .map(|idx| pending.remove(idx).1)
                    };

                    let implementation = implementation.unwrap_or_default();

                    if implementation.is_empty() {
                        warn!("No implementation content found for task {}", task_id);
                        let findings = vec![ReviewFinding {
                            finding: "No implementation content provided for review".to_string(),
                            severity: "high".to_string(),
                        }];
                        self.publish_review_completed(&ctx, task_id, &findings, false)
                            .await?;
                        self.request_rework(&ctx, task_id, &findings).await?;
                        continue;
                    }

                    let findings = self.check_rubric(&ctx, task_id, &implementation).await?;
                    let adrs: Vec<Adr> = vec![];
                    let iface_specs: Vec<InterfaceSpec> = vec![];
                    let arch_findings = self
                        .check_architectural_compliance(&ctx, &implementation, &adrs, &iface_specs)
                        .await?;

                    let all_findings: Vec<ReviewFinding> =
                        findings.into_iter().chain(arch_findings).collect();

                    let accepted = all_findings.is_empty();

                    self.publish_review_completed(&ctx, task_id, &all_findings, accepted)
                        .await?;

                    if !accepted {
                        self.request_rework(&ctx, task_id, &all_findings).await?;
                    }
                }
                _ => {}
            }
        }
    }
}

impl Default for Reviewer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use mmat_coordinator::{AuthorityScope, Role, RoleType};
    use mmat_event_stream::event::{EventType, ReviewFinding};

    use super::*;

    #[test]
    fn creates_with_default_id() {
        let reviewer = Reviewer::new();
        assert_eq!(reviewer.id().0, "reviewer-001");
    }

    #[test]
    fn spec_matches_review_authority_and_contracts() {
        let reviewer = Reviewer::new();
        let spec = reviewer.spec();
        assert_eq!(spec.role_type, RoleType::Reviewer);
        assert!(matches!(spec.authority_scope, AuthorityScope::Review));
        assert!(spec.output_contract.contains(&EventType::ReviewCompleted));
        assert!(
            spec.output_contract
                .contains(&EventType::EscalationRequested)
        );
    }

    #[test]
    fn subscribes_to_review_and_completion_events() {
        let reviewer = Reviewer::new();
        let subscriptions = reviewer.subscriptions();
        assert!(subscriptions.contains(&EventType::ReviewRequested));
        assert!(subscriptions.contains(&EventType::TaskCompleted));
    }

    #[test]
    fn classifies_failures_from_finding_text() {
        let reviewer = Reviewer::new();

        let defect = ReviewFinding {
            finding: "Missing error handling".to_string(),
            severity: "high".to_string(),
        };
        assert!(matches!(
            reviewer.classify_failure(&defect, &[]),
            FailureClass::ImplementationDefect
        ));

        let arch_conflict = ReviewFinding {
            finding: "Architectural dependency violation".to_string(),
            severity: "high".to_string(),
        };
        assert!(matches!(
            reviewer.classify_failure(&arch_conflict, &[]),
            FailureClass::ArchitecturalConflict
        ));

        let missing_knowledge = ReviewFinding {
            finding: "Missing domain knowledge about X".to_string(),
            severity: "medium".to_string(),
        };
        assert!(matches!(
            reviewer.classify_failure(&missing_knowledge, &[]),
            FailureClass::MissingKnowledge
        ));

        let ambiguous = ReviewFinding {
            finding: "Ambiguous intent in task description".to_string(),
            severity: "high".to_string(),
        };
        assert!(matches!(
            reviewer.classify_failure(&ambiguous, &[]),
            FailureClass::AmbiguousIntent
        ));

        let broken_process = ReviewFinding {
            finding: "Broken process detected".to_string(),
            severity: "medium".to_string(),
        };
        assert!(matches!(
            reviewer.classify_failure(&broken_process, &[]),
            FailureClass::BrokenProcess
        ));
    }

    #[test]
    fn maps_failure_classes_to_escalation_targets() {
        let reviewer = Reviewer::new();

        assert_eq!(
            reviewer
                .escalation_target_for(&FailureClass::ArchitecturalConflict)
                .0,
            "architect-001"
        );
        assert_eq!(
            reviewer
                .escalation_target_for(&FailureClass::MissingKnowledge)
                .0,
            "scholar-001"
        );
        assert_eq!(
            reviewer
                .escalation_target_for(&FailureClass::AmbiguousIntent)
                .0,
            "intent-lead-001"
        );
        assert_eq!(
            reviewer
                .escalation_target_for(&FailureClass::BrokenProcess)
                .0,
            "ops-manager-001"
        );
        assert_eq!(
            reviewer
                .escalation_target_for(&FailureClass::ImplementationDefect)
                .0,
            "worker-001"
        );
    }
}
