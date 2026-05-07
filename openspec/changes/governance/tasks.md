## 1. Roles Crate Extension

- [ ] 1.1 Create `src/auditor.rs` in `crates/roles/`
- [ ] 1.2 Add module declaration to `crates/roles/src/lib.rs`
- [ ] 1.3 Define audit artefact types in `src/artefacts.rs`: `AuditReport`, `EvidenceChainStatus`, `ProcessAdherenceCheck`, `ConfidenceAssessment`

## 2. Auditor — Core

- [ ] 2.1 Implement `Auditor` struct with event store handle, provenance engine handle, memory store handle (read-only), coordinator handle
- [ ] 2.2 Implement `Role` trait: `id()`, `spec()` with `AuthorityScope::Audit`, subscriptions to ALL event types
- [ ] 2.3 Implement `Auditor::run()` actor loop: consume all events, run deterministic checks on relevant ones, publish findings

## 3. Auditor — Evidence Chain Validation

- [ ] 3.1 Implement evidence reference validation: on `ClaimMade`, check each `evidence_refs` EventId exists in event store
- [ ] 3.2 Implement evidence existence check: query event store for each referenced EventId
- [ ] 3.3 Publish `EvidenceChainBroken` event when a reference points to non-existent event, including claim content and broken ref
- [ ] 3.4 Implement evidence consistency check: on `ClaimMade`, compare claim text against cited `ToolExecuted` output (deterministic pattern match for exit codes, LLM-assisted for semantic consistency)

## 4. Auditor — Process Adherence

- [ ] 4.1 Implement process step tracking: maintain expected process sequences for common operations (e.g., for "tests passed" claim, expect prior `cargo test` ToolExecuted)
- [ ] 4.2 Implement temporal ordering check: verify that required tool executions occurred before the claim that depends on them
- [ ] 4.3 Implement exit code verification: when a claim asserts success (e.g., "tests passed", "build succeeded"), verify the cited ToolExecuted has exit_code 0
- [ ] 4.4 Publish `ProcessSkipped` event when a required step is missing
- [ ] 4.5 Implement contradiction detection: when claim contradicts tool output (e.g., claims passed but exit_code != 0)

## 5. Auditor — Hallucination Detection

- [ ] 5.1 Implement file existence check: on `EvidencePack` references to file paths, verify paths exist in repository
- [ ] 5.2 Implement capability verification: on claims about API capabilities, check against actual repository state
- [ ] 5.3 Implement source verification for Scholar citations: web URLs checked via HTTP HEAD (configurable, may be disabled)
- [ ] 5.4 Publish `EvidenceChainBroken` for hallucinated citations with the non-existent reference

## 6. Auditor — Confidence Assessment

- [ ] 6.1 Implement confidence-vs-evidence scoring: high confidence (≥0.8) with no evidence_refs → flag as unjustified
- [ ] 6.2 Implement confidence-vs-evidence mismatch detection: low confidence (≤0.3) with strong evidence → note for human review
- [ ] 6.3 Publish `PolicyViolationDetected` for unjustified confidence

## 7. Auditor — Authority Boundary Enforcement

- [ ] 7.1 Implement authority check: on every event, verify source_agent's authority scope allows publishing this event type
- [ ] 7.2 Maintain authority scope registry (from role specs in coordinator)
- [ ] 7.3 Publish `PolicyViolationDetected` when a role exceeds its authority boundary

## 8. Auditor — Reporting

- [ ] 8.1 Implement `AuditReport` assembly: summary of findings, violation counts by type, confidence assessments
- [ ] 8.2 Implement periodic report publishing: on configurable interval (default: hourly) or on task completion
- [ ] 8.3 Publish `ArtefactProduced` events with audit reports
- [ ] 8.4 Implement memory contamination reporting: when accepted memory derives from a now-flagged claim, publish violation referencing memory ID (but do NOT mutate memory)

## 9. Auditor — Selective LLM Assistance

- [ ] 9.1 Implement LLM-assisted semantic consistency check: for claims that pass deterministic checks but may be semantically inconsistent with evidence
- [ ] 9.2 Configure LLM usage: only invoked for ambiguous cases, with strict prompt instructing sceptical evaluation
- [ ] 9.3 Implement LLM call budget: limit LLM-assisted checks per audit cycle to prevent cost explosion

## 10. Integration

- [ ] 10.1 Write integration test: Worker claims "tests passed" without running tests → Auditor detects ProcessSkipped
- [ ] 10.2 Write integration test: Worker claims "tests passed" but exit code is 1 → Auditor detects contradiction
- [ ] 10.3 Write integration test: Scholar cites non-existent file → Auditor detects EvidenceChainBroken
- [ ] 10.4 Write integration test: Claim with high confidence and no evidence → Auditor flags unjustified confidence
- [ ] 10.5 Write integration test: Worker publishes DecisionRecorded → Auditor flags authority violation
- [ ] 10.6 Write integration test: Auditor detects memory contamination → publishes violation but does not mutate memory → Librarian consumes violation
- [ ] 10.7 Write integration test: Valid claim with proper evidence chain → Auditor does NOT flag anything

## 11. Validation

- [ ] 11.1 `cargo fmt --all` passes
- [ ] 11.2 `cargo clippy -- -D warnings` passes on all crates
- [ ] 11.3 `cargo test` passes all tests including doc tests
