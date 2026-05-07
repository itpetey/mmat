## ADDED Requirements

### Requirement: Reviewer validates implementation against rubrics
The system SHALL provide a `Reviewer` actor that, on receiving a `ReviewRequested` event, validates the Worker's implementation against the Ops Manager's review rubrics. The Reviewer MUST publish `ReviewCompleted` with findings and an acceptance decision.

#### Scenario: Reviewer accepts compliant implementation
- **WHEN** the Reviewer checks a Worker's implementation and all rubric dimensions pass
- **THEN** it MUST publish `ReviewCompleted { accepted: true, findings: [] }`

#### Scenario: Reviewer rejects non-compliant implementation
- **WHEN** the Reviewer finds violations (e.g., missing error handling, broken API contract)
- **THEN** it MUST publish `ReviewCompleted { accepted: false, findings: [...] }`
- **AND** each finding MUST include the violated rubric dimension and specific location in code

### Requirement: Reviewer validates architectural compliance
The system SHALL check that the implementation conforms to the Architect's ADRs and interface specifications. Architectural violations MUST be flagged separately from code quality issues.

#### Scenario: Architectural violation detected
- **WHEN** a Worker's implementation introduces a dependency that violates the Architect's dependency rules
- **THEN** the Reviewer MUST flag it as `ArchitecturalConflict`
- **AND** publish `EscalationRequested` targeting the Architect

### Requirement: Reviewer classifies failures and determines escalation
The system SHALL classify each finding into exactly one failure class: `ImplementationDefect`, `ArchitecturalConflict`, `MissingKnowledge`, `AmbiguousIntent`, or `BrokenProcess`. The escalation target MUST be determined by the failure class.

#### Scenario: Implementation defect escalates to Worker retry
- **WHEN** the Reviewer classifies a finding as `ImplementationDefect`
- **THEN** it MUST include a rework instruction in the review findings
- **AND** the coordinator MUST republish `TaskAssigned` to the Worker with the rework context

#### Scenario: Architectural conflict escalates to Architect
- **WHEN** the Reviewer classifies a finding as `ArchitecturalConflict`
- **THEN** it MUST publish `EscalationRequested { severity: Moderate, target: Architect }`
- **AND** the coordinator MUST route the escalation to the Architect

#### Scenario: Missing knowledge escalates to Scholar
- **WHEN** the Reviewer determines that the Worker lacked necessary domain knowledge
- **THEN** it MUST publish `EscalationRequested { severity: Moderate, target: Scholar }`
- **AND** the task MUST be suspended until the Scholar provides the missing knowledge

#### Scenario: Ambiguous intent escalates to Intent Lead
- **WHEN** the Reviewer determines that the task card was ambiguous
- **THEN** it MUST publish `EscalationRequested { severity: Major, target: IntentLead }`
- **AND** the Intent Lead MUST clarify the intent before the task can proceed

### Requirement: Reviewer rework loop respects retry limits
The system SHALL track the number of rework cycles per task. When retries exceed the contract's `max_retries`, the Reviewer MUST escalate rather than request another rework.

#### Scenario: Retry limit reached
- **WHEN** a task has been reworked `max_retries` times and still fails review
- **THEN** the Reviewer MUST NOT request another rework
- **AND** MUST escalate with `EscalationRequested { severity: Critical }`
