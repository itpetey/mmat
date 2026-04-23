## ADDED Requirements

### Requirement: MMAT generates three concurrent solution branches
The system SHALL generate conservative, recommended, and ambitious solution branches concurrently from the gathered discovery context and selected knowledge groups.

#### Scenario: Branch generation starts from solution-ready context
- **WHEN** discovery and knowledge materialisation have produced the inputs required for solution generation
- **THEN** the system MUST start conservative, recommended, and ambitious branch generation as separate concurrent solution branches

#### Scenario: Each branch stays distinct
- **WHEN** the three branches are generated from the same input context
- **THEN** each branch MUST preserve its own positioning and rationale rather than collapsing into one generic answer

### Requirement: MMAT collects branches into a recommendation step
The system SHALL collect the generated branches into a stage that presents each option, recommends one branch or a hybrid, and records the recommendation rationale.

#### Scenario: Collector recommends a hybrid
- **WHEN** the strongest result combines ideas from more than one branch
- **THEN** the collector MUST be able to recommend a hybrid and explain which branch ideas were adopted or deferred

### Requirement: The user chooses the solution direction
The system SHALL present the collected branch options and ask the user to choose a branch, choose a recommended hybrid, or request revisions before downstream architecture work continues.

#### Scenario: User selects a recommended branch
- **WHEN** the collector presents the branch set and recommendation
- **THEN** the system MUST ask the user for an explicit selection before proceeding to the Software Architect stage

#### Scenario: User requests revisions
- **WHEN** the user rejects the presented branches or asks for revisions
- **THEN** the workflow MUST return to an earlier stage with that guidance instead of proceeding as if a solution had been selected

### Requirement: Selected solutions are handed to the Software Architect stage
The system SHALL hand the chosen solution direction, together with the relevant scoped knowledge, into a downstream Software Architect stage.

#### Scenario: Architect input is assembled after user choice
- **WHEN** the user has chosen a branch or hybrid
- **THEN** the system MUST build an architect-stage handoff from the selected solution and the knowledge groups scoped for architectural work
