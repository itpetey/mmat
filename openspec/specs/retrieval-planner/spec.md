# retrieval-planner Specification

## Purpose
TBD - created by archiving change coordinator. Update Purpose after archive.
## Requirements
### Requirement: Retrieval planner provides stage-appropriate memory
The system SHALL provide a `RetrievalPlanner` that, given a `RoleId` and task context, returns a filtered set of memories from the memory store. The filter MUST be different for each role type, based on that role's retrieval profile.

#### Scenario: Worker receives project-scoped memory only
- **WHEN** a Worker requests memory context for a task
- **THEN** the retrieval planner MUST query memories with scope `Project` and type `Constraint`, `Decision`, `Fact`
- **AND** MUST NOT include `Organisational` or `WorldModel` scopes
- **AND** MUST filter for `authority >= ReviewFindings`

#### Scenario: Scholar receives all memory scopes
- **WHEN** a Scholar requests memory context for research
- **THEN** the retrieval planner MUST query memories across all scopes (`Ephemeral`, `Project`, `Organisational`, `WorldModel`)
- **AND** MUST include all authority levels
- **AND** MUST include all memory types

#### Scenario: Architect receives project and organisational memory
- **WHEN** an Architect requests memory context for design work
- **THEN** the retrieval planner MUST query memories with scopes `Project` and `Organisational`
- **AND** MUST filter for types `Decision`, `Constraint`, `Risk`, `Lesson`
- **AND** MUST exclude `Ephemeral` scope

### Requirement: Retrieval profiles are configurable per role type
The system SHALL define a `RetrievalProfile` struct per role type specifying: allowed memory scopes, allowed memory types, minimum authority level, maximum age, and result count limit. The profiles MUST be defined at coordinator startup and MAY be overridden per task.

#### Scenario: Default retrieval profile for Worker
- **WHEN** no task-specific override is provided
- **THEN** the Worker's default `RetrievalProfile` MUST be used
- **AND** it MUST specify `scope = [Project], types = [Constraint, Decision, Fact, SOP], min_authority = ReviewFindings`

#### Scenario: Task-specific retrieval override
- **WHEN** a `TaskAssigned` event includes a `retrieval_override` field
- **THEN** the retrieval planner MUST use the override instead of the default profile for that task

### Requirement: Retrieval planner supports semantic search
The system SHALL support semantic (vector) search via Qdrant as part of retrieval, keyed by a natural-language query. Results MUST be merged with structured query results and deduplicated by `MemoryId`.

#### Scenario: Semantic search finds relevant memories
- **WHEN** a role queries with `semantic_query: "database migration patterns"`
- **THEN** the retrieval planner MUST return memories whose embeddings are semantically similar to the query
- **AND** results MUST be ranked by cosine similarity descending

#### Scenario: Structured and semantic results are merged
- **WHEN** both structured filters and a semantic query are provided
- **THEN** the merged result set MUST contain memories matching EITHER criteria
- **AND** duplicate `MemoryId`s MUST appear only once

