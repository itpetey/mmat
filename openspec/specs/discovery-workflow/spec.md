## MODIFIED Requirements

### Requirement: Discovery produces a structured handoff for solution generation
The system SHALL produce a structured discovery result that records the current problem statement, goals, constraints, assumptions, risks, and readiness for solution generation.

#### Scenario: Discovery reaches solution-ready state
- **WHEN** discovery determines that the gathered context is specific enough for solution generation
- **THEN** it MUST emit a structured solution-ready handoff rather than asking another clarification question

#### Scenario: Discovery preserves explicit uncertainty
- **WHEN** discovery proceeds despite unresolved but non-blocking ambiguity
- **THEN** the structured handoff MUST include those uncertainties as explicit assumptions, defaults, or risks

#### Scenario: Discovery produces a domain node
- **WHEN** discovery runs as part of domain-mapped planning
- **THEN** it MUST produce a `DomainNode` that includes either sub-domain children (for further decomposition) or a solution-ready handoff (for leaf nodes)

### Requirement: Discovery supports parallel tab-based sessions
The system SHALL allow multiple discovery sessions to run concurrently, each in its own UI context.

#### Scenario: Independent sub-domain discoveries run in parallel
- **WHEN** the domain tree contains multiple sub-domains with no parent-child relationship
- **THEN** their discovery sessions MUST be able to run concurrently
- **AND** the UI MUST present each session in a separate tab

#### Scenario: User switches between discovery tabs
- **WHEN** multiple discovery sessions are active
- **THEN** the user MUST be able to switch between tabs to answer questions for different sub-domains
- **AND** each tab MUST preserve its own conversation state independently

## REMOVED Requirements

None. The existing discovery behaviour (live recursion, structured handoff, stage-owned code) is preserved. This modification adds domain-tree awareness and parallel session support without removing existing functionality.
