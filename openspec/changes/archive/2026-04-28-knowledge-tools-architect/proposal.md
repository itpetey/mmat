## Why

The architect stage currently receives knowledge groups as augmented text in the system prompt but cannot actively query that knowledge during planning. This limits the architect's ability to discover and apply existing patterns from the codebase when designing solutions. Additionally, the knowledge step plans ingestion without validating the resulting groups for graph or metadata issues before materialisation.

## What Changes

- **Architect gains `knowledge_search` tool**: The architect stage will have a `KnowledgeSearchTool` wired in, scoped to its relevant knowledge groups, allowing the model to query existing repository patterns, code conventions, and prior decisions during architectural design.

- **Knowledge step gains `knowledge_lint` validator**: After knowledge planning succeeds but before materialisation, the system will run `knowledge_lint` against the proposed groups. Findings will feed back into retry logic if the lint report contains issues.

- **StageKnowledgeSession extends to carry tool registry**: The knowledge session passed to stages will optionally carry a `ToolRegistry`, enabling tool-calling stages like architect to access knowledge tools without separate wiring.

- **Repository tools remain out of scope for initial implementation**: While NAAF exposes repository tools (read_file, glob_paths, search_files), this change focuses on knowledge tools only.

## Capabilities

### New Capabilities

- `architect-knowledge-tools`: Enables the architect stage to use `knowledge_search` tool calls to inform architectural decisions from existing repository knowledge. Creates `specs/architect-knowledge-tools/spec.md`.

### Modified Capabilities

- `scoped-knowledge-groups`: Extend the StageKnowledgeSession to optionally carry a ToolRegistry alongside the augmented system prompt and collection names. Affects how knowledge is exposed to stages that support tool calls.

## Impact

- **src/workflow/architect.rs**: Switch from `json_task` to `tool_task` or equivalent that wires in the knowledge tool registry
- **src/workflow/knowledge.rs**: Add `knowledge_lint` call as a pre-materialisation validator
- **src/workflow/mod.rs**: Build and propagate `ToolRegistry` through `StageKnowledgeSession`
- **naaf-knowledge**: No changes required—existing `KnowledgeSearchTool` and `KnowledgeLintTool` provide the needed primitives