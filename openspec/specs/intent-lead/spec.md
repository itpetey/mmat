## ADDED Requirements

### Requirement: Intent Lead interrogates human intent
The system SHALL provide an `IntentLead` actor implementing the `Role` trait. On receiving a `HumanFeedbackReceived` event containing the initial prompt, it MUST interrogate the human for clarification: identify unstated goals, identify non-goals, capture constraints, and track satisfaction criteria. It MUST NOT propose implementation approaches.

#### Scenario: Intent Lead produces an intent brief
- **WHEN** the human provides a prompt like "build a data pipeline"
- **THEN** the Intent Lead MUST ask clarifying questions before producing any output
- **AND** once sufficient context is gathered, it MUST publish an `ArtefactProduced` event with an `IntentBrief` artefact
- **AND** the brief MUST include: clarified goals, explicit non-goals, constraints, success metrics, and priority ordering

#### Scenario: Intent Lead does not propose architecture
- **WHEN** the human describes a problem
- **THEN** the Intent Lead MUST NOT publish events containing implementation suggestions (e.g., "use microservices")
- **AND** it MUST instead capture the underlying need (e.g., "prioritises team autonomy and deploy isolation")

### Requirement: Intent Lead maintains a stakeholder model
The system SHALL track stakeholder preferences, taste, and priorities across interactions. The stakeholder model MUST persist as project-scoped memory items.

#### Scenario: Stakeholder preferences are recalled
- **WHEN** the human has previously expressed a preference (e.g., "prefer simplicity over performance")
- **AND** the Intent Lead processes a new prompt
- **THEN** it MUST retrieve and apply that preference from memory
- **AND** include it in the context for downstream roles

### Requirement: Intent Lead dispatches Scholar and Ops Manager
The system SHALL allow the Intent Lead to publish `TaskAssigned` events targeting Scholar or Ops Manager roles with specific research questions or process requirements.

#### Scenario: Intent Lead dispatches Scholar for codebase research
- **WHEN** the Intent Lead determines that the existing codebase must be studied
- **THEN** it MUST publish a `TaskAssigned` event targeting the Scholar with a research brief
- **AND** the brief MUST include specific questions to answer and scope boundaries

#### Scenario: Intent Lead dispatches Ops Manager for process definition
- **WHEN** the Intent Lead determines that a project type requires specific processes
- **THEN** it MUST publish a `TaskAssigned` event targeting the Ops Manager
- **AND** the task MUST specify the project type and required standards

### Requirement: Intent brief is a structured artefact
The system SHALL define `IntentBrief` as a serializable struct with fields: `goals` (prioritised list), `non_goals` (explicit exclusions), `constraints` (must-satisfy conditions), `success_metrics` (measurable outcomes), `stakeholder_preferences` (taste, priorities), `open_questions` (unresolved ambiguity), and `confidence` (how well the intent is understood).

#### Scenario: IntentBrief is published as an artefact
- **WHEN** the Intent Lead finalises an intent brief
- **THEN** it MUST publish an `ArtefactProduced` event with `artefact_type: "intent_brief"`
- **AND** the artefact payload MUST be the serialised `IntentBrief` struct
- **AND** downstream roles (Architect, PM) MUST be able to retrieve it from the event stream
