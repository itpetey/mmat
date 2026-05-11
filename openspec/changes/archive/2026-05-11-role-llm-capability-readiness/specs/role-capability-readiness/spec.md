## ADDED Requirements

### Requirement: Workbench exposes role capability status
The workbench SHALL show whether each role has required LLM clients, tools, storage access, and fallback mode available.

#### Scenario: Role has configured provider
- **WHEN** a role has all required providers and tools configured
- **THEN** the UI MUST show the role as capability-ready

#### Scenario: Role uses fallback behaviour
- **WHEN** a role will use deterministic or degraded fallback behaviour
- **THEN** the UI MUST label the role as fallback/degraded
- **AND** the role output MUST be traceable to that mode

### Requirement: Role dispatch uses rich contracts
The workbench SHALL create role-specific task contracts that capture intent, expected outputs, constraints, evidence requirements, and acceptance criteria.

#### Scenario: Scholar contract is created
- **WHEN** the user asks Scholar to investigate a topic
- **THEN** the assigned task contract MUST request evidence, open questions, confidence, and source references

#### Scenario: Worker contract is created
- **WHEN** the user asks Worker to implement a change
- **THEN** the assigned task contract MUST include repository target, scope boundaries, validation commands, and expected patch/output rules

### Requirement: Worker tasks require visible safety context
The workbench SHALL show target repository/worktree and validation expectations before Worker implementation tasks are started from the UI.

#### Scenario: Worker task is dispatched
- **WHEN** a Worker task is created from chat
- **THEN** the UI MUST show where code will be written
- **AND** which validation commands will be run when known

### Requirement: Capability failures are actionable
The workbench SHALL convert missing provider/tool configuration into actionable UI messages.

#### Scenario: LLM provider missing
- **WHEN** a role requiring an LLM is invoked without a configured provider
- **THEN** the UI MUST show a clear message identifying the missing configuration
- **AND** it MUST NOT present fallback output as fully capable output
