## ADDED Requirements

### Requirement: Executor mediates the model-tool-call-result loop
The system SHALL provide an `Executor` that runs the chat completions loop: sending a request, collecting the model's response, executing any tool calls, feeding tool results back, and repeating until the model returns a final text response or a turn limit is exceeded. The executor MUST accept a `ToolRegistry` and a configurable `ExecutorConfig` with turn limit and token budget.

#### Scenario: Executor completes without tool calls
- **WHEN** the model returns a text response with no tool calls
- **THEN** the executor MUST return the response immediately without further iterations

#### Scenario: Executor handles a single tool call
- **WHEN** the model returns a response with one tool call
- **THEN** the executor MUST invoke the registered tool with the provided arguments
- **AND** feed the tool result back into the conversation as a tool message
- **AND** continue the loop until a final text response is received

#### Scenario: Executor enforces turn limit
- **WHEN** the config specifies `max_turns = 5` and the model makes 5 tool calls
- **THEN** the executor MUST return `ExecutorError::TurnLimitExceeded` after the 5th iteration

#### Scenario: Executor enforces token budget
- **WHEN** the config specifies a `max_tokens` budget and the conversation exceeds it
- **THEN** the executor MUST return `ExecutorError::TokenLimitExceeded`

### Requirement: Tool trait defines callable tools
The system SHALL provide a `Tool` trait with a `spec()` method returning a `ToolSpec` and a `call()` method accepting a runtime reference and JSON arguments. Tool specifications MUST include a name, description, and JSON Schema input schema.

#### Scenario: Tool spec is generated from implementation
- **WHEN** a tool implementing the `Tool` trait is queried for its spec
- **THEN** the returned `ToolSpec` MUST include the tool's name, natural-language description, and JSON Schema describing accepted arguments

#### Scenario: Tool is invoked with valid arguments
- **WHEN** `Tool::call()` is invoked with arguments matching the tool's input schema
- **THEN** the tool MUST execute and return a JSON value result

#### Scenario: Tool is invoked with invalid arguments
- **WHEN** `Tool::call()` is invoked with arguments that do not match the schema
- **THEN** it is the tool implementation's responsibility to return an appropriate error

### Requirement: Tool registry manages the available tool set
The system SHALL provide a `ToolRegistry` that stores tools by name and provides their specifications for inclusion in completion requests. Duplicate tool names MUST be rejected at registration time.

#### Scenario: Tools are registered by name
- **WHEN** a tool is registered with `registry.register(my_tool)`
- **THEN** the tool MUST be retrievable by its spec name
- **AND** the registry's `tool_specs()` method MUST include this tool's spec

#### Scenario: Duplicate tool names are rejected
- **WHEN** two tools with the same spec name are registered
- **THEN** the second registration MUST return an error
