## ADDED Requirements

### Requirement: Discovery produces a recursive domain tree
The system SHALL produce a `DomainTree` from discovery rather than a single solution-ready context. Discovery MUST recursively sub-divide the domain into sub-domains, producing internal nodes that decompose further and leaf nodes that proceed to solution generation.

#### Scenario: Root discovery identifies top-level sub-domains
- **WHEN** discovery starts with a broad user prompt
- **THEN** it MUST produce a root `DomainNode` whose discovery context maps the overall domain boundary
- **AND** the root node MAY identify child sub-domains that require further decomposition

#### Scenario: Sub-domain discovery decomposes recursively
- **WHEN** a non-leaf `DomainNode` has children
- **THEN** each child MUST run its own discovery session to further decompose its sub-domain
- **AND** discovery MUST continue recursively until all branches reach leaf nodes

#### Scenario: Leaf nodes proceed to planning
- **WHEN** a `DomainNode` is marked as a leaf (concrete enough for implementation)
- **THEN** it MUST proceed to knowledge planning, solution generation, and architect stages
- **AND** it MUST produce a `DesignHandoff` scoped to its sub-domain

### Requirement: Domain tree enforces configurable maximum depth
The system SHALL enforce a configurable maximum tree depth to prevent infinite decomposition.

#### Scenario: Discovery halts at configured max depth
- **WHEN** a `DomainNode` reaches the configured maximum depth (default: 3, configurable via `DomainTreeConfig::max_depth`)
- **THEN** it MUST be treated as a leaf regardless of whether further decomposition is possible

#### Scenario: Depth is configurable per project
- **WHEN** a project configures `DomainTreeConfig::max_depth` to a custom value
- **THEN** the domain tree MUST respect that value rather than the default

### Requirement: Domain nodes declare dependencies
The system SHALL allow `DomainNode`s to declare dependencies on other nodes, forming a partial order that determines planning and delivery order.

#### Scenario: Node depends on upstream node's interface
- **WHEN** node B declares a dependency on node A
- **THEN** node B's planning pipeline MUST have access to node A's declared interfaces
- **AND** node B's delivery MUST wait until node A's delivery completes

#### Scenario: Independent nodes can plan and deliver in parallel
- **WHEN** two leaf nodes have no dependency relationship
- **THEN** their planning and delivery pipelines MAY execute concurrently
