## Context

Built-in roles can run without real LLM clients in some cases, but their usefulness is limited or fallback-driven. The UI must make that obvious and prevent users from assuming production-quality role output when dependencies are missing.

## Goals / Non-Goals

**Goals:**
- Show capability readiness per role.
- Expose configured LLM/tool provider status.
- Generate role-specific contracts instead of generic mention contracts.
- Improve Worker safety before casual implementation tasks are exposed.

**Non-Goals:**
- Selecting a single mandatory LLM provider.
- Rewriting all role prompts or planners.

## Decisions

- Represent readiness as visible state: configured, degraded, fallback, unavailable.
- Attach readiness details to role cards and task dispatch confirmations.
- Require Worker tasks to show target repository/worktree and validation commands before code execution.

## Risks / Trade-offs

- More status information can clutter the UI. Mitigation: compact badges with expandable detail.
- Readiness checks may be stale. Mitigation: refresh at runtime startup and after configuration changes.
