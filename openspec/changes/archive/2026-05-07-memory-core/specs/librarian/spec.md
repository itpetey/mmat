## ADDED Requirements

### Requirement: Librarian validates memory proposals against write gates
The system SHALL provide a `Librarian` actor that subscribes to `MemoryProposed` events and applies five write gates before accepting: (1) durability — does this matter beyond the current turn? (2) grounding — is it backed by user instruction, code, tests, or artefacts? (3) scope — is the proposed scope appropriate? (4) invalidatability — can it be superseded later? (5) non-duplication — does it duplicate or contradict existing memory?

#### Scenario: Proposal passes all gates
- **WHEN** a `MemoryProposed` event passes all five write gates
- **THEN** the librarian MUST publish a `MemoryAccepted` event with the final authority, scope, and memory ID
- **AND** the memory MUST be persisted to the memory store

#### Scenario: Proposal fails grounding gate
- **WHEN** a `MemoryProposed` event has empty `evidence_refs` and `source_agent` is not the user
- **THEN** the librarian MUST reject the proposal
- **AND** publish a `MemoryRejected` event with reason "ungrounded"

#### Scenario: Proposal fails duplication gate
- **WHEN** a `MemoryProposed` event contains content semantically similar (cosine > 0.92) to an existing memory
- **THEN** the librarian MUST reject the proposal
- **AND** the reason MUST cite the existing `MemoryId`

### Requirement: Librarian handles contradiction between proposals
The system SHALL detect when a proposed memory contradicts an existing memory of the same type and scope. Contradiction resolution MUST favour the higher-authority memory. If authorities are equal, the more recent memory wins.

#### Scenario: Higher authority proposal overrides lower
- **WHEN** a new memory proposal has `CompilerOutput` authority
- **AND** it contradicts an existing memory with `LLMInference` authority
- **THEN** the librarian MUST accept the new proposal
- **AND** supersede the existing memory with reason "superseded by higher authority"

#### Scenario: Equal authority favours recency
- **WHEN** a new memory proposal contradicts an existing memory
- **AND** both have `LLMInference` authority
- **THEN** the librarian MUST accept the new proposal
- **AND** supersede the existing memory with reason "superseded by more recent evidence"

#### Scenario: Lower authority proposal is rejected
- **WHEN** a new memory proposal has `SpeculativeReasoning` authority
- **AND** it contradicts an existing memory with `UserInstruction` authority
- **THEN** the librarian MUST reject the proposal
- **AND** NOT supersede the existing higher-authority memory

### Requirement: Librarian enforces decay policies via periodic scan
The system SHALL run a periodic scan (default: hourly) checking for memories whose decay policy has triggered. Decayed memories MUST be marked as superseded with reason "decayed", and a `MemorySuperseded` event MUST be published.

#### Scenario: Stale memory is decayed
- **WHEN** a memory has `DecayPolicy::StaleAfterDays(30)` and 31 days have passed
- **THEN** the periodic scan MUST detect it
- **AND** publish a `MemorySuperseded` event linking the old memory to the decay event

#### Scenario: Non-decayed memory is untouched
- **WHEN** a memory has `DecayPolicy::Never` or has not reached its decay date
- **THEN** the periodic scan MUST NOT supersede it

### Requirement: Librarian handles explicit supersession requests
The system SHALL accept explicit `MemorySuperseded` events from other actors (e.g., Architect replacing an ADR) and propagate the supersession through the memory store's chain.

#### Scenario: Explicit supersession updates the chain
- **WHEN** the librarian receives a `MemorySuperseded` event with `superseded_id` and `superseding_id`
- **THEN** it MUST update the store to link the two memories via `superseded_by` and `supersedes`
- **AND** future retrievals MUST return the superseding memory, not the superseded one

### Requirement: Librarian maintains ontology consistency
The system SHALL detect when memory types are used inappropriately (e.g., a `Fact` being superseded without evidence, or an `SOP` being created with `Ephemeral` scope). Violations MUST be published as `PolicyViolationDetected` events.

#### Scenario: SOP with Ephemeral scope is flagged
- **WHEN** a `MemoryProposed` event proposes an `SOP` with `Ephemeral` scope
- **THEN** the librarian MUST reject the proposal
- **AND** publish a `PolicyViolationDetected` event
- **AND** the reason MUST explain that SOPs require durable scope

#### Scenario: Fact without evidence is flagged
- **WHEN** a `MemoryProposed` event proposes a `Fact` with empty `evidence_refs` and `LLMInference` authority
- **THEN** the librarian MUST lower the accepted authority to `SpeculativeReasoning`
- **AND** publish a `PolicyViolationDetected` event noting the downgrade
