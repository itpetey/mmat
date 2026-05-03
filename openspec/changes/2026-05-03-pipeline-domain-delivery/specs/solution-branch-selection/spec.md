## MODIFIED Requirements

### Requirement: Solution branch selection runs per sub-domain
The system SHALL generate solution branches, collect recommendations, and prompt for user selection independently for each leaf `DomainNode`.

#### Scenario: Each leaf node generates its own branches
- **WHEN** knowledge materialisation completes for a leaf `DomainNode`
- **THEN** the system MUST generate conservative, recommended, and ambitious solution branches scoped to that sub-domain only

#### Scenario: Solution selection is independent per sub-domain
- **WHEN** the user selects a solution for sub-domain A
- **THEN** it MUST NOT affect the pending solution selection for sub-domain B

#### Scenario: Solution selection respects domain dependencies
- **WHEN** sub-domain B depends on sub-domain A
- **THEN** B's solution generation MAY incorporate A's selected solution direction as a constraint
- **AND** B's solution generation MUST wait until A has selected a solution

### Requirement: Solution selection runs in parallel for independent sub-domains
The system SHALL allow solution generation and selection for independent sub-domains to run concurrently.

#### Scenario: Independent sub-domain solutions generate in parallel
- **WHEN** sub-domains A and C have no dependency relationship
- **THEN** their solution generation, collection, and selection MAY execute concurrently

## REMOVED Requirements

None. Existing solution branch behaviour (three branches, collection, recommendation, hybrid support, user choice, architect handoff) is preserved. This modification adds per-sub-domain independence without removing existing functionality.
