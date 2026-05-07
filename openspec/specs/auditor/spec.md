# auditor Specification

## Purpose
TBD - created by archiving change governance. Update Purpose after archive.
## Requirements
### Requirement: Auditor validates evidence chains
The system SHALL provide an `Auditor` actor that subscribes to all `ClaimMade` events and validates that every evidence reference points to an existing `ToolExecuted` event in the event store. Claims with broken evidence chains MUST be flagged.

#### Scenario: Valid evidence chain passes
- **WHEN** a `ClaimMade` event references a `ToolExecuted` event that exists in the event store
- **THEN** the Auditor MUST NOT flag the claim

#### Scenario: Broken evidence chain is detected
- **WHEN** a `ClaimMade` event references an `EventId` that does not exist in the event store
- **THEN** the Auditor MUST publish `EvidenceChainBroken { claim_id, missing_ref }`
- **AND** the event MUST include the claim content and the broken reference

### Requirement: Auditor detects process adherence violations
The system SHALL verify that required process steps were completed. For example, when a Worker claims "all tests passed", the Auditor MUST check that a `ToolExecuted` event for `cargo test` exists with exit code 0 and was published before the claim.

#### Scenario: Process was followed correctly
- **WHEN** a Worker publishes `ToolExecuted { tool: "cargo_test", exit_code: 0 }` followed by `ClaimMade { claim: "tests passed" }`
- **THEN** the Auditor MUST verify the tool execution precedes the claim
- **AND** MUST NOT flag a violation

#### Scenario: Process step was skipped
- **WHEN** a Worker publishes `ClaimMade { claim: "tests passed" }` without any prior `ToolExecuted` for cargo test
- **THEN** the Auditor MUST publish `ProcessSkipped { step: "cargo_test", claim_id }`

#### Scenario: Process step evidence shows failure
- **WHEN** a Worker runs `cargo test` (exit code 1) but still claims "tests passed"
- **THEN** the Auditor MUST detect the contradiction between the tool output and the claim
- **AND** publish `PolicyViolationDetected` with details of the mismatch

### Requirement: Auditor detects hallucinations
The system SHALL detect when claims reference non-existent sources, files that don't exist in the repository, or capabilities that contradict repository state.

#### Scenario: Scholar cites non-existent file
- **WHEN** a Scholar's `EvidencePack` references a file path that does not exist in the repository
- **THEN** the Auditor MUST flag the citation as hallucinated
- **AND** publish `EvidenceChainBroken` with the non-existent path

#### Scenario: Worker claims a capability that doesn't exist
- **WHEN** a Worker claims "the API supports endpoint X" but that endpoint does not exist in the codebase
- **THEN** the Auditor MUST detect the mismatch via repository inspection
- **AND** publish `PolicyViolationDetected`

### Requirement: Auditor assesses confidence against evidence
The system SHALL compare claimed confidence against actual evidence quality. A claim with high confidence but no evidence MUST be flagged. A claim with low confidence but strong evidence MUST be noted.

#### Scenario: High confidence without evidence is flagged
- **WHEN** a `ClaimMade` event has `confidence: 0.95` but empty `evidence_refs`
- **THEN** the Auditor MUST publish `PolicyViolationDetected { reason: "unjustified confidence" }`

#### Scenario: Low confidence with strong evidence is noted
- **WHEN** a `ClaimMade` event has `confidence: 0.3` but references a successful `ToolExecuted` event
- **THEN** the Auditor MUST publish an `AuditReport` noting the confidence-evidence mismatch

### Requirement: Auditor checks authority boundaries
The system SHALL verify that roles do not exceed their authority scope. A Worker publishing `DecisionRecorded` events, or a Scholar making architectural recommendations, MUST be flagged.

#### Scenario: Worker exceeds authority
- **WHEN** a Worker publishes a `DecisionRecorded` event
- **THEN** the Auditor MUST flag it as an authority violation
- **AND** publish `PolicyViolationDetected { reason: "authority boundary exceeded" }`

#### Scenario: Role stays within authority
- **WHEN** an Architect publishes a `DecisionRecorded` event
- **THEN** the Auditor MUST NOT flag it (Architect has architecture authority)

### Requirement: Auditor publishes structured audit reports
The system SHALL produce `AuditReport` artefacts summarising findings, violation counts by type, confidence assessments, and traceability reports. Reports MUST be published periodically and on significant events.

#### Scenario: Periodic audit report
- **WHEN** the Auditor's periodic report interval triggers (default: after every task completion or hourly)
- **THEN** it MUST publish an `ArtefactProduced` event with `artefact_type: "audit_report"`
- **AND** the report MUST include violation counts grouped by type

### Requirement: Auditor does not mutate memory
The system SHALL ensure the Auditor only publishes findings — it MUST NOT directly modify the memory store, supersede memories, or change authority levels. The Librarian decides what to do with audit findings.

#### Scenario: Auditor flags contaminated memory
- **WHEN** the Auditor detects that an accepted memory was derived from a hallucinated claim
- **THEN** it MUST publish `PolicyViolationDetected` referencing the memory ID
- **AND** it MUST NOT directly supersede the memory
- **AND** the Librarian MUST consume the violation and decide the appropriate action

