## ADDED Requirements

### Requirement: Architect produces Architecture Decision Records
The system SHALL provide an `Architect` actor that, upon receiving a `TaskAssigned` event with an `IntentBrief` and relevant `ResearchBrief`s, produces Architecture Decision Records (ADRs). Each ADR MUST document: the decision, context, considered alternatives, tradeoffs, consequences, and references to supporting evidence.

#### Scenario: Architect produces an ADR
- **WHEN** the Architect receives a task to design the data storage layer
- **THEN** it MUST publish a `DecisionRecorded` event with `MemoryType::Decision`
- **AND** the decision MUST reference the Intent Brief (for goals/constraints) and Scholar's Research Brief (for repository constraints)
- **AND** the ADR MUST include at least two considered alternatives with tradeoff analysis

#### Scenario: Architect produces dependency rules
- **WHEN** the Architect defines module boundaries
- **THEN** it MUST publish `DependencyRules` as an artefact
- **AND** the rules MUST specify allowed and forbidden dependency directions between modules

### Requirement: Architect validates against constraints
The system SHALL ensure every architectural decision is validated against the Intent Brief's constraints and the Ops Manager's architectural policies before publication.

#### Scenario: ADR violates a constraint
- **WHEN** the Architect proposes a design that contradicts an Intent Brief constraint
- **THEN** the Architect MUST detect the contradiction and either revise the design or escalate to the Intent Lead

### Requirement: Architect defines contracts and interfaces
The system SHALL produce `InterfaceSpec`s for every system boundary: public APIs, module interfaces, and data schemas. Each spec MUST include: the interface name, input/output types, error modes, and backwards compatibility promises.

#### Scenario: Interface spec is published
- **WHEN** the Architect defines a module boundary
- **THEN** it MUST publish an `ArtefactProduced` event with `artefact_type: "interface_spec"`
- **AND** the spec MUST include input/output type definitions
