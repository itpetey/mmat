## ADDED Requirements

### Requirement: Delivery emits backflow events with severity classification
The system SHALL emit `BackflowEvent`s when delivery encounters problems that cannot be resolved through remediation. Each event MUST carry a severity level that determines which planning stage receives it.

#### Scenario: Moderate severity routes back to architect
- **WHEN** a delivery job finds a problem that requires architectural replanning but not a change in solution approach
- **THEN** it MUST emit a `BackflowEvent` with `Moderate` severity
- **AND** the sub-domain's pipeline MUST re-enter the architect phase

#### Scenario: Major severity routes back to solution selection
- **WHEN** a delivery job finds a problem that requires reconsidering the solution approach
- **THEN** it MUST emit a `BackflowEvent` with `Major` severity
- **AND** the sub-domain's pipeline MUST re-enter the solution selection phase

#### Scenario: Critical severity routes back to domain mapping
- **WHEN** a delivery job finds a problem indicating the sub-domain boundary or interface was incorrectly defined
- **THEN** it MUST emit a `BackflowEvent` with `Critical` severity
- **AND** the sub-domain's pipeline MUST re-enter the domain mapping phase
- **AND** dependent sub-domains MUST be marked for cascading replanning

### Requirement: Backflow events cascade to dependent sub-domains
The system SHALL cascade critical backflow events to sub-domains that depend on the failing sub-domain.

#### Scenario: Critical backflow cascades
- **WHEN** sub-domain A emits a `Critical` backflow event
- **AND** sub-domain B depends on A
- **THEN** B MUST be marked for replanning starting from its architect phase
- **AND** if B has already been delivered, its delivery MUST be invalidated

### Requirement: Backflow depth is capped and configurable
The system SHALL enforce a configurable maximum backflow cascade depth per project to prevent infinite replanning loops.

#### Scenario: Backflow exceeds maximum cascade depth
- **WHEN** a sub-domain has been replanned more than the configured maximum cascade depth (default: 3, configurable via `DomainTreeConfig::max_cascade_depth`)
- **THEN** the pipeline MUST halt and surface the issue for human review rather than replanning again

#### Scenario: Cascade depth is configurable per project
- **WHEN** a project configures `DomainTreeConfig::max_cascade_depth` to a custom value
- **THEN** the backflow system MUST respect that value rather than the default

### Requirement: Backflow uses Pipeline Route::Switch for routing
The system SHALL implement backflow routing via `Pipeline::Route::Switch` rather than a separate backflow mechanism.

#### Scenario: Delivery phase routes back via Switch
- **WHEN** a delivery phase completes and produces a `BackflowEvent`
- **THEN** the Pipeline's `Route::Switch` MUST examine the event's severity and return the appropriate phase ID (architect, solution selection, or domain mapping)
- **AND** the Pipeline executor MUST follow that route, re-entering the target phase

### Requirement: Orphaned knowledge groups are deleted on replanning
The system SHALL delete knowledge groups belonging to a sub-domain when that sub-domain is replanned due to backflow.

#### Scenario: Knowledge groups are cleaned up before replanning
- **WHEN** a sub-domain replanning phase begins (triggered by backflow)
- **THEN** all existing knowledge groups scoped to that sub-domain MUST be deleted from SQLite and Qdrant before new groups are materialised

#### Scenario: Dependent sub-domain knowledge is preserved
- **WHEN** a sub-domain A is replanned and sub-domain B depends on A
- **THEN** ONLY A's knowledge groups MUST be deleted; B's groups MUST remain intact until and unless B itself is replanned
