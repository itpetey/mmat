//! The Auditor role continuously monitors events for policy violations, evidence chain integrity,
//! process adherence, confidence justification, and authority scope enforcement.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use coordinator::{
    AuthorityScope, Budget, Role, RoleContext, RoleError, RoleLifecycleState, RoleSpec, RoleType,
};
use event_stream::event::{EventId, EventType, EvidenceRef, RoleId as EventRoleId, SemanticEvent};
use llm::client::LlmClient;
use llm::message::{CompletionRequest, Message};
use memory::provenance::ProvenanceEngine;
use tracing::{info, warn};

use crate::artefacts::{
    AuditReport, ConfidenceAssessment, EvidenceChainStatus, EvidencePack, ProcessAdherenceCheck,
};

/// The Auditor role monitors the organisation for policy violations and evidence integrity.
pub struct Auditor {
    id: EventRoleId,
    provenance_engine: ProvenanceEngine,
    authority_registry: HashMap<EventRoleId, AuthorityScope>,
    llm_client: Option<Arc<dyn LlmClient>>,
    llm_config: AuditorLlmConfig,
    llm_checks_this_cycle: parking_lot::Mutex<u32>,
    source_verification_enabled: bool,
    http_client: reqwest::Client,
    violation_counts: parking_lot::Mutex<HashMap<String, u32>>,
    evidence_chain_statuses: parking_lot::Mutex<Vec<EvidenceChainStatus>>,
    process_checks: parking_lot::Mutex<Vec<ProcessAdherenceCheck>>,
    confidence_assessments: parking_lot::Mutex<Vec<ConfidenceAssessment>>,
    report_interval_seconds: u64,
    last_report_time: parking_lot::Mutex<std::time::Instant>,
}

/// Configuration for the Auditor's LLM-based semantic checks.
#[derive(Clone, Debug)]
pub struct AuditorLlmConfig {
    /// Whether LLM-based checks are enabled.
    pub enabled: bool,
    /// The model identifier to use for checks.
    pub model: String,
    /// Maximum number of LLM checks per audit cycle.
    pub max_checks_per_cycle: u32,
}

impl Default for AuditorLlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "gpt-4.1-mini".to_string(),
            max_checks_per_cycle: 3,
        }
    }
}

impl Auditor {
    /// Creates a new Auditor with default authority registry, no LLM client, and source verification disabled.
    pub fn new() -> Self {
        let mut authority_registry = HashMap::new();
        authority_registry.insert(
            EventRoleId("intent-lead-001".to_string()),
            AuthorityScope::IntentOnly,
        );
        authority_registry.insert(
            EventRoleId("scholar-001".to_string()),
            AuthorityScope::Architecture,
        );
        authority_registry.insert(
            EventRoleId("ops-manager-001".to_string()),
            AuthorityScope::Architecture,
        );
        authority_registry.insert(
            EventRoleId("architect-001".to_string()),
            AuthorityScope::Architecture,
        );
        authority_registry.insert(EventRoleId("pm-001".to_string()), AuthorityScope::Planning);
        authority_registry.insert(
            EventRoleId("worker-001".to_string()),
            AuthorityScope::Implementation,
        );
        authority_registry.insert(
            EventRoleId("reviewer-001".to_string()),
            AuthorityScope::Review,
        );
        authority_registry.insert(
            EventRoleId("auditor-001".to_string()),
            AuthorityScope::Audit,
        );
        authority_registry.insert(
            EventRoleId("system".to_string()),
            AuthorityScope::FullAccess,
        );

        Self {
            id: EventRoleId("auditor-001".to_string()),
            provenance_engine: ProvenanceEngine::new(),
            authority_registry,
            llm_client: None,
            llm_config: AuditorLlmConfig::default(),
            llm_checks_this_cycle: parking_lot::Mutex::new(0),
            source_verification_enabled: false,
            http_client: reqwest::Client::new(),
            violation_counts: parking_lot::Mutex::new(HashMap::new()),
            evidence_chain_statuses: parking_lot::Mutex::new(Vec::new()),
            process_checks: parking_lot::Mutex::new(Vec::new()),
            confidence_assessments: parking_lot::Mutex::new(Vec::new()),
            report_interval_seconds: 3600,
            last_report_time: parking_lot::Mutex::new(std::time::Instant::now()),
        }
    }

    /// Sets the interval in seconds between periodic audit reports.
    pub fn with_report_interval(mut self, seconds: u64) -> Self {
        self.report_interval_seconds = seconds;
        self
    }

    /// Configures the Auditor with an LLM client for semantic consistency checks.
    pub fn with_llm_client(mut self, client: Arc<dyn LlmClient>) -> Self {
        self.llm_client = Some(client);
        self
    }

    /// Configures the Auditor with LLM settings.
    pub fn with_llm_config(mut self, config: AuditorLlmConfig) -> Self {
        self.llm_config = config;
        self
    }

    /// Enables or disables source verification (e.g. checking reachability of web sources).
    pub fn with_source_verification(mut self, enabled: bool) -> Self {
        self.source_verification_enabled = enabled;
        self
    }

    /// Registers a role's authority scope in the auditor's registry.
    pub fn register_authority(&mut self, role_id: EventRoleId, scope: AuthorityScope) {
        self.authority_registry.insert(role_id, scope);
    }

    /// Registers authority scopes from a set of role specifications.
    pub fn with_role_specs(mut self, specs: &[RoleSpec]) -> Self {
        for spec in specs {
            self.authority_registry
                .insert(spec.id.clone(), spec.authority_scope.clone());
        }
        self
    }

    fn record_violation(&self, violation_type: &str) {
        let mut counts = self.violation_counts.lock();
        *counts.entry(violation_type.to_string()).or_insert(0) += 1;
    }

    async fn publish_evidence_chain_broken(
        &self,
        ctx: &RoleContext,
        claim_id: EventId,
        broken_ref: EventId,
        claim_text: &str,
    ) -> Result<(), RoleError> {
        let event = SemanticEvent::new_evidence_chain_broken(
            EventRoleId(self.id.0.clone()),
            claim_id,
            broken_ref,
            claim_text,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish EvidenceChainBroken: {e:?}"))
        })?;
        self.record_violation("evidence_chain_broken");
        Ok(())
    }

    async fn publish_process_skipped(
        &self,
        ctx: &RoleContext,
        step: &str,
        claim_id: EventId,
    ) -> Result<(), RoleError> {
        let event =
            SemanticEvent::new_process_skipped(EventRoleId(self.id.0.clone()), step, claim_id);
        ctx.bus
            .publish(event)
            .map_err(|e| RoleError::Internal(format!("Failed to publish ProcessSkipped: {e:?}")))?;
        self.record_violation("process_skipped");
        Ok(())
    }

    async fn publish_policy_violation(
        &self,
        ctx: &RoleContext,
        violation_type: &str,
        description: &str,
        related_event_id: Option<EventId>,
    ) -> Result<(), RoleError> {
        let event = SemanticEvent::new_policy_violation_detected(
            EventRoleId(self.id.0.clone()),
            violation_type,
            description,
            related_event_id,
        );
        ctx.bus.publish(event).map_err(|e| {
            RoleError::Internal(format!("Failed to publish PolicyViolationDetected: {e:?}"))
        })?;
        self.record_violation(violation_type);
        Ok(())
    }

    async fn check_evidence_references(
        &self,
        ctx: &RoleContext,
        claim_id: EventId,
        evidence_refs: &[EvidenceRef],
        claim_text: &str,
    ) -> Result<(), RoleError> {
        let Some(store) = ctx.bus.store() else {
            return Ok(());
        };

        let mut checked = Vec::new();
        let mut broken = Vec::new();

        for ev_ref in evidence_refs {
            checked.push(ev_ref.event_id.to_string());
            let referenced = store
                .get_by_event_id(ev_ref.event_id)
                .map_err(|e| RoleError::Internal(format!("Event store query failed: {e}")))?;

            let valid_tool_reference =
                matches!(referenced, Some(SemanticEvent::ToolExecuted { .. }));
            if !valid_tool_reference {
                broken.push(ev_ref.event_id.to_string());
                self.publish_evidence_chain_broken(ctx, claim_id, ev_ref.event_id, claim_text)
                    .await?;
            }
        }

        if !checked.is_empty() {
            let status = if broken.is_empty() {
                "intact"
            } else {
                "broken"
            };
            self.evidence_chain_statuses
                .lock()
                .push(EvidenceChainStatus {
                    claim_id: claim_id.to_string(),
                    evidence_refs_checked: checked,
                    broken_refs: broken,
                    status: status.to_string(),
                });
        }

        Ok(())
    }

    fn should_run_semantic_check(&self, claim_text: &str, evidence_refs: &[EvidenceRef]) -> bool {
        if !self.llm_config.enabled || self.llm_client.is_none() || evidence_refs.is_empty() {
            return false;
        }

        let claim_lower = claim_text.to_lowercase();
        let deterministic_phrases = [
            "tests passed",
            "build succeeded",
            "compilation succeeded",
            "fmt passed",
            "formatting passed",
        ];
        !deterministic_phrases
            .iter()
            .any(|phrase| claim_lower.contains(phrase))
    }

    fn reserve_llm_check(&self) -> bool {
        let mut checks = self.llm_checks_this_cycle.lock();
        if *checks >= self.llm_config.max_checks_per_cycle {
            return false;
        }

        *checks += 1;
        true
    }

    async fn run_semantic_consistency_check(
        &self,
        ctx: &RoleContext,
        claim_id: EventId,
        claim_text: &str,
        evidence_text: &str,
    ) -> Result<(), RoleError> {
        let Some(client) = &self.llm_client else {
            return Ok(());
        };
        if !self.reserve_llm_check() {
            return Ok(());
        }

        let request = CompletionRequest {
            model: self.llm_config.model.clone(),
            messages: vec![
                Message::System {
                    content: "You are a sceptical Auditor. Decide whether the claim is strictly supported by the tool output. Reply with exactly one leading word: CONSISTENT, INCONSISTENT, or AMBIGUOUS. Treat missing or contradictory support as INCONSISTENT.".to_string(),
                    name: None,
                },
                Message::User {
                    content: format!("Claim:\n{claim_text}\n\nTool output:\n{evidence_text}"),
                    name: None,
                },
            ],
            tools: None,
            tool_choice: None,
            temperature: Some(0.0),
            max_tokens: Some(16),
            stream: None,
            stream_options: None,
        };

        let response = client
            .complete(request)
            .await
            .map_err(|e| RoleError::Internal(format!("LLM semantic check failed: {e}")))?;
        let verdict = response
            .choices
            .first()
            .and_then(|choice| match &choice.message {
                Message::Assistant { content, .. } => content.clone(),
                _ => None,
            })
            .unwrap_or_default()
            .to_lowercase();

        if verdict.trim_start().starts_with("inconsistent") {
            self.publish_policy_violation(
                ctx,
                "semantic_inconsistency",
                "Claim is not semantically supported by cited tool output",
                Some(claim_id),
            )
            .await?;
        }

        Ok(())
    }

    async fn check_evidence_consistency(
        &self,
        ctx: &RoleContext,
        claim_id: EventId,
        evidence_refs: &[EvidenceRef],
        claim_text: &str,
    ) -> Result<(), RoleError> {
        let Some(store) = ctx.bus.store() else {
            return Ok(());
        };

        let mut cited_tool_output = String::new();

        for ev_ref in evidence_refs {
            let Some(event) = store
                .get_by_event_id(ev_ref.event_id)
                .map_err(|e| RoleError::Internal(format!("Event store query failed: {e}")))?
            else {
                continue;
            };

            if let SemanticEvent::ToolExecuted {
                exit_code,
                stdout,
                stderr,
                tool_name,
                ..
            } = event
            {
                let claim_lower = claim_text.to_lowercase();
                let asserts_success = claim_lower.contains("passed")
                    || claim_lower.contains("succeeded")
                    || claim_lower.contains("success");
                let asserts_failure = claim_lower.contains("failed")
                    || claim_lower.contains("failure")
                    || claim_lower.contains("error");

                if asserts_success && exit_code != 0 {
                    self.publish_policy_violation(
                        ctx,
                        "contradiction",
                        &format!("Claim asserts success but cited tool has exit_code {exit_code}"),
                        Some(claim_id),
                    )
                    .await?;
                }

                if asserts_failure && exit_code == 0 {
                    self.publish_policy_violation(
                        ctx,
                        "contradiction",
                        "Claim asserts failure but cited tool has exit_code 0",
                        Some(claim_id),
                    )
                    .await?;
                }

                cited_tool_output.push_str(&format!(
                    "tool: {tool_name}\nstdout:\n{stdout}\nstderr:\n{stderr}\n"
                ));
            }
        }

        if self.should_run_semantic_check(claim_text, evidence_refs)
            && !cited_tool_output.is_empty()
        {
            self.run_semantic_consistency_check(ctx, claim_id, claim_text, &cited_tool_output)
                .await?;
        }

        Ok(())
    }

    async fn check_process_adherence(
        &self,
        ctx: &RoleContext,
        claim_id: EventId,
        claim_text: &str,
        evidence_refs: &[EvidenceRef],
        source_agent: &EventRoleId,
    ) -> Result<(), RoleError> {
        let Some(store) = ctx.bus.store() else {
            return Ok(());
        };

        let claim_lower = claim_text.to_lowercase();

        let required_tools: Vec<&str> = if claim_lower.contains("tests passed") {
            vec!["cargo test"]
        } else if claim_lower.contains("build succeeded")
            || claim_lower.contains("compilation succeeded")
        {
            vec!["cargo build"]
        } else if claim_lower.contains("fmt passed") || claim_lower.contains("formatting passed") {
            vec!["cargo fmt"]
        } else {
            vec![]
        };

        if required_tools.is_empty() {
            return Ok(());
        }

        for required in &required_tools {
            let mut found_in_evidence = false;
            let mut temporal_valid = false;

            // First, check cited evidence refs for matching tool executions
            for ev_ref in evidence_refs {
                if let Some(SemanticEvent::ToolExecuted {
                    tool_name,
                    exit_code,
                    event_id: tool_event_id,
                    source_agent: tool_agent,
                    ..
                }) = store
                    .get_by_event_id(ev_ref.event_id)
                    .map_err(|e| RoleError::Internal(format!("Event store query failed: {e}")))?
                    && tool_name.contains(required.trim())
                    && tool_agent.0 == source_agent.0
                {
                    found_in_evidence = true;

                    let tool_row = store.row_for_event_id(tool_event_id).map_err(|e| {
                        RoleError::Internal(format!("Event store query failed: {e}"))
                    })?;
                    let claim_row = store.row_for_event_id(claim_id).map_err(|e| {
                        RoleError::Internal(format!("Event store query failed: {e}"))
                    })?;

                    if let (Some(t_row), Some(c_row)) = (tool_row, claim_row)
                        && t_row < c_row
                    {
                        temporal_valid = true;
                    }

                    if exit_code != 0 {
                        self.publish_policy_violation(
                            ctx,
                            "contradiction",
                            &format!(
                                "Required tool '{required}' executed with exit_code {exit_code}"
                            ),
                            Some(claim_id),
                        )
                        .await?;
                    }
                }
            }

            let found = found_in_evidence;

            self.process_checks.lock().push(ProcessAdherenceCheck {
                claim_id: claim_id.to_string(),
                required_step: required.to_string(),
                found,
                temporal_order_valid: temporal_valid,
            });

            if !found {
                self.publish_process_skipped(ctx, required, claim_id)
                    .await?;
            } else if !temporal_valid {
                self.publish_policy_violation(
                    ctx,
                    "temporal_violation",
                    &format!("Required tool '{required}' did not occur before the claim"),
                    Some(claim_id),
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn verify_web_source(
        &self,
        ctx: &RoleContext,
        event_id: EventId,
        source: &str,
    ) -> Result<(), RoleError> {
        if !self.source_verification_enabled {
            return Ok(());
        }

        let valid = match self.http_client.head(source).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        };

        if !valid {
            self.publish_evidence_chain_broken(
                ctx,
                event_id,
                event_id,
                &format!("EvidencePack references unreachable URL: {source}"),
            )
            .await?;
        }

        Ok(())
    }

    fn extract_endpoint_paths(claim_text: &str) -> Vec<String> {
        claim_text
            .split_whitespace()
            .filter_map(|word| {
                let candidate = word.trim_matches(|c: char| {
                    matches!(c, '`' | '\'' | '"' | ',' | '.' | ':' | ';' | ')' | '(')
                });
                if candidate.starts_with('/') && candidate.len() > 1 {
                    Some(candidate.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    fn repository_contains(needle: &str) -> bool {
        fn visit(dir: &Path, needle: &str) -> bool {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return false;
            };

            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();

                if path.is_dir() {
                    if matches!(file_name.as_ref(), ".git" | "target" | "archive") {
                        continue;
                    }
                    if visit(&path, needle) {
                        return true;
                    }
                } else if path.is_file()
                    && let Ok(contents) = std::fs::read_to_string(&path)
                    && contents.contains(needle)
                {
                    return true;
                }
            }

            false
        }

        std::env::current_dir().is_ok_and(|dir| visit(&dir, needle))
    }

    async fn check_evidence_pack_paths(
        &self,
        ctx: &RoleContext,
        event_id: EventId,
        reference: &str,
    ) -> Result<(), RoleError> {
        let Some((_, payload)) = reference.split_once('|') else {
            return Ok(());
        };
        let Ok(pack) = serde_json::from_str::<EvidencePack>(payload) else {
            return Ok(());
        };

        for finding in pack.findings {
            let source = finding.source_reference.trim();
            if source.is_empty() {
                continue;
            }
            if source.starts_with("http://") || source.starts_with("https://") {
                self.verify_web_source(ctx, event_id, source).await?;
                continue;
            }
            if (source.contains('/') || source.contains('.')) && !Path::new(source).exists() {
                self.publish_evidence_chain_broken(
                    ctx,
                    event_id,
                    event_id,
                    &format!("EvidencePack references non-existent path: {source}"),
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn check_hallucinations(
        &self,
        ctx: &RoleContext,
        claim_id: EventId,
        claim_text: &str,
        evidence_refs: &[EvidenceRef],
    ) -> Result<(), RoleError> {
        for ev_ref in evidence_refs {
            if ev_ref.description.contains("file") || ev_ref.description.contains("path") {
                let potential_paths: Vec<&str> = claim_text
                    .split_whitespace()
                    .filter(|w| w.contains('/') || w.contains(".rs"))
                    .collect();

                for path_str in potential_paths {
                    let trimmed = path_str.trim_matches(|c| c == '`' || c == '\'' || c == '"');
                    if !trimmed.is_empty() && !Path::new(trimmed).exists() {
                        self.publish_evidence_chain_broken(
                            ctx,
                            claim_id,
                            ev_ref.event_id,
                            &format!("Referenced path does not exist: {trimmed}"),
                        )
                        .await?;
                    }
                }
            }
        }

        let claim_lower = claim_text.to_lowercase();
        if claim_lower.contains("api supports")
            || claim_lower.contains("endpoint")
            || claim_lower.contains("capability")
        {
            for endpoint in Self::extract_endpoint_paths(claim_text) {
                if !Self::repository_contains(&endpoint) {
                    self.publish_policy_violation(
                        ctx,
                        "hallucinated_capability",
                        &format!(
                            "Claimed API endpoint is not present in repository state: {endpoint}"
                        ),
                        Some(claim_id),
                    )
                    .await?;
                }
            }
        }

        Ok(())
    }

    async fn check_confidence(
        &self,
        ctx: &RoleContext,
        claim_id: EventId,
        confidence_score: f32,
        evidence_refs: &[EvidenceRef],
    ) -> Result<(), RoleError> {
        let evidence_strength = if evidence_refs.is_empty() {
            "none"
        } else {
            "present"
        };

        if confidence_score >= 0.8 && evidence_refs.is_empty() {
            self.publish_policy_violation(
                ctx,
                "unjustified_confidence",
                &format!("High confidence ({confidence_score}) with no evidence references"),
                Some(claim_id),
            )
            .await?;

            self.confidence_assessments
                .lock()
                .push(ConfidenceAssessment {
                    claim_id: claim_id.to_string(),
                    claimed_confidence: confidence_score as f64,
                    evidence_strength: evidence_strength.to_string(),
                    assessment: "unjustified_high".to_string(),
                });
        }

        if confidence_score <= 0.3 && self.has_strong_evidence(ctx, evidence_refs)? {
            self.confidence_assessments
                .lock()
                .push(ConfidenceAssessment {
                    claim_id: claim_id.to_string(),
                    claimed_confidence: confidence_score as f64,
                    evidence_strength: evidence_strength.to_string(),
                    assessment: "low_with_evidence".to_string(),
                });
        }

        Ok(())
    }

    fn has_strong_evidence(
        &self,
        ctx: &RoleContext,
        evidence_refs: &[EvidenceRef],
    ) -> Result<bool, RoleError> {
        let Some(store) = ctx.bus.store() else {
            return Ok(false);
        };

        for ev_ref in evidence_refs {
            if let Some(SemanticEvent::ToolExecuted { exit_code: 0, .. }) = store
                .get_by_event_id(ev_ref.event_id)
                .map_err(|e| RoleError::Internal(format!("Event store query failed: {e}")))?
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn check_authority(
        &self,
        ctx: &RoleContext,
        source_agent: &EventRoleId,
        event_type: &EventType,
        event_id: EventId,
    ) -> Result<(), RoleError> {
        let Some(scope) = self.authority_registry.get(source_agent).cloned() else {
            self.publish_policy_violation(
                ctx,
                "authority_boundary_exceeded",
                &format!("Role {source_agent} is not registered in the authority scope registry"),
                Some(event_id),
            )
            .await?;
            return Ok(());
        };

        if !scope.can_publish(event_type) {
            self.publish_policy_violation(
                ctx,
                "authority_boundary_exceeded",
                &format!(
                    "Role {source_agent} with scope {scope:?} attempted to publish {event_type:?}"
                ),
                Some(event_id),
            )
            .await?;
        }

        Ok(())
    }

    async fn check_memory_contamination(
        &self,
        ctx: &RoleContext,
        memory_event_id: EventId,
    ) -> Result<(), RoleError> {
        let Some(store) = ctx.bus.store() else {
            return Ok(());
        };

        let memory_id = memory::types::MemoryId(memory_event_id.0);
        let Some(memory) = ctx
            .memory_store
            .get_by_id(memory_id)
            .map_err(|e| RoleError::Internal(format!("Memory store query failed: {e}")))?
        else {
            return Ok(());
        };

        for ev_ref in &memory.evidence_refs {
            if let Some(SemanticEvent::ClaimMade {
                event_id: claim_id, ..
            }) = store
                .get_by_event_id(*ev_ref)
                .map_err(|e| RoleError::Internal(format!("Event store query failed: {e}")))?
            {
                let broken = self
                    .provenance_engine
                    .check_broken_evidence(claim_id, &store)
                    .map_err(|e| RoleError::Internal(format!("Provenance check failed: {e}")))?;

                if !broken.is_empty() {
                    self.publish_policy_violation(
                        ctx,
                        "memory_contamination",
                        &format!(
                            "Memory {} derives from claim with broken evidence",
                            memory.id
                        ),
                        Some(memory_event_id),
                    )
                    .await?;
                }
            }
        }

        Ok(())
    }

    async fn publish_audit_report(&self, ctx: &RoleContext) -> Result<(), RoleError> {
        let counts = self.violation_counts.lock().clone();
        let evidence_statuses = self.evidence_chain_statuses.lock().clone();
        let process_checks = self.process_checks.lock().clone();
        let confidence_assessments = self.confidence_assessments.lock().clone();

        let total_violations: u32 = counts.values().sum();
        let summary = if total_violations == 0 {
            "No violations detected in this audit cycle.".to_string()
        } else {
            format!(
                "{total_violations} violation(s) detected across {} type(s).",
                counts.len()
            )
        };

        let report = AuditReport {
            report_id: format!("audit-report-{}", uuid::Uuid::new_v4()),
            violation_counts: counts,
            evidence_chain_statuses: evidence_statuses,
            process_checks,
            confidence_assessments,
            summary,
        };

        let reference = serde_json::to_string(&report)
            .map_err(|e| RoleError::Internal(format!("Failed to serialise audit report: {e}")))?;

        let event = SemanticEvent::new_artefact_produced(
            EventRoleId(self.id.0.clone()),
            "audit_report",
            reference,
            EventRoleId(self.id.0.clone()),
        );
        ctx.bus
            .publish(event)
            .map_err(|e| RoleError::Internal(format!("Failed to publish audit report: {e:?}")))?;

        // Clear cycle tracking after report publication
        self.violation_counts.lock().clear();
        self.evidence_chain_statuses.lock().clear();
        self.process_checks.lock().clear();
        self.confidence_assessments.lock().clear();
        *self.llm_checks_this_cycle.lock() = 0;

        info!("Auditor published periodic audit report");
        Ok(())
    }

    async fn maybe_publish_report(&self, ctx: &RoleContext) -> Result<(), RoleError> {
        let now = std::time::Instant::now();
        let should_publish = {
            let last = self.last_report_time.lock();
            now.duration_since(*last).as_secs() >= self.report_interval_seconds
        };
        if should_publish {
            self.publish_audit_report(ctx).await?;
            let mut last = self.last_report_time.lock();
            *last = now;
        }
        Ok(())
    }

    async fn handle_event(
        &self,
        ctx: &RoleContext,
        event: &SemanticEvent,
    ) -> Result<(), RoleError> {
        let source_agent = match event {
            SemanticEvent::ToolExecuted { source_agent, .. } => source_agent.clone(),
            SemanticEvent::ClaimMade { source_agent, .. } => source_agent.clone(),
            SemanticEvent::DecisionRecorded { source_agent, .. } => source_agent.clone(),
            SemanticEvent::MemoryProposed { source_agent, .. } => source_agent.clone(),
            SemanticEvent::MemoryAccepted { source_agent, .. } => source_agent.clone(),
            SemanticEvent::MemoryRejected { source_agent, .. } => source_agent.clone(),
            SemanticEvent::MemorySuperseded { source_agent, .. } => source_agent.clone(),
            SemanticEvent::EvidenceChainBroken { source_agent, .. } => source_agent.clone(),
            SemanticEvent::ProcessSkipped { source_agent, .. } => source_agent.clone(),
            SemanticEvent::PolicyViolationDetected { source_agent, .. } => source_agent.clone(),
            SemanticEvent::TaskAssigned { source_agent, .. } => source_agent.clone(),
            SemanticEvent::TaskStarted { source_agent, .. } => source_agent.clone(),
            SemanticEvent::TaskCompleted { source_agent, .. } => source_agent.clone(),
            SemanticEvent::TaskFailed { source_agent, .. } => source_agent.clone(),
            SemanticEvent::ReviewRequested { source_agent, .. } => source_agent.clone(),
            SemanticEvent::ReviewCompleted { source_agent, .. } => source_agent.clone(),
            SemanticEvent::EscalationRequested { source_agent, .. } => source_agent.clone(),
            SemanticEvent::HumanFeedbackRequested { source_agent, .. } => source_agent.clone(),
            SemanticEvent::HumanFeedbackReceived { source_agent, .. } => source_agent.clone(),
            SemanticEvent::ArtefactProduced { source_agent, .. } => source_agent.clone(),
            SemanticEvent::BudgetWarning { source_agent, .. } => source_agent.clone(),
            SemanticEvent::EscalationAccepted { source_agent, .. } => source_agent.clone(),
            SemanticEvent::RoleStateChanged { source_agent, .. } => source_agent.clone(),
            SemanticEvent::OrganisationStarted { source_agent, .. } => source_agent.clone(),
            SemanticEvent::OrganisationStopped { source_agent, .. } => source_agent.clone(),
            SemanticEvent::Heartbeat { source_agent, .. } => source_agent.clone(),
        };

        self.check_authority(ctx, &source_agent, &event.event_type(), event.event_id())
            .await?;

        if let SemanticEvent::ClaimMade {
            event_id,
            claim_text,
            evidence_refs,
            confidence_score,
            ..
        } = event
        {
            self.check_evidence_references(ctx, *event_id, evidence_refs, claim_text)
                .await?;
            self.check_evidence_consistency(ctx, *event_id, evidence_refs, claim_text)
                .await?;
            self.check_process_adherence(ctx, *event_id, claim_text, evidence_refs, &source_agent)
                .await?;
            self.check_hallucinations(ctx, *event_id, claim_text, evidence_refs)
                .await?;
            self.check_confidence(ctx, *event_id, *confidence_score, evidence_refs)
                .await?;
        }

        if let SemanticEvent::MemoryAccepted { memory_id, .. } = event {
            self.check_memory_contamination(ctx, *memory_id).await?;
        }

        if let SemanticEvent::ArtefactProduced {
            event_id,
            artefact_type,
            reference,
            ..
        } = event
            && artefact_type == "evidence_pack"
        {
            self.check_evidence_pack_paths(ctx, *event_id, reference)
                .await?;
        }

        if let SemanticEvent::TaskCompleted { .. } = event {
            self.publish_audit_report(ctx).await?;
            let mut last = self.last_report_time.lock();
            *last = std::time::Instant::now();
        }

        self.maybe_publish_report(ctx).await?;

        Ok(())
    }
}

#[async_trait]
impl Role for Auditor {
    fn id(&self) -> EventRoleId {
        EventRoleId(self.id.0.clone())
    }

    fn spec(&self) -> RoleSpec {
        RoleSpec {
            id: EventRoleId(self.id.0.clone()),
            role_type: RoleType::Auditor,
            authority_scope: AuthorityScope::Audit,
            default_budget: Budget {
                time_limit_seconds: 3600,
                token_limit: 100_000,
                max_retries: 1,
            },
            escalation_paths: std::collections::HashMap::new(),
            input_contract: EventType::OrganisationStarted,
            output_contract: vec![
                EventType::PolicyViolationDetected,
                EventType::EvidenceChainBroken,
                EventType::ProcessSkipped,
                EventType::ArtefactProduced,
            ],
        }
    }

    fn subscriptions(&self) -> &'static [EventType] {
        &[
            EventType::ToolExecuted,
            EventType::ClaimMade,
            EventType::DecisionRecorded,
            EventType::MemoryProposed,
            EventType::MemoryAccepted,
            EventType::MemoryRejected,
            EventType::MemorySuperseded,
            EventType::EvidenceChainBroken,
            EventType::ProcessSkipped,
            EventType::PolicyViolationDetected,
            EventType::TaskAssigned,
            EventType::TaskStarted,
            EventType::TaskCompleted,
            EventType::TaskFailed,
            EventType::ReviewRequested,
            EventType::ReviewCompleted,
            EventType::EscalationRequested,
            EventType::HumanFeedbackRequested,
            EventType::HumanFeedbackReceived,
            EventType::ArtefactProduced,
            EventType::BudgetWarning,
            EventType::EscalationAccepted,
            EventType::RoleStateChanged,
            EventType::OrganisationStarted,
            EventType::OrganisationStopped,
            EventType::Heartbeat,
        ]
    }

    async fn run(self: Arc<Self>, ctx: RoleContext) -> Result<(), RoleError> {
        info!("Auditor starting");

        ctx.coordinator
            .report_status(EventRoleId(self.id.0.clone()), RoleLifecycleState::Running)
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        let mut receiver = ctx.bus.subscribe(self.subscriptions());

        loop {
            let event = match receiver.recv().await {
                Ok(event) => event,
                Err(_) => break,
            };

            if let Err(e) = self.handle_event(&ctx, &event).await {
                warn!(
                    "Auditor failed to process event {}: {}",
                    event.event_id(),
                    e
                );
            }
        }

        ctx.coordinator
            .report_status(
                EventRoleId(self.id.0.clone()),
                RoleLifecycleState::Completed,
            )
            .await
            .map_err(|e| RoleError::Internal(format!("Failed to report status: {e:?}")))?;

        info!("Auditor completed");
        Ok(())
    }
}

impl Default for Auditor {
    fn default() -> Self {
        Self::new()
    }
}
