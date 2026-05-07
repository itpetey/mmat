## ADDED Requirements

### Requirement: Scholar gathers evidence about the problem domain
The system SHALL provide a `Scholar` actor implementing the `Role` trait. On receiving a `TaskAssigned` event with a research brief, it MUST gather evidence using available tools (file reading, web search, knowledge base query). It MUST publish `ResearchBrief` and `EvidencePack` artefacts. It MUST NOT make architectural decisions.

#### Scenario: Scholar studies repository state
- **WHEN** the Scholar receives a research brief asking "what is the current architecture?"
- **THEN** it MUST use file reading tools to explore the repository
- **AND** publish a `ResearchBrief` artefact summarising findings
- **AND** include `EvidencePack` referencing specific files and patterns observed

#### Scenario: Scholar researches prior art
- **WHEN** the Scholar receives a research brief asking about external techniques
- **THEN** it MUST use web search tools to find relevant prior art
- **AND** publish findings with citations and confidence ratings

#### Scenario: Scholar does not make architectural decisions
- **WHEN** the Scholar discovers a pattern in the codebase
- **THEN** it MUST report the pattern as a finding, not as a recommendation
- **AND** it MUST NOT publish `DecisionRecorded` events (those belong to the Architect)

### Requirement: Scholar identifies and tracks uncertainty
The system SHALL ensure the Scholar explicitly identifies what it does NOT know. Uncertainties MUST be published as `OpenQuestions` artefacts with a confidence assessment and suggested research direction.

#### Scenario: Scholar surfaces open questions
- **WHEN** the Scholar encounters ambiguity (e.g., "the docs don't mention the database version")
- **THEN** it MUST publish an `OpenQuestions` artefact
- **AND** each question MUST include: the question, why it matters, suggested approach to resolve it, and current confidence

#### Scenario: Scholar refines research based on human feedback
- **WHEN** the human provides feedback on the Scholar's findings via `HumanFeedbackReceived`
- **THEN** the Scholar MUST adjust its research direction
- **AND** publish updated findings reflecting the human's guidance

### Requirement: Scholar produces structured evidence packs
The system SHALL define `EvidencePack` as a collections of findings, each with: the claim, source reference (file path, URL, event ID), extracted content, confidence rating, and relevance to the research question.

#### Scenario: Evidence pack links to source material
- **WHEN** the Scholar publishes an `EvidencePack`
- **THEN** each finding MUST reference its source (file path for repo study, URL for web research, EventId for prior events)
- **AND** the Architect MUST be able to trace any architectural decision back to Scholar evidence

### Requirement: Scholar respects research budget
The system SHALL enforce a configurable research budget on the Scholar: maximum LLM calls, maximum web searches, and maximum tool invocations per research task. When the budget is exhausted, the Scholar MUST publish partial findings and escalate.

#### Scenario: Budget is tracked across tool calls
- **WHEN** the Scholar's tool invocations reach the configured budget limit
- **THEN** it MUST stop gathering new evidence
- **AND** publish `ResearchBrief` with what it found so far
- **AND** include a note about what was not investigated due to budget constraints

#### Scenario: Budget can be extended by escalation
- **WHEN** the Scholar exhausts its budget and the question is high-priority
- **THEN** it MUST escalate to the Intent Lead with reason "research budget exhausted"
- **AND** the Intent Lead MAY grant additional budget via a new `TaskAssigned` event
