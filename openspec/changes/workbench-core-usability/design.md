## Context

The MVP workbench is a single fixed project with a chat and DAG projection. That is sufficient for smoke testing, but it does not yet let users manage real projects or inspect why the runtime is behaving as it is.

## Goals / Non-Goals

**Goals:**
- Represent project, run, and session identity in the workbench state.
- Let users inspect and filter event history.
- Provide reset/archive operations that are explicit and safe.
- Improve status projection for roles, tasks, and DAG nodes.

**Non-Goals:**
- Multi-user collaboration or authentication.
- Full project management dashboards beyond the active project surface.

## Decisions

- Keep chat as the default surface. Event and DAG views are supporting inspection surfaces, not the primary landing page.
- Use semantic event context (`project_id`, `run_id`, `task_id`, correlation IDs) as the source of project/run scoping.
- Make reset/archive operations explicit API actions that emit semantic events where appropriate.

## Risks / Trade-offs

- More state can make the UI feel dashboard-heavy. Mitigation: default to the active lane/chat and keep management controls secondary.
- Reset operations can destroy useful context. Mitigation: prefer archive/new-run over destructive deletion, and require confirmation for destructive resets.
