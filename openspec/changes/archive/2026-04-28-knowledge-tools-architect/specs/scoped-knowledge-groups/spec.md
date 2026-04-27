## MODIFIED Requirements

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