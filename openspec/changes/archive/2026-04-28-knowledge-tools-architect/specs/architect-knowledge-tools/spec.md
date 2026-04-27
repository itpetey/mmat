## ADDED Requirements

### Requirement: Architect stage can search knowledge groups during planning
The system SHALL provide the architect stage with an active knowledge search capability, enabling the model to query knowledge groups while producing architectural decisions.

#### Scenario: Architect queries existing patterns
- **WHEN** the architect stage is processing a solution
- **THEN** it MUST have access to a `knowledge_search` tool scoped to its relevant knowledge groups
- **AND** the tool MUST allow querying for patterns, conventions, and prior decisions from the repository

#### Scenario: Architect uses tool-enabled step
- **WHEN** the architect stage is built with knowledge tools
- **THEN** tools MUST be injectable at step construction time (not stored in serialisable structs)
- **AND** the architect step builder MUST support tool-calling to enable knowledge search during planning

### Requirement: Knowledge lint validates groups before materialisation
The system SHALL lint knowledge groups for graph and metadata issues before materialising them.

#### Scenario: Pre-materialisation lint catches structural issues
- **WHEN** a knowledge plan passes validation but before materialisation
- **THEN** the system MUST run `knowledge_lint` against the proposed groups
- **AND** if lint reports issues, the findings MUST be fed back to the planning stage for retry

#### Scenario: Lint does not block planning itself
- **WHEN** the model is producing a knowledge plan
- **THEN** lint validation MUST NOT occur during model inference
- **AND** lint validation MUST occur only after planning succeeds, as a separate validation step