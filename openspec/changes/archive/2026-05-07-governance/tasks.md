## 1. Roles Crate Extension

- [x] 1.1 Create `src/auditor.rs` in `crates/roles/`
- [x] 1.2 Add module declaration to `crates/roles/src/lib.rs`
- [x] 1.3 Define audit artefact types in `src/artefacts.rs`: `AuditReport`, `EvidenceChainStatus`, `ProcessAdherenceCheck`, `ConfidenceAssessment`

## 2. Auditor — Core

- [x] 2.1 Implement `Auditor` struct with event store handle, provenance engine handle, memory store handle (read-only), coordinator handle
- [x] 2.2 Implement `Role` trait: `id()`, `spec()` with `AuthorityScope::Audit`, subscriptions to ALL event types
- [x] 2.3 Implement `Auditor::run()` actor loop: consume all events, run deterministic checks on relevant ones, publish findings

## 3. Auditor — Evidence Chain Validation

- [x] 3.1 Implement evidence reference validation: on `ClaimMade`, check each `evidence_refs` EventId exists in event store
- [x] 3.2 Implement evidence existence check: query event store for each referenced EventId
- [x] 3.3 Publish `EvidenceChainBroken` event when a reference points to non-existent event, including claim content and broken ref
- [x] 3.4 Implement evidence consistency check: on `ClaimMade`, compare claim text against cited `ToolExecuted` output

## 4. Auditor — Process Adherence

- [x] 4.1 Implement process step tracking: maintain expected process sequences for common operations (e.g., for "tests passed" claim, expect prior `cargo test` ToolExecuted)
- [x] 4.2 Implement temporal ordering check: verify that required tool executions occurred before the claim that depends on them
- [x] 4.3 Implement exit code verification: when a claim asserts success (e.g., "tests passed", "build succeeded"), verify the cited ToolExecuted has exit_code 0
- [x] 4.4 Publish `ProcessSkipped` event when a required step is missing
- [x] 4.5 Implement contradiction detection: when claim contradicts tool output (e.g., claims passed but exit_code != 0)

## 5. Auditor — Hallucination Detection

- [x] 5.1 Implement file existence check: on `EvidencePack` references to file paths, verify paths exist in repository
- [x] 5.2 Implement capability verification: on claims about API capabilities, check against actual repository state
- [x] 5.3 Implement source verification for Scholar citations: web URLs checked via HTTP HEAD (configurable, may be disabled)
- [x] 5.4 Publish `EvidenceChainBroken` for hallucinated citations with the non-existent reference

## 6. Auditor — Confidence Assessment

- [x] 6.1 Implement confidence-vs-evidence scoring: high confidence (≥0.8) with no evidence_refs → flag as unjustified
- [x] 6.2 Implement confidence-vs-evidence mismatch detection: low confidence (≤0.3) with strong evidence → note for human review
- [x] 6.3 Publish `PolicyViolationDetected` for unjustified confidence

## 7. Auditor — Authority Boundary Enforcement

- [x] 7.1 Implement authority check: on every event, verify source_agent's authority scope allows publishing this event type
- [x] 7.2 Maintain authority scope registry (from role specs in coordinator)
- [x] 7.3 Publish `PolicyViolationDetected` when a role exceeds its authority boundary

## 8. Auditor — Reporting

- [x] 8.1 Implement `AuditReport` assembly: summary of findings, violation counts by type, confidence assessments
- [x] 8.2 Implement periodic report publishing: on configurable interval (default: hourly) or on task completion
- [x] 8.3 Publish `ArtefactProduced` events with audit reports
- [x] 8.4 Implement memory contamination reporting: when accepted memory derives from a now-flagged claim, publish violation referencing memory ID (but do NOT mutate memory)

## 9. Auditor — Selective LLM Assistance

- [x] 9.1 Implement LLM-assisted semantic consistency check: for claims that pass deterministic checks but may be semantically inconsistent with evidence
- [x] 9.2 Configure LLM usage: only invoked for ambiguous cases, with strict prompt instructing sceptical evaluation
- [x] 9.3 Implement LLM call budget: limit LLM-assisted checks per audit cycle to prevent cost explosion

## 10. Integration

- [x] 10.1 Write integration test: Worker claims "tests passed" without running tests → Auditor detects ProcessSkipped
- [x] 10.2 Write integration test: Worker claims "tests passed" but exit code is 1 → Auditor detects contradiction
- [x] 10.3 Write integration test: Scholar cites non-existent file → Auditor detects EvidenceChainBroken
- [x] 10.4 Write integration test: Claim with high confidence and no evidence → Auditor flags unjustified confidence
- [x] 10.5 Write integration test: Worker publishes DecisionRecorded → Auditor flags authority violation
- [x] 10.6 Write integration test: Auditor detects memory contamination → publishes violation but does not mutate memory → Librarian consumes violation
- [x] 10.7 Write integration test: Valid claim with proper evidence chain → Auditor does NOT flag anything

## 11. Validation

- [x] 11.1 `cargo fmt --all` passes
- [x] 11.2 `cargo clippy -- -D warnings` passes on all crates
- [x] 11.3 `cargo test` passes all tests including doc tests
