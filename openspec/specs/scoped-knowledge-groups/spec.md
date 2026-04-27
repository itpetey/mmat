## ADDED Requirements

### Requirement: Knowledge planning is separate from knowledge materialisation
The system SHALL produce a knowledge plan before mutating knowledge-group state, and SHALL materialise knowledge groups through a separate deterministic stage.

#### Scenario: Discovery identifies useful downstream knowledge
- **WHEN** the workflow determines that downstream stages would benefit from additional learning context
- **THEN** it MUST produce a knowledge plan describing zero or more candidate knowledge groups and their intended consumers

#### Scenario: Materialisation applies the knowledge plan
- **WHEN** a knowledge plan is accepted for materialisation
- **THEN** the system MUST validate the proposed groups and materialise only the groups described by that plan

### Requirement: Knowledge groups are persisted in SQLite
The system SHALL persist knowledge-group metadata in SQLite rather than filesystem-backed group storage.

#### Scenario: Materialised knowledge group metadata is saved
- **WHEN** a knowledge group is created or updated during materialisation
- **THEN** its metadata MUST be persisted through a SQLite-backed knowledge-group store

### Requirement: Knowledge groups use controlled templates and run-specific instances
The system SHALL derive knowledge groups from a controlled template vocabulary and create concrete group instances only when the current run needs them.

#### Scenario: Planner proposes a repository code group
- **WHEN** the planner identifies repository code as relevant evidence
- **THEN** it MUST select a controlled group template and produce a concrete instance for the current run rather than inventing an unconstrained group shape

#### Scenario: No unnecessary groups are created
- **WHEN** a run does not need a given kind of knowledge
- **THEN** the system MUST allow the knowledge plan to materialise zero instances of that group type

### Requirement: Knowledge exposure is scoped per stage
The system SHALL expose only the materialised knowledge groups that a given workflow stage is allowed to use, and SHALL optionally provide tool-based access to those groups.

#### Scenario: Solution generation receives only selected groups
- **WHEN** the solution branch generation stage is built
- **THEN** its LLM context MUST include only the knowledge groups selected for solution generation rather than every materialised group in the run

#### Scenario: Architect stage can receive a different scope
- **WHEN** the Software Architect stage runs after solution selection
- **THEN** it MUST be able to receive a different set of materialised knowledge groups from the solution generation stage

#### Scenario: Tool-capable stages receive knowledge as tool registry
- **WHEN** a workflow stage supports tool calls
- **THEN** its `StageKnowledgeSession` MAY include a `ToolRegistry` scoped to the stage's knowledge groups
- **AND** the registry MUST be buildable from the same materials used to construct the stage's system prompt augmentation

### Requirement: Platform knowledge gaps are tracked as upstream work
The system SHALL record missing platform-level knowledge capabilities as upstream NAAF work rather than silently embedding MMAT-only replacements as permanent behaviour.

#### Scenario: Duplicate detection is unavailable in NAAF
- **WHEN** MMAT depends on knowledge duplicate detection that NAAF does not yet provide
- **THEN** the change documentation MUST identify that gap as upstream NAAF work instead of treating a local workaround as the final platform design
