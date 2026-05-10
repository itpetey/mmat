## Context

Mentioning `@scholar` currently assigns a generic task. Mentioning `@reviewer` also assigns a generic task even though Reviewer consumes `ReviewRequested` and `TaskCompleted`. The UI needs to route human intent into the right semantic event shape.

## Goals / Non-Goals

**Goals:**
- Make human-to-role routing event-type aware.
- Expose the Librarian as a real running service rather than a placeholder UI row.
- Derive DAG/task state from runtime scheduler and semantic events.
- Prevent role event subscriptions from failing on irrelevant broadcasts.

**Non-Goals:**
- Rewriting role internals wholesale.
- Forcing every role to stay alive forever in this change.

## Decisions

- Introduce a routing layer in workbench message handling that maps mentions and actions to semantic event kinds.
- Treat Reviewer as review-request driven, not generic task driven.
- Run Librarian as a service bound to the same bus, memory store, and vector backend as the runtime.
- Keep auto-chaining configurable; the UI should expose what was dispatched rather than hiding it.

## Risks / Trade-offs

- More routing logic can become a second scheduler. Mitigation: keep the workbench as a translator from UI intent to semantic events; runtime remains authoritative for execution.
- Librarian startup requires vector backend choices. Mitigation: use existing hash/trait fallback where Postgres/Qdrant is not configured.
