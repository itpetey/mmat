## Context

The MMAT workflow separates knowledge into two phases: planning (what to ingest) and materialisation (actually ingesting). Currently:

- **Architect stage** receives `StageKnowledgeSession` containing an augmented system prompt with knowledge group descriptions, but cannot actively query that knowledge during planning.
- **Knowledge step** produces an ingestion plan via `json_task`, then materialises via a deterministic step, but has no pre-materialisation validation.

NAAF provides `KnowledgeSearchTool` and `KnowledgeLintTool` as reusable primitives, but MMAT does not wire them into its workflow stages.

## Goals / Non-Goals

**Goals:**
- Enable architect stage to actively search knowledge groups during architectural planning
- Add lint validation before knowledge materialisation
- Maintain backward compatibility for stages that only use text augmentation

**Non-Goals:**
- Adding repository tools (read_file, glob_paths) to architect - this change focuses on knowledge tools only
- Changing how knowledge groups are scoped to stages
- Modifying the knowledge planning prompt or output format

## Decisions

### Decision 1: Inject knowledge tools at architect step construction time

**Choice**: Rather than extending `StageKnowledgeSession` with a generic `ToolRegistry` field, inject knowledge tools inside the architect step builder function.

**Rationale**: `StageKnowledgeSession` carries knowledge context (system prompt, group collections) for building prompts. The `ToolRegistry<R, E>` generic requires a runtime type `R` that cannot be stored in a serialisable struct. Instead of threading the tool registry through the input types, we construct it inside the step's build_request closure where we have access to the agent's executor with tools pre-wired.

**Implementation**:
```rust
// In architect.rs - instead of extending StageKnowledgeSession
pub(super) fn step_with_knowledge_tools<C, R, E>(
    agent: &LlmAgent<C, R, E>,
    knowledge_groups: &[MaterialisedKnowledgeGroup],
    knowledge_backend: Arc<QdrantKnowledgeBackend<R>>,
) -> ArchitectStep<C, R, E>
where
    ...
{
    // Build tool registry from groups here, inject into executor
    let mut tools = ToolRegistry::new();
    for group in knowledge_groups {
        let qdrant_agent = knowledge_backend.agent_for(group)?;
        let search_tool = KnowledgeSearchTool::new(/* embedder */, 5, 0.7)
            .with_group(group.group.clone(), qdrant_agent.into_client());
        tools.register(search_tool)?;
    }

    // Create executor with tools and build json_task
    let executor = Executor::with_tools(agent.executor().client().clone(), tools);
    Step::builder(agent.task(/* ... */).with_executor(executor))
        // ...
}
```

### Decision 2: Architect uses `tool_task` instead of `json_task`

**Choice**: Replace `agent.json_task(...)` in architect with `agent.tool_task(...)` that wires in the knowledge tool registry.

**Rationale**: `json_task` is for structured JSON output only. `tool_task` supports tool calls and structured output. The architect will use `knowledge_search` during planning and return JSON at the end.

**Alternative considered**: Keep `json_task` and add a separate tool-calling round. This would require the architect to produce JSON, then do tool calls, then produce final JSON again. A single tool-aware step is cleaner.

### Decision 3: `knowledge_lint` runs as a validator step before materialisation

**Choice**: Add lint as a pre-materialisation validator, similar to how architect validation works.

**Rationale**: Lint findings are about the quality/integrity of the planned groups, not about what the model should plan. Running it as a separate step (not in the model loop) keeps concerns separated. If lint finds issues, the knowledge step can retry with those findings fed back.

**Implementation**:
```rust
.materialise_step_with_lint(backend, lint_config)?
```

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| Architect makes excessive tool calls, slowing planning | Set sensible defaults for `top_k` and `min_score`; limit tool scope to architect-relevant groups only |
| Tool registry serialization issues across async boundaries | Keep `ToolRegistry` as a concrete field, not serialised |
| Lint findings cause confusion if they're not actionable | Lint only reports structural issues; content quality remains the planner's responsibility |

## Open Questions

- Should `knowledge_search` be available to other stages (solutions, discovery) as well?
- Should the architect's system prompt indicate that `knowledge_search` is available, or is tool registration sufficient?
- What's the retry strategy if `knowledge_lint` finds issues—does the model re-plan, or does it surface the issues for human review?