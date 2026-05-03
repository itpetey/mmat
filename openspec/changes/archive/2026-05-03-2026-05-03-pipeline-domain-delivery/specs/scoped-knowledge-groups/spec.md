## MODIFIED Requirements

### Requirement: Knowledge groups support cross-sub-domain visibility
The system SHALL allow knowledge groups to be declared as `Public` (visible to dependent sub-domains) or `Private` (visible only to the owning sub-domain).

#### Scenario: Public knowledge group is visible to dependents
- **WHEN** sub-domain A creates a knowledge group with `Public` visibility
- **AND** sub-domain B depends on A
- **THEN** B's knowledge sessions for architect, implementation planning, and execution MUST include A's public groups

#### Scenario: Private knowledge group is isolated to owner
- **WHEN** sub-domain A creates a knowledge group with `Private` visibility
- **AND** sub-domain B depends on A
- **THEN** B's knowledge sessions MUST NOT include A's private groups

#### Scenario: Default visibility is private
- **WHEN** a knowledge group is created without an explicit visibility declaration
- **THEN** it MUST default to `Private` visibility

### Requirement: Knowledge exposure is scoped per stage and per sub-domain
The system SHALL expose only the materialised knowledge groups that a given sub-domain plus its dependency chain are allowed to use, in addition to the existing per-stage scoping.

#### Scenario: Sub-domain receives its own plus upstream public groups
- **WHEN** a solution generation stage runs for sub-domain B (which depends on A)
- **THEN** its LLM context MUST include B's own knowledge groups scoped to solution generation, plus A's public groups scoped to solution generation

#### Scenario: No cross-contamination between independent sub-domains
- **WHEN** two sub-domains A and C have no dependency relationship
- **THEN** neither MUST receive the other's knowledge groups

## REMOVED Requirements

None. Existing scoped knowledge behaviour (stage-scoped sessions, SQLite persistence, controlled templates, lint validation) is preserved. This modification adds cross-sub-domain visibility without removing existing functionality.
