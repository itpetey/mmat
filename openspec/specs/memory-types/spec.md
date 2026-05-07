## ADDED Requirements

### Requirement: Memory items are typed and fully attributed
The system SHALL define a `Memory` struct with fields for a unique `MemoryId`, a `MemoryType` discriminant, textual `content`, an optional embedding vector, `MemoryScope`, `Authority`, `Confidence`, `DecayPolicy`, evidence references (list of `EventId`), supersession fields (`supersedes` and `superseded_by`), creation and last-access timestamps, and source agent identifier.

#### Scenario: Memory is constructed with required fields
- **WHEN** a `Memory` is created
- **THEN** it MUST have a unique `MemoryId` (UUID v4)
- **AND** `memory_type`, `content`, `scope`, `authority`, `confidence`, and `decay_policy` MUST be present

#### Scenario: Memory carries evidence lineage
- **WHEN** a memory is derived from a `ClaimMade` event with evidence refs pointing to a `ToolExecuted` event
- **THEN** the memory's `evidence_refs` MUST contain the `EventId` of both events
- **AND** the `source_agent` MUST identify the role that proposed the memory

### Requirement: Eleven memory types are defined
The system SHALL provide a `MemoryType` enum with exactly eleven variants: `Fact`, `Decision`, `Constraint`, `Preference`, `Risk`, `Lesson`, `SOP`, `Incident`, `Assumption`, `OpenQuestion`, and `Relationship`. Each variant MUST serialize to a distinct discriminant string.

#### Scenario: Memory type is serializable and queryable
- **WHEN** a memory of type `Decision` is stored
- **THEN** the store MUST allow querying for all memories of type `Decision` via the discriminant string

### Requirement: Four memory scopes are defined with retrieval rules
The system SHALL provide a `MemoryScope` enum with variants: `Ephemeral` (minutes to days decay), `Project` (months to years), `Organisational` (cross-project, reviewed periodically), and `WorldModel` (relationships, causality). Each scope MUST carry an implicit decay expectation.

#### Scenario: Scope constrains retrieval
- **WHEN** a Worker retrieves memory for a task
- **THEN** the retrieval planner MUST limit results to `Project` and `Ephemeral` scopes by default
- **AND** `Organisational` scoped memories MUST NOT be returned unless explicitly requested

#### Scenario: Ephemeral scope has aggressive decay
- **WHEN** a memory is created with `Ephemeral` scope and `stale_after_days(1)` decay policy
- **THEN** the librarian MUST mark it for decay 1 day after creation

### Requirement: Authority hierarchy is strictly ordered
The system SHALL define an `Authority` enum with a total ordering from highest to lowest: `CompilerOutput`, `UserInstruction`, `RepositoryState`, `AcceptedADR`, `ReviewFindings`, `LLMInference`, `SpeculativeReasoning`. The enum MUST implement `Ord` and `PartialOrd`.

#### Scenario: Higher authority overrides lower on conflict
- **WHEN** the librarian detects two memories making contradictory claims
- **THEN** the memory with higher authority MUST be preserved
- **AND** the lower-authority memory MUST be marked for supersession, not silently deleted

#### Scenario: Compiler output is highest authority
- **WHEN** a `ToolExecuted` event captures a compiler error
- **THEN** any memory derived from it MUST default to `CompilerOutput` authority

### Requirement: Confidence is a bounded float with validation
The system SHALL define `Confidence` as a value between 0.0 and 1.0 inclusive. Construction with a value outside this range MUST be rejected.

#### Scenario: Valid confidence is accepted
- **WHEN** `Confidence::new(0.94)` is called
- **THEN** it MUST return `Ok(Confidence(0.94))`

#### Scenario: Invalid confidence is rejected
- **WHEN** `Confidence::new(1.5)` or `Confidence::new(-0.1)` is called
- **THEN** it MUST return an error

### Requirement: Decay policies control memory lifecycle
The system SHALL provide a `DecayPolicy` enum with variants: `Never` (persists indefinitely), `StaleAfterDays(u32)` (decays N days after creation), and `SupersededOnly` (persists until explicitly superseded). The librarian MUST enforce decay based on policy.

#### Scenario: StaleAfterDays triggers decay
- **WHEN** a memory has `DecayPolicy::StaleAfterDays(30)` and 31 days have passed since creation
- **THEN** the librarian's periodic scan MUST publish a `MemorySuperseded` event for that memory
- **AND** the reason MUST indicate decay, not contradiction

#### Scenario: Never policy persists indefinitely
- **WHEN** a memory has `DecayPolicy::Never`
- **THEN** it MUST NOT be decayed by the periodic scan
- **AND** it MUST only be removed by explicit supersession
