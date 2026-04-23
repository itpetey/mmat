## ADDED Requirements

### Requirement: Discovery gathers architect-ready intent through live recursion
The system SHALL run discovery as a live human-in-the-loop workflow stage that can recursively gather clarifications until the context is ready for downstream solution generation.

#### Scenario: Discovery asks a live clarification question
- **WHEN** discovery identifies a blocking ambiguity that cannot be resolved from the repository or currently materialised knowledge
- **THEN** it MUST ask the user a live follow-up question before continuing the discovery flow

#### Scenario: Discovery continues after an answer
- **WHEN** the user answers a live discovery question
- **THEN** the next discovery turn MUST incorporate that answer into the gathered context instead of starting from a blank prompt

### Requirement: Discovery produces a structured handoff for solution generation
The system SHALL produce a structured discovery result that records the current problem statement, goals, constraints, assumptions, risks, and readiness for solution generation.

#### Scenario: Discovery reaches solution-ready state
- **WHEN** discovery determines that the gathered context is specific enough for solution generation
- **THEN** it MUST emit a structured solution-ready handoff rather than asking another clarification question

#### Scenario: Discovery preserves explicit uncertainty
- **WHEN** discovery proceeds despite unresolved but non-blocking ambiguity
- **THEN** the structured handoff MUST include those uncertainties as explicit assumptions, defaults, or risks

### Requirement: Discovery workflow code is stage-owned
The discovery implementation SHALL be organised as a subject-owned workflow module rather than being split across generic prompt, model, step, and task modules.

#### Scenario: Discovery code is traced end-to-end
- **WHEN** a developer inspects the discovery workflow implementation
- **THEN** the core discovery types, prompt construction, and step orchestration MUST be located under the discovery workflow module
